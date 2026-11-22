//! Opus for symphonia, which demuxes it (Ogg, Matroska) but has no decoder of its own.
//!
//! The decoding is libopus 1.3.1 itself, translated to Rust (`unsafe-libopus`): the same
//! output as ffmpeg's libopus decode, mono to 7.1, with no C toolchain on any platform.
//! Everything goes through the multistream decoder at 48 kHz, so mono, stereo and surround
//! take one path.

use symphonia::core::audio::{AsGenericAudioBufferRef, AudioBuffer, AudioMut, AudioSpec, Channels, GenericAudioBufferRef, Position};
use symphonia::core::codecs::CodecInfo;
use symphonia::core::codecs::audio::well_known::CODEC_ID_OPUS;
use symphonia::core::codecs::audio::{AudioCodecParameters, AudioDecoder, AudioDecoderOptions, FinalizeResult};
use symphonia::core::codecs::registry::{RegisterableAudioDecoder, SupportedAudioCodec};
use symphonia::core::errors::{Result, decode_error, unsupported_error};
use symphonia::core::packet::PacketRef;
use unsafe_libopus::{OpusMSDecoder, opus_multistream_decode_float, opus_multistream_decoder_create, opus_multistream_decoder_destroy, opus_multistream_decoder_init};

/// Opus always decodes at 48 kHz, whatever rate the file was made from.
pub const RATE: u32 = 48_000;

/// Frames to decode before a seek point and drop. RFC 7845 asks for 80 ms, but measured against
/// a decode from the start that leaves music up to −37 dB off for a quarter of a second (and
/// speech-mode files for two seconds); after one second the output is the same from the first
/// frame on every sample file. It costs about a millisecond.
pub(crate) const PRE_ROLL: usize = 48_000;

/// The longest packet: 120 ms at 48 kHz.
const MAX_FRAMES: usize = 5760;

/// An Opus identification header (RFC 7845 §5.1), as Ogg carries it in its first packet and
/// Matroska in CodecPrivate.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Head {
    pub channels: usize,
    /// Frames of encoder delay at the start, at 48 kHz.
    pub pre_skip: usize,
    /// Gain to apply on output, in dB (Q7.8 in the file).
    pub gain_db: f32,
    pub streams: u8,
    pub coupled: u8,
    /// For each output channel, the decoded channel it plays (255: silence).
    pub mapping: Vec<u8>,
}

impl Head {
    pub(crate) fn parse(b: &[u8]) -> Result<Head> {
        if b.len() < 19 || &b[..8] != b"OpusHead" {
            return unsupported_error("opus: no OpusHead");
        }
        // Versions 0–15 are compatible with the one we know (the low nibble is the minor one).
        if b[8] > 15 {
            return unsupported_error("opus: unknown OpusHead version");
        }
        let channels = b[9] as usize;
        if channels == 0 {
            return decode_error("opus: no channels");
        }
        let pre_skip = u16::from_le_bytes([b[10], b[11]]) as usize;
        let gain_db = i16::from_le_bytes([b[16], b[17]]) as f32 / 256.0;
        let (streams, coupled, mapping) = match b[18] {
            // RTP's mapping: one stream, mono or a coupled stereo pair.
            0 => match channels {
                1 => (1, 0, vec![0]),
                2 => (1, 1, vec![0, 1]),
                _ => return decode_error("opus: family 0 is mono or stereo"),
            },
            // Vorbis channel order, up to 7.1.
            1 => {
                if channels > 8 {
                    return decode_error("opus: family 1 is up to 8 channels");
                }
                let Some(table) = b.get(19..21 + channels) else {
                    return decode_error("opus: truncated channel mapping");
                };
                let (streams, coupled) = (table[0], table[1]);
                if streams == 0 || coupled > streams || streams as usize + coupled as usize > 255 {
                    return decode_error("opus: bad stream counts");
                }
                let mapping = table[2..].to_vec();
                // Each channel plays one of the decoded channels, or silence.
                let decoded = streams as usize + coupled as usize;
                if mapping.iter().any(|&m| m != 255 && m as usize >= decoded) {
                    return decode_error("opus: bad channel mapping");
                }
                (streams, coupled, mapping)
            }
            // Ambisonics and the reserved families: not for playback.
            _ => return unsupported_error("opus: unsupported channel mapping family"),
        };
        Ok(Head { channels, pre_skip, gain_db, streams, coupled, mapping })
    }

