//! The play queue and its order of play: what follows the current track by itself, and where
//! the next / previous buttons lead.
//!
//! Shuffle isn't worked out here: the UI reorders the queue it mirrors to us, so the order
//! shown is the order played, and the next track is known ahead (gapless needs it).

use serde::{Deserialize, Serialize};

/// What happens at the end of a track.
#[derive(Deserialize, Serialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Repeat {
    /// The queue plays through once and stops.
    #[default]
    Off,
    /// After the last track, the first again.
    All,
    /// The same track, over and over.
    One,
}

/// The play queue, mirrored from the UI so the next track starts from Rust — on time, and
/// also while the UI sleeps (a locked iPhone suspends the web view, not the audio).
#[derive(Default)]
pub struct Queue {
    pub paths: Vec<String>,
    pub index: Option<usize>,
    pub repeat: Repeat,
    /// The UI shuffled `paths`: tracks of different albums follow each other.
    pub shuffle: bool,
}

impl Queue {
    /// The entry that plays by itself when the current track ends (None: the queue stops).
    pub fn following(&self) -> Option<usize> {
        let (i, len) = (self.index?, self.paths.len());
        let next = match self.repeat {
            Repeat::One => i,
            Repeat::All if len > 0 => (i + 1) % len,
            Repeat::All | Repeat::Off => i + 1,
        };
        (next < len).then_some(next)
    }

    /// Where "next" (+1) or "previous" (−1) leads from the current track (None: nowhere).
    /// Repeating the queue joins its ends; repeating one track doesn't hold a button back.
    pub fn step(&self, delta: i64) -> Option<usize> {
        let (i, len) = (self.index?, self.paths.len());
        if len == 0 {
            return None;
        }
        let to = i as i64 + delta;
        if self.repeat == Repeat::All {
            return Some(to.rem_euclid(len as i64) as usize);
        }
        usize::try_from(to).ok().filter(|&to| to < len)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn queue(len: usize, index: Option<usize>, repeat: Repeat) -> Queue {
        Queue { paths: (0..len).map(|i| format!("{i}.flac")).collect(), index, repeat, shuffle: false }
    }

    #[test]
    fn without_repeat_the_queue_plays_through_and_stops() {
        assert_eq!(queue(3, Some(0), Repeat::Off).following(), Some(1));
        assert_eq!(queue(3, Some(2), Repeat::Off).following(), None);
        assert_eq!(queue(3, None, Repeat::Off).following(), None);
        assert_eq!(queue(0, None, Repeat::Off).following(), None);
    }

    #[test]
    fn repeating_the_queue_goes_from_the_last_track_to_the_first() {
        assert_eq!(queue(3, Some(1), Repeat::All).following(), Some(2));
        assert_eq!(queue(3, Some(2), Repeat::All).following(), Some(0));
        // A queue of one track repeats that track.
        assert_eq!(queue(1, Some(0), Repeat::All).following(), Some(0));
        assert_eq!(queue(3, None, Repeat::All).following(), None);
    }

    #[test]
    fn repeating_one_track_follows_it_with_itself() {
        assert_eq!(queue(3, Some(1), Repeat::One).following(), Some(1));
        assert_eq!(queue(3, Some(2), Repeat::One).following(), Some(2));
    }

    /// The queue shrank under the playing track (the UI removed entries): nothing follows an
    /// index that is no longer there.
    #[test]
    fn an_index_past_the_end_is_followed_by_nothing() {
        assert_eq!(queue(2, Some(5), Repeat::One).following(), None);
        assert_eq!(queue(2, Some(5), Repeat::Off).following(), None);
        assert_eq!(queue(0, Some(0), Repeat::All).following(), None);
    }

    #[test]
    fn the_buttons_stop_at_the_ends_unless_the_queue_repeats() {
        for repeat in [Repeat::Off, Repeat::One] {
            assert_eq!(queue(3, Some(1), repeat).step(1), Some(2));
            assert_eq!(queue(3, Some(1), repeat).step(-1), Some(0));
            assert_eq!(queue(3, Some(2), repeat).step(1), None);
            assert_eq!(queue(3, Some(0), repeat).step(-1), None);
        }
        assert_eq!(queue(3, Some(2), Repeat::All).step(1), Some(0));
        assert_eq!(queue(3, Some(0), Repeat::All).step(-1), Some(2));
        assert_eq!(queue(3, None, Repeat::All).step(1), None);
        assert_eq!(queue(0, Some(0), Repeat::All).step(1), None);
    }
}
