//! Registering and swapping the global shortcuts.
//!
//! The record hotkey can change at any time, so a rebind has to reach the OS
//! registration rather than only the stored setting. Discard is armed only
//! while a recording is in flight: registered in `start_recording`, torn down
//! on every path off "recording", so it never takes a key from other apps.

use crate::state::AppState;
use std::str::FromStr;
use tauri::AppHandle;
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut};

/// What ships before anyone has changed a setting, and the fallback if a
/// stored chord ever fails to parse -- a corrupt setting must cost the
/// hotkey, never the app's ability to start.
pub fn default_hotkey() -> Shortcut {
    Shortcut::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::Space)
}

/// Parses a chord the way the settings UI writes it ("Ctrl+Shift+Space",
/// "Ctrl+Alt+K"). `None` on anything the underlying parser rejects, so a bad
/// string costs nothing that already worked rather than panicking or
/// registering garbage.
pub fn parse(chord: &str) -> Option<Shortcut> {
    Shortcut::from_str(chord).ok()
}

/// The shortcut to register for `chord`, or `None` when it is not a change --
/// either it fails to parse, or it already matches what is registered. Kept
/// pure and OS-free so the decision is testable without a global shortcut
/// manager, which needs a running app to exist at all.
fn next(current: Shortcut, chord: &str) -> Option<Shortcut> {
    let candidate = parse(chord)?;
    (candidate != current).then_some(candidate)
}

/// Called from `set_settings` whenever the hotkey field actually changed.
/// Registers the replacement before unregistering the old one, so a combo
/// already taken by another app leaves the previous hotkey working rather
/// than a gap with nothing registered at all.
pub fn rebind_hotkey(app: &AppHandle, state: &AppState, chord: &str) {
    let mut slot = state.hotkey.lock().unwrap_or_else(|p| p.into_inner());
    let Some(replacement) = next(*slot, chord) else {
        return;
    };
    if let Err(e) = app.global_shortcut().register(replacement) {
        eprintln!("could not register hotkey '{chord}': {e}");
        return;
    }
    let _ = app.global_shortcut().unregister(*slot);
    *slot = replacement;
}

/// Arms the discard shortcut for the lifetime of one recording. A parse or
/// registration failure is logged and leaves discard mouse-only for this take
/// (the panel's Discard button still works) -- a taken key must not cost the
/// recording that is already under way.
pub fn arm_discard(app: &AppHandle, state: &AppState, chord: &str) {
    let Some(shortcut) = parse(chord) else {
        eprintln!("discard hotkey '{chord}' did not parse, discard stays mouse-only this take");
        return;
    };
    if let Err(e) = app.global_shortcut().register(shortcut) {
        eprintln!("could not register discard hotkey '{chord}': {e}");
        return;
    }
    *state
        .discard_shortcut
        .lock()
        .unwrap_or_else(|p| p.into_inner()) = Some(shortcut);
}

/// Disarms whatever discard shortcut is currently registered, if any. Called
/// on every path off "recording" -- stop and discard alike -- so the key is
/// never armed a moment longer than a recording is actually in flight.
pub fn disarm_discard(app: &AppHandle, state: &AppState) {
    let taken = state
        .discard_shortcut
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .take();
    if let Some(shortcut) = taken {
        let _ = app.global_shortcut().unregister(shortcut);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_different_chord_is_registered() {
        let current = default_hotkey();
        assert_eq!(
            next(current, "Ctrl+Alt+K"),
            Some(Shortcut::new(
                Some(Modifiers::CONTROL | Modifiers::ALT),
                Code::KeyK
            ))
        );
    }

    #[test]
    fn the_same_chord_is_not_treated_as_a_change() {
        let current = default_hotkey();
        assert_eq!(next(current, "Ctrl+Shift+Space"), None);
    }

    /// A bad string must cost nothing that already worked -- keeping the
    /// current combo beats silently registering nothing at all.
    #[test]
    fn an_unparseable_chord_keeps_the_current_one() {
        let current = default_hotkey();
        assert_eq!(next(current, "not a real chord"), None);
    }

    /// The settings UI's own format (SettingsPanel.tsx, onboarding).
    #[test]
    fn the_settings_ui_format_parses() {
        assert_eq!(
            parse("Ctrl+Shift+Space"),
            Some(Shortcut::new(
                Some(Modifiers::CONTROL | Modifiers::SHIFT),
                Code::Space
            ))
        );
        assert_eq!(
            parse("Ctrl+Alt+K"),
            Some(Shortcut::new(
                Some(Modifiers::CONTROL | Modifiers::ALT),
                Code::KeyK
            ))
        );
        assert_eq!(parse("Escape"), Some(Shortcut::new(None, Code::Escape)));
    }
}