    /// Where the speakers are: the same sets symphonia's Ogg demuxer reports.
    fn positions(&self) -> Position {
        use Position as P;
        match self.channels {
            1 => P::FRONT_LEFT,
            2 => P::FRONT_LEFT | P::FRONT_RIGHT,
            3 => P::FRONT_LEFT | P::FRONT_CENTER | P::FRONT_RIGHT,
            4 => P::FRONT_LEFT | P::FRONT_RIGHT | P::REAR_LEFT | P::REAR_RIGHT,
            5 => P::FRONT_LEFT | P::FRONT_CENTER | P::FRONT_RIGHT | P::REAR_LEFT | P::REAR_RIGHT,
            6 => P::FRONT_LEFT | P::FRONT_CENTER | P::FRONT_RIGHT | P::REAR_LEFT | P::REAR_RIGHT | P::LFE1,
            7 => P::FRONT_LEFT | P::FRONT_CENTER | P::FRONT_RIGHT | P::SIDE_LEFT | P::SIDE_RIGHT | P::REAR_CENTER | P::LFE1,
            _ => P::FRONT_LEFT | P::FRONT_CENTER | P::FRONT_RIGHT | P::SIDE_LEFT | P::SIDE_RIGHT | P::REAR_LEFT | P::REAR_RIGHT | P::LFE1,
        }
    }
}

/// The plane (symphonia's order for the channel set) of each channel libopus outputs (Vorbis
/// order) — the permutation of symphonia's Vorbis decoder.
fn plane_of(channels: usize, ch: usize) -> usize {
    let order: &[usize] = match channels {
        3 => &[0, 2, 1],                // FL, FC, FR
        5 => &[0, 2, 1, 3, 4],          // FL, FC, FR, RL, RR
        6 => &[0, 2, 1, 4, 5, 3],       // FL, FC, FR, RL, RR, LFE
        7 => &[0, 2, 1, 5, 6, 4, 3],    // FL, FC, FR, SL, SR, RC, LFE
        8 => &[0, 2, 1, 6, 7, 4, 5, 3], // FL, FC, FR, SL, SR, RL, RR, LFE
        _ => return ch,                 // mono, stereo, quad: already in order
    };
    order[ch]
}

/// The encoder delay of an Opus track, in frames: what the container states (Ogg), else the
/// OpusHead's pre-skip (symphonia's Matroska reader doesn't pass CodecDelay on).
pub(crate) fn delay(container: Option<u32>, params: &AudioCodecParameters) -> usize {
    container.map(|d| d as usize).or_else(|| params.extra_data.as_deref().and_then(|b| Head::parse(b).ok()).map(|h| h.pre_skip)).unwrap_or(0)
}

pub struct OpusDecoder {
    params: AudioCodecParameters,
    opts: AudioDecoderOptions,
    head: Head,
    /// libopus' state, owned: made in `try_new`, destroyed in `drop`.
    st: *mut OpusMSDecoder,
    /// libopus' interleaved output for one packet.
    pcm: Vec<f32>,
    /// The header's output gain, linear.
    gain: f32,
    buf: AudioBuffer<f32>,
}

// SAFETY: `st` belongs to this decoder alone and is only used through `&mut self` (decode,
// reset, drop), so moving the decoder to another thread or sharing `&OpusDecoder` (whose
// methods never touch `st`) is fine. libopus keeps no global mutable state: its static tables
// are only read.
unsafe impl Send for OpusDecoder {}
unsafe impl Sync for OpusDecoder {}

