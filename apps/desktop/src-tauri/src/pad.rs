//! Game controllers on Linux (gamepads, handhelds' built-in controls): read here rather than
//! through the web view's Gamepad API, which WebKitGTK builds may leave out. Each press goes to the window as a `pad` event naming the button; the UI moves
//! its focus and plays/pauses with them (see `gamepad.ts`).
//!
//! Directions (D-pad or left stick) repeat while held, like keys. The right stick scrolls.

use std::time::{Duration, Instant};

use gilrs::{Axis, Button, EventType, Gilrs};
use tauri::{AppHandle, Emitter};

/// First repeat of a held direction, then the pace of the next ones.
const REPEAT_AFTER: Duration = Duration::from_millis(380);
const REPEAT_EVERY: Duration = Duration::from_millis(110);
/// How far a stick must lean to count as a direction (and to let go of one).
const STICK_ON: f32 = 0.6;
const STICK_OFF: f32 = 0.35;

fn name(b: Button) -> Option<&'static str> {
    Some(match b {
        Button::South => "a",
        Button::East => "b",
        Button::West => "x",
        Button::North => "y",
        Button::LeftTrigger => "lb",
        Button::RightTrigger => "rb",
        Button::LeftTrigger2 => "lt",
        Button::RightTrigger2 => "rt",
        Button::Start => "start",
        Button::Select => "select",
        Button::DPadUp => "up",
        Button::DPadDown => "down",
        Button::DPadLeft => "left",
        Button::DPadRight => "right",
        _ => return None,
    })
}

/// The direction a stick points at, with hysteresis around `current`.
fn stick_dir(x: f32, y: f32, current: Option<&'static str>) -> Option<&'static str> {
    let reach = if current.is_some() { STICK_OFF } else { STICK_ON };
    if x.abs().max(y.abs()) < reach {
        return None;
    }
    // gilrs: up is positive Y.
    Some(if x.abs() > y.abs() {
        if x > 0.0 { "right" } else { "left" }
    } else if y > 0.0 {
        "up"
    } else {
        "down"
    })
}

pub fn install(app: &AppHandle) {
    let app = app.clone();
    let _ = std::thread::Builder::new().name("hifi-pad".into()).spawn(move || {
        let Ok(mut gilrs) = Gilrs::new() else { return };
        // The direction held (D-pad button or stick), and when it should repeat next.
        let mut held: Option<(&'static str, Instant)> = None;
        let (mut lx, mut ly, mut ry) = (0f32, 0f32, 0f32);
        let mut stick: Option<&'static str> = None;
        let mut last_scroll = Instant::now();
        loop {
            // One event per turn, so a stream of stick events can't hold up repeats and scrolling.
            if let Some(ev) = gilrs.next_event_blocking(Some(Duration::from_millis(16))) {
                match ev.event {
                    EventType::ButtonPressed(b, _) => {
                        let Some(n) = name(b) else { continue };
                        if matches!(n, "up" | "down" | "left" | "right") {
                            held = Some((n, Instant::now() + REPEAT_AFTER));
                        }
                        let _ = app.emit("pad", n);
                    }
                    EventType::ButtonReleased(b, _) => {
                        if held.is_some_and(|(n, _)| Some(n) == name(b)) {
                            held = None;
                        }
                    }
                    EventType::AxisChanged(axis, v, _) => {
                        match axis {
                            Axis::LeftStickX => lx = v,
                            Axis::LeftStickY => ly = v,
                            Axis::RightStickY => ry = v,
                            _ => continue,
                        }
                        let now = stick_dir(lx, ly, stick);
                        if now != stick {
                            stick = now;
                            match now {
                                Some(n) => {
                                    held = Some((n, Instant::now() + REPEAT_AFTER));
                                    let _ = app.emit("pad", n);
                                }
                                None => held = None,
                            }
                        }
                    }
                    _ => {}
                }
            }
            let now = Instant::now();
            if let Some((n, at)) = held {
                if now >= at {
                    held = Some((n, now + REPEAT_EVERY));
                    let _ = app.emit("pad", n);
                }
            }
            // Right stick: scroll, faster the further it leans.
            if ry.abs() > 0.2 && now.duration_since(last_scroll) >= Duration::from_millis(16) {
                last_scroll = now;
                let _ = app.emit("pad-scroll", -ry * ry.abs() * 40.0);
            }
        }
    });
}
