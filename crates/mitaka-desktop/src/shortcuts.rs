use crate::commands::Command;
use eframe::egui::{self, Event, ImeEvent, Key, Modifiers};

fn command_only(modifiers: Modifiers) -> bool {
    modifiers.matches_exact(Modifiers::COMMAND) && !(modifiers.mac_cmd && modifiers.ctrl)
}

fn main_key(key: Key, m: Modifiers, chart: bool) -> Option<Command> {
    if (key == Key::F && m.matches_exact(Modifiers::MAC_CMD | Modifiers::CTRL))
        || (key == Key::F11 && m.is_none() && !cfg!(target_os = "macos"))
    {
        return Some(Command::Fullscreen);
    }
    if command_only(m) {
        return match key {
            Key::O => Some(Command::Open),
            Key::S => Some(Command::SaveState),
            Key::F => Some(Command::Objects),
            Key::Q if !cfg!(target_os = "macos") => Some(Command::Quit),
            _ => None,
        };
    }
    match key {
        Key::S if m.is_none() => Some(Command::Land),
        Key::Plus | Key::Equals if chart && (m.is_none() || m == Modifiers::SHIFT) => {
            Some(Command::ZoomIn)
        }
        Key::Minus if chart && m.is_none() => Some(Command::ZoomOut),
        Key::Num0 if chart && m.is_none() => Some(Command::Reset),
        _ => None,
    }
}

pub fn commands(ctx: &egui::Context, chart: bool) -> Vec<Command> {
    ctx.input_mut(|input| {
        let mut result = Vec::new();
        input.events.retain(|event| {
            if let Event::Key {
                key,
                pressed: true,
                repeat,
                modifiers,
                ..
            } = event
            {
                if let Some(cmd) = main_key(*key, *modifiers, chart) {
                    // AppKit のメニューで扱うキーを、同じフレームで再実行しない。
                    #[cfg(target_os = "macos")]
                    if crate::commands::native_shortcut(cmd).is_some() {
                        return false;
                    }
                    if !repeat || matches!(cmd, Command::ZoomIn | Command::ZoomOut) {
                        result.push(cmd);
                    }
                    return false;
                }
            }
            true
        });
        result
    })
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ConsoleKey {
    Run,
    Previous,
    Next,
}

fn console_event(events: &mut Vec<Event>, composing: &mut bool) -> Option<ConsoleKey> {
    // 変換確定のフレームで届く Enter を、スクリプトの実行に使わない。
    let mut ime = *composing;
    for event in events.iter() {
        if let Event::Ime(event) = event {
            match event {
                ImeEvent::Preedit(text) => {
                    *composing = !text.is_empty();
                    ime = true;
                }
                ImeEvent::Commit(_) | ImeEvent::Disabled => {
                    *composing = false;
                    ime = true;
                }
                ImeEvent::Enabled => {}
            }
        }
    }
    if ime {
        return None;
    }
    let mut action = None;
    events.retain(|event| {
        let Event::Key {
            key,
            pressed: true,
            repeat,
            modifiers,
            ..
        } = event
        else {
            return true;
        };
        let shortcut = match key {
            Key::Enter if command_only(*modifiers) => ConsoleKey::Run,
            Key::ArrowUp if *modifiers == Modifiers::ALT => ConsoleKey::Previous,
            Key::ArrowDown if *modifiers == Modifiers::ALT => ConsoleKey::Next,
            _ => return true,
        };
        if !repeat {
            action = Some(shortcut);
        }
        false
    });
    action
}

pub fn console(ctx: &egui::Context, composing: &mut bool) -> Option<ConsoleKey> {
    ctx.input_mut(|input| console_event(&mut input.events, composing))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pressed(key: Key, modifiers: Modifiers) -> Event {
        Event::Key {
            key,
            modifiers,
            physical_key: None,
            pressed: true,
            repeat: false,
        }
    }

    #[test]
    fn native_shortcuts_distinguish_land_save_find_and_fullscreen() {
        assert_eq!(main_key(Key::S, Modifiers::NONE, true), Some(Command::Land));
        assert_eq!(
            main_key(Key::S, Modifiers::COMMAND, true),
            Some(Command::SaveState)
        );
        assert_eq!(
            main_key(Key::F, Modifiers::COMMAND, true),
            Some(Command::Objects)
        );
        assert_eq!(
            main_key(
                Key::F,
                Modifiers::MAC_CMD | Modifiers::COMMAND | Modifiers::CTRL,
                true
            ),
            Some(Command::Fullscreen)
        );
        assert_eq!(
            main_key(Key::Plus, Modifiers::SHIFT, true),
            Some(Command::ZoomIn)
        );
        assert_eq!(main_key(Key::Num0, Modifiers::NONE, false), None);
    }

    #[test]
    fn console_keeps_newlines_and_ime_confirmation_and_handles_history() {
        let mut composing = false;
        let enter = pressed(Key::Enter, Modifiers::COMMAND);
        for mut events in [
            vec![Event::Ime(ImeEvent::Preedit("月".into()))],
            vec![enter.clone()],
            vec![Event::Ime(ImeEvent::Commit("月".into())), enter.clone()],
        ] {
            assert_eq!(console_event(&mut events, &mut composing), None);
        }
        assert_eq!(
            console_event(&mut vec![enter], &mut composing),
            Some(ConsoleKey::Run)
        );
        let mut newline = vec![pressed(Key::Enter, Modifiers::NONE)];
        assert_eq!(console_event(&mut newline, &mut composing), None);
        assert_eq!(newline.len(), 1);
        for (key, action) in [
            (Key::ArrowUp, ConsoleKey::Previous),
            (Key::ArrowDown, ConsoleKey::Next),
        ] {
            let mut events = vec![pressed(key, Modifiers::ALT)];
            assert_eq!(console_event(&mut events, &mut composing), Some(action));
            assert!(events.is_empty());
        }
    }
}