impl OpusDecoder {
    pub fn try_new(params: &AudioCodecParameters, opts: &AudioDecoderOptions) -> Result<Self> {
        if params.codec != CODEC_ID_OPUS {
            return unsupported_error("opus: invalid codec");
        }
        let Some(extra) = params.extra_data.as_deref() else {
            return unsupported_error("opus: missing OpusHead");
        };
        let head = Head::parse(extra)?;
        // The engine interleaves by the container's count: both must agree.
        if params.channels.as_ref().is_some_and(|c| c.count() != head.channels) {
            return decode_error("opus: channel count differs from the container's");
        }
        let mut err = 0;
        // SAFETY: the counts and the mapping (one entry per channel) were checked in `parse`;
        // libopus copies the mapping and checks it again.
        let st = unsafe {
            opus_multistream_decoder_create(RATE as i32, head.channels as i32, head.streams as i32, head.coupled as i32, head.mapping.as_ptr(), &mut err)
        };
        if st.is_null() || err != 0 {
            if !st.is_null() {
                // SAFETY: made just above, used nowhere else.
                unsafe { opus_multistream_decoder_destroy(st) };
            }
            return unsupported_error("opus: libopus refused the stream layout");
        }
        let spec = AudioSpec::new(RATE, Channels::Positioned(head.positions()));
        Ok(OpusDecoder {
            params: params.clone(),
            opts: *opts,
            pcm: vec![0.0; MAX_FRAMES * head.channels],
            // RFC 7845 §5.1: players must apply it (ffmpeg does).
            gain: 10f32.powf(head.gain_db / 20.0),
            head,
            st,
            buf: AudioBuffer::new(spec, MAX_FRAMES),
        })
    }

    fn decode_inner(&mut self, packet: &PacketRef<'_>) -> Result<()> {
        self.buf.clear();
        // An empty packet would make libopus conceal a loss: 120 ms of made-up audio.
        if packet.data.is_empty() {
            return decode_error("opus: empty packet");
        }
        let Ok(len) = i32::try_from(packet.data.len()) else {
            return decode_error("opus: packet too long");
        };
        // SAFETY: `st` is live; `data` holds `len` bytes; `pcm` has room for MAX_FRAMES frames
        // of every channel, the most libopus writes when told that frame size.
        let n = unsafe { opus_multistream_decode_float(self.st, packet.data.as_ptr(), len, self.pcm.as_mut_ptr(), MAX_FRAMES as i32, 0) };
        if n < 0 {
            return decode_error("opus: undecodable packet");
        }
        let n = (n as usize).min(MAX_FRAMES);
        let ch = self.head.channels;
        self.buf.render_uninit(Some(n));
        for c in 0..ch {
            let Some(plane) = self.buf.plane_mut(plane_of(ch, c)) else {
                return decode_error("opus: missing audio plane");
            };
            for (out, frame) in plane.iter_mut().zip(self.pcm[..n * ch].chunks_exact(ch)) {
                *out = frame[c] * self.gain;
            }
        }
        // The Ogg demuxer puts the end padding in the last packet; the start (pre-skip) is
        // left to the engine (see `decode::open`).
        if self.opts.gapless {
            self.buf.trim(packet.trim_start.get() as usize, packet.trim_end.get() as usize);
        }
        Ok(())
    }
}

impl Drop for OpusDecoder {
    fn drop(&mut self) {
        // SAFETY: made in `try_new`, owned by us, not used after this.
        unsafe { opus_multistream_decoder_destroy(self.st) };
    }
}

impl AudioDecoder for OpusDecoder {
    fn reset(&mut self) {
        // Back to a fresh decoder (after a seek), in the memory it already has.
        // SAFETY: `st` was made for exactly these counts, so it is big enough; init checks
        // the arguments again and clears every stream's state.
        let err = unsafe {
            opus_multistream_decoder_init(self.st, RATE as i32, self.head.channels as i32, self.head.streams as i32, self.head.coupled as i32, self.head.mapping.as_ptr())
        };
        if err != 0 {
            log::warn!("opus: reset failed ({err})");
        }
        self.buf.clear();
    }

    fn codec_info(&self) -> &CodecInfo {
        &Self::supported_codecs()[0].info
    }

    fn codec_params(&self) -> &AudioCodecParameters {
        &self.params
    }

    fn decode_ref(&mut self, packet: &PacketRef<'_>) -> Result<GenericAudioBufferRef<'_>> {
        if let Err(e) = self.decode_inner(packet) {
            self.buf.clear();
            return Err(e);
        }
        Ok(self.buf.as_generic_audio_buffer_ref())
    }

    fn finalize(&mut self) -> FinalizeResult {
        Default::default()
    }

    fn last_decoded(&self) -> GenericAudioBufferRef<'_> {
        self.buf.as_generic_audio_buffer_ref()
    }
}

impl RegisterableAudioDecoder for OpusDecoder {
    fn try_registry_new(params: &AudioCodecParameters, opts: &AudioDecoderOptions) -> Result<Box<dyn AudioDecoder>>
    where
        Self: Sized,
    {
        Ok(Box::new(OpusDecoder::try_new(params, opts)?))
    }

