//! Settings, stored as one JSON document under a single key.
//!
//! A column per field would mean a migration every time one is added, and
//! nothing ever queries settings by value -- they are read whole at startup and
//! written whole on change.

use crate::error::{Error, Result};
use crate::model::Settings;
use rusqlite::{params, Connection, OptionalExtension};

const KEY: &str = "settings";

/// A custom model path is checked on the way in, not resolved later and
/// silently ignored -- a typo should tell the person who made it rather than
/// read as the model quietly falling back to the catalogue. Only fields the
/// incoming patch actually touches are checked, so a file that goes missing
/// after being saved cannot make an unrelated later patch fail to merge.
fn validate_custom_model_patch(patch: &serde_json::Value) -> Result<()> {
    validate_model_path_field(
        patch,
        "customReasoningModelPath",
        "your own reasoning model",
    )?;
    validate_model_path_field(
        patch,
        "customTranscriptionModelPath",
        "your own transcription model",
    )
}

fn validate_model_path_field(patch: &serde_json::Value, field: &str, label: &str) -> Result<()> {
    let Some(value) = patch.get(field) else {
        return Ok(());
    };
    let path = match value {
        // Clearing it is always allowed.
        serde_json::Value::Null => return Ok(()),
        serde_json::Value::String(s) if s.trim().is_empty() => return Ok(()),
        serde_json::Value::String(s) => s.trim(),
        // Not a string at all -- the ordinary deserialize error reports that.
        _ => return Ok(()),
    };
    let p = std::path::Path::new(path);
    let is_gguf = p
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("gguf"));
    if is_gguf && p.is_file() {
        return Ok(());
    }
    Err(Error::Other(format!(
        "{label} must be an existing .gguf file"
    )))
}

/// An empty string is how the UI asks to clear one of these fields, and
/// `Option<String>` needs to hear that as null -- otherwise it stores
/// `Some("")`, a path nothing resolves and nothing falls back from.
fn normalize_empty_custom_paths(mut patch: serde_json::Value) -> serde_json::Value {
    if let Some(obj) = patch.as_object_mut() {
        for field in ["customReasoningModelPath", "customTranscriptionModelPath"] {
            let is_blank =
                matches!(obj.get(field), Some(serde_json::Value::String(s)) if s.trim().is_empty());
            if is_blank {
                obj.insert(field.to_string(), serde_json::Value::Null);
            }
        }
    }
    patch
}

/// Defaults when nothing is stored, and defaults for anything a stored document
/// predates -- so adding a field cannot leave an old install unreadable.
pub fn get(conn: &Connection) -> Result<Settings> {
    let raw: Option<String> = conn
        .query_row(
            "SELECT value FROM settings WHERE key = ?1",
            params![KEY],
            |row| row.get(0),
        )
        .optional()?;

    let Some(json) = raw else {
        return Ok(Settings::default());
    };

    // Field by field over the defaults rather than all-or-nothing. Discarding
    // the whole document on one bad field would be persisted by the next merge,
    // so a single unreadable value would quietly cost every real setting.
    let stored: serde_json::Value = serde_json::from_str(&json).unwrap_or(serde_json::Value::Null);
    let mut doc = serde_json::to_value(Settings::default())?;

    if let Some(stored) = stored.as_object() {
        for (key, value) in stored {
            let mut candidate = doc.clone();
            match candidate.as_object_mut() {
                Some(obj) => obj.insert(key.clone(), value.clone()),
                None => continue,
            };
            if serde_json::from_value::<Settings>(candidate.clone()).is_ok() {
                doc = candidate;
            }
        }
    }
    Ok(chorded_discard(
        serde_json::from_value(doc).unwrap_or_default(),
    ))
}

/// Discard is registered globally for the length of a take, and a key with no
/// modifier would be swallowed from every other app meanwhile -- Esc in a
/// browser would throw the recording away.
fn chorded_discard(mut settings: Settings) -> Settings {
    let chorded =
        crate::shortcuts::parse(&settings.discard_hotkey).is_some_and(|s| !s.mods.is_empty());
    if !chorded {
        settings.discard_hotkey = Settings::default().discard_hotkey;
    }
    settings
}

pub fn set(conn: &Connection, settings: &Settings) -> Result<()> {
    conn.execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![KEY, serde_json::to_string(settings)?],
    )?;
    Ok(())
}

