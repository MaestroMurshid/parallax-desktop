use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, WindowEvent};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

/// The canvas. Hidden rather than closed, so the app outlives its window.
const MAIN: &str = "main";
/// The capture panel. Defined in tauri.conf.json, hidden until it has something
/// to show.
const PANEL: &str = "panel";

/// Gap from the corner of the work area, in logical pixels.
const PANEL_MARGIN: f64 = 16.0;

/// §4 — the hotkey starts recording immediately; the panel appears second.
///
/// Capture happens where you are looking. With the canvas focused the recording
/// belongs to it, and the entry opens when it lands. Anywhere else — the whole
/// point of a global shortcut — the panel takes it and the app stays where it
/// was, in the background.
///
/// Only the event goes out from here. The panel window shows itself once
/// recording is actually under way, so the two can never come apart: a webview
/// that is slow to start, or throws before it reaches the recorder, used to
/// leave an empty transparent window on screen swallowing clicks.
fn on_hotkey(app: &AppHandle) {
    let in_canvas = app
        .get_webview_window(MAIN)
        .and_then(|w| w.is_focused().ok())
        .unwrap_or(false);

    let target = if in_canvas { MAIN } else { PANEL };
    let _ = app.emit_to(target, "capture://hotkey", ());
}

/// Bring the canvas forward, from the tray. `show` comes first: a window
/// sitting in the tray cannot take focus until it is on screen again.
fn reveal_main(app: &AppHandle) {
    if let Some(main) = app.get_webview_window(MAIN) {
        let _ = main.show();
        let _ = main.unminimize();
        let _ = main.set_focus();
    }
}

/// Recording has started, so there is now something to show.
///
/// Bottom-right of the work area, clear of the taskbar and out of whatever you
/// are actually reading — you hit the key mid-paragraph and keep reading, which
/// a panel in the middle of the screen makes impossible. Recomputed on every
/// show, because the display you are working on is the one it belongs on.
///
/// Deliberately no set_focus: taking the keyboard out from under whatever you
/// were typing in is the interruption §4 exists to avoid. Start and stop both
/// arrive on the global shortcut, so the panel has no use for focus.
#[tauri::command]
fn show_capture(app: AppHandle) {
    let Some(panel) = app.get_webview_window(PANEL) else {
        return;
    };

    if let (Ok(Some(monitor)), Ok(size)) = (panel.current_monitor(), panel.outer_size()) {
        let area = monitor.work_area();
        let margin = (PANEL_MARGIN * monitor.scale_factor()).round() as i32;
        let x = area.position.x + area.size.width as i32 - size.width as i32 - margin;
        let y = area.position.y + area.size.height as i32 - size.height as i32 - margin;
        let _ = panel.set_position(PhysicalPosition::new(x, y));
    }

    let _ = panel.show();
}

/// Capture is over. The panel leaves without summoning anything: a recording
/// made while you were reading something else should cost you nothing but the
/// keypress, and the entry is already on the canvas for whenever you next open
/// it. Transcription carries on behind this.
#[tauri::command]
fn hide_capture(app: AppHandle) {
    if let Some(panel) = app.get_webview_window(PANEL) {
        let _ = panel.hide();
    }
}

/// The tray is the app's real home: capture is global, so the process has to
/// outlive the canvas window for the hotkey to mean anything (§4).
fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "Open canvas", true, None::<&str>)?;
    let capture = MenuItem::with_id(app, "capture", "Start capture", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &capture, &separator, &quit])?;

    TrayIconBuilder::with_id("tray")
        .icon(app.default_window_icon().cloned().expect("bundled app icon"))
        .tooltip("Parallax — Ctrl+Shift+Space to capture")
        .menu(&menu)
        // Windows convention: left click opens the window, right click opens
        // the menu. The default puts the menu on both.
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "open" => reveal_main(app),
            "capture" => on_hotkey(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                reveal_main(tray.app_handle());
            }
        })
        .build(app)?;

    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let shortcut = Shortcut::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::Space);

    tauri::Builder::default()
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(move |app, triggered, event| {
                    if triggered == &shortcut && event.state() == ShortcutState::Pressed {
                        on_hotkey(app);
                    }
                })
                .build(),
        )
        .invoke_handler(tauri::generate_handler![show_capture, hide_capture])
        .setup(move |app| {
            app.global_shortcut().register(shortcut)?;
            build_tray(app.handle())?;
            Ok(())
        })
        .on_window_event(|window, event| {
            // Closing the canvas sends the app to the tray instead of ending
            // it. A capture tool that only runs while its window is open is not
            // available from anywhere, which is the whole premise.
            if let WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == MAIN {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running application");
}