    fn supported_codecs() -> &'static [SupportedAudioCodec] {
        &[SupportedAudioCodec { id: CODEC_ID_OPUS, info: CodecInfo { short_name: "opus", long_name: "Opus", profiles: &[] } }]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use symphonia::core::units::{Duration, Timestamp};

    fn head(channels: u8, family: u8, table: &[u8]) -> Vec<u8> {
        let mut b = b"OpusHead".to_vec();
        b.extend_from_slice(&[1, channels]);
        b.extend_from_slice(&312u16.to_le_bytes());
        b.extend_from_slice(&44_100u32.to_le_bytes());
        b.extend_from_slice(&(-512i16).to_le_bytes()); // −2 dB
        b.push(family);
        b.extend_from_slice(table);
        b
    }

    #[test]
    fn a_stereo_head_is_one_coupled_stream() {
        let h = Head::parse(&head(2, 0, &[])).unwrap();
        assert_eq!(h, Head { channels: 2, pre_skip: 312, gain_db: -2.0, streams: 1, coupled: 1, mapping: vec![0, 1] });
        assert_eq!(Head::parse(&head(1, 0, &[])).unwrap().mapping, vec![0]);
    }

    #[test]
    fn a_surround_head_has_its_mapping() {
        // What libopus writes for 5.1: four streams, two of them pairs.
        let h = Head::parse(&head(6, 1, &[4, 2, 0, 4, 1, 2, 3, 5])).unwrap();
        assert_eq!((h.streams, h.coupled, h.mapping), (4, 2, vec![0, 4, 1, 2, 3, 5]));
        // A silent channel (255) is allowed.
        assert!(Head::parse(&head(2, 1, &[1, 0, 0, 255])).is_ok());
    }

    #[test]
    fn broken_heads_are_refused() {
        let good = head(6, 1, &[4, 2, 0, 4, 1, 2, 3, 5]);
        // Cut anywhere short of the whole mapping.
        for len in 0..good.len() {
            assert!(Head::parse(&good[..len]).is_err(), "{len} bytes");
        }
        let mut magic = good.clone();
        magic[0] = b'X';
        assert!(Head::parse(&magic).is_err());
        let mut version = good.clone();
        version[8] = 16;
        assert!(Head::parse(&version).is_err());
        assert!(Head::parse(&head(0, 0, &[])).is_err());
        // Family 0 is mono or stereo; family 1 up to 8 channels; others are not for playback.
        assert!(Head::parse(&head(3, 0, &[])).is_err());
        assert!(Head::parse(&head(9, 1, &[5, 4, 0, 1, 2, 3, 4, 5, 6, 7, 8])).is_err());
        assert!(Head::parse(&head(2, 2, &[1, 1, 0, 1])).is_err());
        assert!(Head::parse(&head(2, 255, &[1, 1, 0, 1])).is_err());
        // Bad stream counts, and a channel reading a stream that isn't there.
        assert!(Head::parse(&head(2, 1, &[0, 0, 0, 1])).is_err());
        assert!(Head::parse(&head(2, 1, &[1, 2, 0, 1])).is_err());
        assert!(Head::parse(&head(2, 1, &[1, 0, 0, 1])).is_err());
        assert!(Head::parse(&head(6, 1, &[4, 2, 0, 4, 1, 2, 3, 6])).is_err());
    }

    #[test]
    fn channels_land_on_symphonias_planes() {
        // 5.1 in Vorbis order (FL FC FR RL RR LFE) to symphonia's (FL FR FC LFE RL RR).
        let planes: Vec<usize> = (0..6).map(|c| plane_of(6, c)).collect();
        assert_eq!(planes, [0, 2, 1, 4, 5, 3]);
        // Every count is a permutation, and the speakers named match the planes.
        for n in 1..=8 {
            let mut seen: Vec<usize> = (0..n).map(|c| plane_of(n, c)).collect();
            seen.sort();
            assert_eq!(seen, (0..n).collect::<Vec<_>>(), "{n} channels");
            let h = Head { channels: n, pre_skip: 0, gain_db: 0.0, streams: 1, coupled: 0, mapping: vec![] };
            assert_eq!(h.positions().bits().count_ones() as usize, n);
        }
        // The LFE of 7.1 is Vorbis' last channel and symphonia's fourth plane.
        assert_eq!(plane_of(8, 7), 3);
    }