/// The bridge sends a partial, so the merge happens over JSON rather than
/// field by field -- otherwise every new setting needs a line here too.
pub fn merge(conn: &Connection, patch: serde_json::Value) -> Result<Settings> {
    validate_custom_model_patch(&patch)?;
    let patch = normalize_empty_custom_paths(patch);
    let mut doc = serde_json::to_value(get(conn)?)?;
    if let (Some(target), Some(patch)) = (doc.as_object_mut(), patch.as_object()) {
        for (key, value) in patch {
            target.insert(key.clone(), value.clone());
        }
    }
    let merged = chorded_discard(serde_json::from_value(doc)?);
    set(conn, &merged)?;
    Ok(merged)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_in_memory;
    use crate::model::{ComputeBackend, Residency};

    #[test]
    fn defaults_when_nothing_is_stored() {
        let conn = open_in_memory().unwrap();
        let s = get(&conn).unwrap();
        assert_eq!(s.hotkey, "Ctrl+Shift+Space");
        assert_eq!(s.residency, Residency::Warm);
        assert_eq!(s.transcription_backend, ComputeBackend::Auto);
    }

    #[test]
    fn a_patch_leaves_everything_else_alone() {
        let conn = open_in_memory().unwrap();
        let merged = merge(&conn, serde_json::json!({ "hotkey": "Ctrl+Alt+K" })).unwrap();

        assert_eq!(merged.hotkey, "Ctrl+Alt+K");
        assert_eq!(merged.discard_hotkey, Settings::default().discard_hotkey);
        assert_eq!(get(&conn).unwrap().hotkey, "Ctrl+Alt+K");
    }

    /// Discard is registered globally while recording, and a bare key there is
    /// taken from every app: Esc in a browser would throw the take away.
    #[test]
    fn a_stored_bare_discard_key_falls_back_to_a_chord() {
        let conn = open_in_memory().unwrap();
        conn.execute(
            "INSERT INTO settings (key, value) VALUES ('settings', '{\"discardHotkey\":\"Escape\"}')",
            [],
        )
        .unwrap();

        let s = get(&conn).unwrap();
        assert_ne!(s.discard_hotkey, "Escape");
        let chord = crate::shortcuts::parse(&s.discard_hotkey).expect("default parses");
        assert!(!chord.mods.is_empty(), "the fallback carries a modifier");
    }

    #[test]
    fn merging_a_bare_discard_key_keeps_a_chord() {
        let conn = open_in_memory().unwrap();
        let merged = merge(&conn, serde_json::json!({ "discardHotkey": "Delete" })).unwrap();

        assert_eq!(merged.discard_hotkey, Settings::default().discard_hotkey);
        assert_eq!(
            get(&conn).unwrap().discard_hotkey,
            Settings::default().discard_hotkey
        );
    }

    #[test]
    fn a_chorded_discard_key_is_kept() {
        let conn = open_in_memory().unwrap();
        let merged = merge(&conn, serde_json::json!({ "discardHotkey": "Ctrl+Alt+D" })).unwrap();
        assert_eq!(merged.discard_hotkey, "Ctrl+Alt+D");
    }

    /// An install that predates a setting must still open -- and keep the
    /// settings it does have. Falling back to factory defaults wholesale would
    /// be written back by the next merge, losing the real configuration.
    #[test]
    fn a_stored_document_missing_a_field_keeps_what_it_has() {
        let conn = open_in_memory().unwrap();
        conn.execute(
            "INSERT INTO settings (key, value) VALUES ('settings', '{\"hotkey\":\"Ctrl+J\"}')",
            [],
        )
        .unwrap();

        let s = get(&conn).unwrap();
        assert_eq!(s.hotkey, "Ctrl+J", "the stored field survives");
        assert_eq!(
            s.discard_hotkey,
            Settings::default().discard_hotkey,
            "the absent one defaults"
        );
    }

    /// `theme` postdates this fixture's shape, same as any other field an old
    /// install's stored document lacks.
    #[test]
    fn a_stored_document_without_theme_defaults_to_system() {
        use crate::model::Theme;

        let conn = open_in_memory().unwrap();
        conn.execute(
            "INSERT INTO settings (key, value) VALUES ('settings', '{\"hotkey\":\"Ctrl+J\"}')",
            [],
        )
        .unwrap();

        assert_eq!(get(&conn).unwrap().theme, Theme::System);
    }

    /// An install that predates the feature entirely -- both fields absent,
    /// not merely null -- must still load rather than fail to deserialize.
    #[test]
    fn a_stored_document_without_custom_model_fields_defaults_to_none() {
        let conn = open_in_memory().unwrap();
        conn.execute(
            "INSERT INTO settings (key, value) VALUES ('settings', '{\"hotkey\":\"Ctrl+J\"}')",
            [],
        )
        .unwrap();

        let s = get(&conn).unwrap();
        assert_eq!(s.custom_reasoning_model_path, None);
        assert_eq!(s.custom_transcription_model_path, None);
    }

    fn temp_gguf(name: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!(
            "parallax-settings-test-{}-{name}",
            uuid::Uuid::new_v4()
        ));
        std::fs::write(&path, b"stand-in, never actually loaded").unwrap();
        path
    }

    #[test]
    fn a_custom_reasoning_path_to_a_real_gguf_is_accepted() {
        let conn = open_in_memory().unwrap();
        let file = temp_gguf("mine.gguf");
        let merged = merge(
            &conn,
            serde_json::json!({ "customReasoningModelPath": file.to_str().unwrap() }),
        )
        .unwrap();
        assert_eq!(merged.custom_reasoning_model_path.as_deref(), file.to_str());
        let _ = std::fs::remove_file(&file);
    }

    #[test]
    fn a_custom_path_to_a_file_that_does_not_exist_is_rejected() {
        let conn = open_in_memory().unwrap();
        let ghost =
            std::env::temp_dir().join(format!("parallax-ghost-{}.gguf", uuid::Uuid::new_v4()));
        let err = merge(
            &conn,
            serde_json::json!({ "customReasoningModelPath": ghost.to_str().unwrap() }),
        )
        .unwrap_err();
        assert!(err.to_string().contains("your own reasoning model"));
        // Rejected, so nothing was written.
        assert_eq!(get(&conn).unwrap().custom_reasoning_model_path, None);
    }

    #[test]
    fn a_custom_path_with_the_wrong_extension_is_rejected() {
        let conn = open_in_memory().unwrap();
        let path =
            std::env::temp_dir().join(format!("parallax-not-gguf-{}.bin", uuid::Uuid::new_v4()));
        std::fs::write(&path, b"wrong extension").unwrap();
        let err = merge(
            &conn,
            serde_json::json!({ "customTranscriptionModelPath": path.to_str().unwrap() }),
        )
        .unwrap_err();
        assert!(err.to_string().contains("your own transcription model"));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn an_empty_string_clears_a_custom_path_without_validation() {
        let conn = open_in_memory().unwrap();
        let file = temp_gguf("to-clear.gguf");
        merge(
            &conn,
            serde_json::json!({ "customReasoningModelPath": file.to_str().unwrap() }),
        )
        .unwrap();

        let cleared = merge(&conn, serde_json::json!({ "customReasoningModelPath": "" })).unwrap();
        assert_eq!(cleared.custom_reasoning_model_path, None);
        let _ = std::fs::remove_file(&file);
    }

    #[test]
    fn a_null_clears_a_custom_path_without_validation() {
        let conn = open_in_memory().unwrap();
        let file = temp_gguf("to-null.gguf");
        merge(
            &conn,
            serde_json::json!({ "customTranscriptionModelPath": file.to_str().unwrap() }),
        )
        .unwrap();

        let cleared = merge(
            &conn,
            serde_json::json!({ "customTranscriptionModelPath": null }),
        )
        .unwrap();
        assert_eq!(cleared.custom_transcription_model_path, None);
        let _ = std::fs::remove_file(&file);
    }

    /// A patch that never mentions the field must not be broken by a value
    /// already stored from before this validation existed -- only what the
    /// incoming patch actually touches is checked.
    #[test]
    fn an_untouched_field_is_not_revalidated_by_an_unrelated_patch() {
        let conn = open_in_memory().unwrap();
        conn.execute(
            "INSERT INTO settings (key, value) VALUES ('settings',
             '{\"customReasoningModelPath\":\"E:/gone/nowhere.gguf\"}')",
            [],
        )
        .unwrap();

        let merged = merge(&conn, serde_json::json!({ "hotkey": "Ctrl+Alt+M" })).unwrap();
        assert_eq!(merged.hotkey, "Ctrl+Alt+M");
    }

    /// A stored document with a field that no longer parses must keep every
    /// field that still does. Resetting to factory defaults would then be
    /// persisted by the next merge, losing the real configuration for good.
    #[test]
    fn an_unparseable_field_does_not_discard_the_rest() {
        let conn = open_in_memory().unwrap();
        conn.execute(
            "INSERT INTO settings (key, value) VALUES ('settings',
             '{\"hotkey\":\"Ctrl+J\",\"residency\":\"lukewarm\"}')",
            [],
        )
        .unwrap();

        let s = get(&conn).unwrap();
        assert_eq!(s.hotkey, "Ctrl+J", "a good field survives a bad neighbour");
        assert_eq!(s.residency, Residency::Warm, "the bad one falls back");
    }
}
