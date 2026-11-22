use crate::Player;
use eframe::egui::{self, Key, Modifiers};

#[derive(Default)]
pub struct Interaction {
    last_activity: f64,
    fullscreen: bool,
}

fn visible(fullscreen: bool, active: bool, busy: bool, elapsed: f64) -> bool {
    !fullscreen || !active || busy || elapsed < 3.0
}

impl Interaction {
    pub fn show_controls(
        &mut self,
        ctx: &egui::Context,
        fullscreen: bool,
        active: bool,
        notice: bool,
    ) -> bool {
        let (time, activity, dragging, focused) = ctx.input(|input| {
            (
                input.time,
                input.events.iter().any(|event| {
                    matches!(
                        event,
                        egui::Event::PointerMoved(_)
                            | egui::Event::PointerButton { .. }
                            | egui::Event::Key { .. }
                            | egui::Event::MouseWheel { .. }
                    )
                }),
                input.pointer.any_down(),
                input.focused,
            )
        });
        if activity || fullscreen != self.fullscreen {
            self.last_activity = time;
        }
        self.fullscreen = fullscreen;
        visible(
            fullscreen,
            active,
            notice || dragging || !focused,
            time - self.last_activity,
        )
    }
}

impl Player {
    pub fn toggle_fullscreen(&self, ctx: &egui::Context) {
        let fullscreen = ctx.input(|input| input.viewport().fullscreen.unwrap_or(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(!fullscreen));
    }

    pub fn shortcuts(&mut self, ctx: &egui::Context) {
        let pressed = |modifiers, key| ctx.input_mut(|input| input.consume_key(modifiers, key));
        if pressed(Modifiers::NONE, Key::Escape) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::CursorVisible(true));
        }
        if pressed(Modifiers::COMMAND, Key::O) {
            self.open();
        }
        if ctx.wants_keyboard_input() {
            return;
        }
        if pressed(Modifiers::NONE, Key::F) {
            self.toggle_fullscreen(ctx);
        }
        if pressed(Modifiers::NONE, Key::Space) {
            self.toggle_play();
        }
        for (key, command) in [(Key::Comma, "frame-back-step"), (Key::Period, "frame-step")] {
            if pressed(Modifiers::NONE, key) && self.playing {
                self.rewinding = false;
                self.speed = 1.0;
                self.command(&["set", "speed", "1"]);
                self.command(&["set", "pause", "yes"]);
                self.command(&[command]);
            }
        }
        if pressed(Modifiers::NONE, Key::R) && self.playing {
            self.rotation = (self.rotation + 90) % 360;
        }
        if pressed(Modifiers::NONE, Key::P)
            && !ctx.input(|input| input.viewport().fullscreen.unwrap_or(false))
        {
            self.show_playlist = !self.show_playlist;
        }
        for (key, offset) in [(Key::ArrowLeft, -5.0), (Key::ArrowRight, 5.0)] {
            if pressed(Modifiers::NONE, key) {
                self.seek(offset);
            }
        }
        for (key, delta) in [(Key::ArrowUp, 5.0), (Key::ArrowDown, -5.0)] {
            if pressed(Modifiers::NONE, key) {
                self.volume = (self.volume + delta).clamp(0.0, 100.0);
                self.command(&["set", "volume", &self.volume.to_string()]);
            }
        }
        for (key, next) in [(Key::PageUp, false), (Key::PageDown, true)] {
            if pressed(Modifiers::NONE, key) {
                if let Some(index) = self.selected {
                    self.play(if next {
                        index + 1
                    } else {
                        index.saturating_sub(1)
                    });
                }
            }
        }
        for (key, change) in [
            (Key::OpenBracket, -0.25),
            (Key::CloseBracket, 0.25),
            (Key::Backslash, 0.0),
        ] {
            if pressed(Modifiers::NONE, key) && self.playing {
                self.speed = if change == 0.0 {
                    1.0
                } else {
                    (self.speed + change).clamp(0.25, 4.0)
                };
                self.rewinding = false;
                self.command(&["set", "speed", &self.speed.to_string()]);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fullscreen_timeout_does_not_hide_paused_or_interacting_controls() {
        assert!(visible(false, true, false, 10.0));
        assert!(visible(true, false, false, 10.0));
        assert!(visible(true, true, true, 10.0));
        assert!(visible(true, true, false, 2.9));
        assert!(!visible(true, true, false, 3.0));
    }
}