    #[test]
    fn decodes_a_packet_and_refuses_garbage() {
        let mut params = AudioCodecParameters::new();
        params.for_codec(CODEC_ID_OPUS).with_sample_rate(RATE).with_extra_data(head(2, 0, &[]).into_boxed_slice());
        let mut d = OpusDecoder::try_new(&params, &AudioDecoderOptions::default()).unwrap();
        let packet = |data| PacketRef::new(0, Timestamp::new(0), Duration::ZERO, data);
        // TOC 0xFC: CELT fullband 20 ms stereo, one frame; an empty frame is digital silence.
        let ok = packet(&[0xFC]);
        let buf = d.decode_ref(&ok).unwrap();
        assert_eq!(buf.frames(), 960);
        for data in [&[][..], &[0xFF, 0xFF][..]] {
            assert!(d.decode_ref(&packet(data)).is_err());
            assert_eq!(d.last_decoded().frames(), 0);
        }
        d.reset();
        assert_eq!(d.decode_ref(&ok).unwrap().frames(), 960);
    }
}

/// Against ffmpeg's libopus decode, on the files in $BITSPRING_OPUS_SAMPLES (skipped when it
/// isn't set): `.opus` files, and `.mka` ones with Opus inside.
#[cfg(test)]
mod files {
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::sync::Arc;
    use std::time::Duration;

    use crate::decode::Track;

    fn samples(exts: &[&str]) -> Vec<PathBuf> {
        let Ok(dir) = std::env::var("BITSPRING_OPUS_SAMPLES") else {
            eprintln!("BITSPRING_OPUS_SAMPLES not set, skipping");
            return Vec::new();
        };
        let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| p.extension().and_then(|e| e.to_str()).is_some_and(|e| exts.contains(&e)))
            .filter(|p| !p.file_name().unwrap().to_string_lossy().starts_with('.'))
            .collect();
        v.sort();
        v
    }

    /// ffmpeg's decode with libopus in float, interleaved in its channel order (5.1: FL FR FC
    /// LFE BL BR, which is ours).
    fn ffmpeg(path: &Path) -> Vec<f32> {
        let out = Command::new("ffmpeg")
            .args(["-hide_banner", "-loglevel", "error", "-c:a", "libopus", "-request_sample_fmt", "flt", "-i"])
            .arg(path)
            .args(["-f", "f32le", "-c:a", "pcm_f32le", "-"])
            .output()
            .expect("ffmpeg");
        assert!(out.status.success(), "ffmpeg: {}", String::from_utf8_lossy(&out.stderr));
        out.stdout.as_chunks::<4>().0.iter().map(|b| f32::from_le_bytes(*b)).collect()
    }

    /// The file decoded as the app plays it, to the end.
    fn ours(path: &Path, start_secs: f64) -> Arc<Track> {
        let t = crate::decode::open_track_at(path, start_secs).unwrap();
        assert!(t.wait_for(usize::MAX, Duration::from_secs(600)) && t.is_complete(), "{}: not done", path.display());
        assert_eq!(t.error(), None, "{}", path.display());
        t
    }

    /// The largest difference between `t` from frame `from` on and `want` (from its start), in
    /// dBFS.
    fn max_diff_db(t: &Track, from: usize, want: &[f32]) -> f64 {
        let ch = t.channels;
        let mut max = 0f64;
        for (i, w) in want.chunks_exact(ch).enumerate() {
            for (c, &w) in w.iter().enumerate() {
                max = max.max((t.sample(from + i, c) as f64 - w as f64).abs());
            }
        }
        20.0 * max.max(1e-12).log10()
    }

    #[test]
    fn matches_ffmpeg() {
        for p in samples(&["opus", "mka"]) {
            let t = ours(&p, 0.0);
            assert_eq!(t.sample_rate, super::RATE);
            let want = ffmpeg(&p);
            let theirs = want.len() / t.channels;
            let mka = p.extension().is_some_and(|e| e == "mka");
            eprintln!("{}: {} ch, frames ours {} / ffmpeg {}", p.display(), t.channels, t.frames(), theirs);
            if mka {
                // symphonia's Matroska reader doesn't report the end padding (at most 20 ms).
                assert!((theirs..=theirs + 960).contains(&t.frames()), "{}: length", p.display());
            } else {
                assert_eq!(t.frames(), theirs, "{}: length", p.display());
            }
            let db = max_diff_db(&t, 0, &want[..theirs.min(t.frames()) * t.channels]);
            eprintln!("  max diff {db:.1} dB");
            assert!(db < -120.0, "{}: differs from ffmpeg by {db:.1} dB", p.display());
        }
    }

    /// Opened in the middle (a seek far ahead, or a resume), the audio is the same as there in
    /// a decode from the start, from the first frame: that is what the one-second pre-roll is
    /// for. Matroska keeps time in whole milliseconds, so there the seek lands within a couple
    /// of them, and the comparison is of audio a few frames apart.
    #[test]
    fn opened_in_the_middle_matches_a_full_decode() {
        for p in samples(&["opus", "mka"]) {
            let full = ours(&p, 0.0);
            let len = full.frames();
            let secs = if len > 40 * 48_000 { 30.0 } else { (len / 2 / 48_000) as f64 };
            let at = ours(&p, secs);
            let from = (secs * 48_000.0) as usize;
            assert_eq!(at.held_from(), from, "{}", p.display());
            // How many frames late the audio at `from` is (0 but for Matroska).
            let late = at.frames() as i64 - len as i64;
            let mka = p.extension().is_some_and(|e| e == "mka");
            assert!(late == 0 || mka && late.abs() <= 96, "{}: {late} frames off", p.display());
            let ch = full.channels;
            let diff = |f: usize| (0..ch).map(|c| (at.sample(f, c) - full.sample((f as i64 - late) as usize, c)).abs()).fold(0f32, f32::max) as f64;
            let db = |x: f64| 20.0 * x.max(1e-12).log10();
            let first = db((from..from + 48_000).map(diff).fold(0.0, f64::max));
            // The last frame still more than −120 dB off: the decoder has caught up after it.
            let caught_up = (from..at.frames().min(len)).rev().find(|&f| db(diff(f)) > -120.0).map_or(0, |f| f + 1 - from);
            eprintln!("{} from {secs} s: {late} frames late, max diff {first:.1} dB in the first second, exact (< −120 dB) after {} ms", p.display(), caught_up / 48);
            if mka {
                assert!(first < -30.0, "{}: differs by {first:.1} dB after a seek", p.display());
            } else {
                assert!(first < -120.0 && caught_up == 0, "{}: differs by {first:.1} dB after a seek, for {} ms", p.display(), caught_up / 48);
            }
        }
    }

    /// Tags as ffprobe reads them: (title, artist, album).
    fn ffprobe_tags(path: &Path) -> [Option<String>; 3] {
        let out = Command::new("ffprobe")
            .args(["-v", "error", "-show_entries", "stream_tags=title,artist,album:format_tags=title,artist,album", "-of", "default=nw=1"])
            .arg(path)
            .output()
            .expect("ffprobe");
        let text = String::from_utf8_lossy(&out.stdout).into_owned();
        let tag = |key: &str| {
            text.lines().find_map(|l| {
                let (k, v) = l.strip_prefix("TAG:")?.split_once('=')?;
                k.eq_ignore_ascii_case(key).then(|| v.to_string())
            })
        };
        [tag("title"), tag("artist"), tag("album")]
    }

    /// Opus in a file named .ogg is still Opus: the library goes by the codec.
    #[test]
    fn an_ogg_named_opus_file_scans_as_opus() {
        let Some(p) = samples(&["opus"]).into_iter().next() else { return };
        let dir = std::env::temp_dir().join(format!("hifi-opus-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let copy = dir.join("track.ogg");
        std::fs::copy(&p, &copy).unwrap();
        let frames = ffmpeg(&p).len() / ours(&p, 0.0).channels;
        let [title, artist, album] = ffprobe_tags(&p);
        assert!(title.is_some() && artist.is_some() && album.is_some(), "the sample has no tags");
        for f in [&p, &copy] {
            let (info, _) = crate::library::scan_file(f).unwrap();
            assert_eq!((info.format.as_str(), info.lossless, info.sample_rate, info.bits), ("Opus", false, 48_000, None), "{}", f.display());
            assert_eq!(info.duration, frames as f64 / 48_000.0, "{}", f.display());
            assert_eq!((Some(info.title), &info.artist, &info.album), (title.clone(), &artist, &album), "{}", f.display());
            eprintln!("{}: {} / {} s", f.display(), info.format, info.duration);
        }
        // And it plays.
        assert_eq!(ours(&copy, 0.0).frames(), frames);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

