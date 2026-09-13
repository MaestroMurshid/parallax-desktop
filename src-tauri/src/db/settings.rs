//! Settings, stored as one JSON document under a single key.
//!
//! A column per field would mean a migration every time one is added, and
//! nothing ever queries settings by value -- they are read whole at startup and
//! written whole on change.

use crate::error::Result;
use crate::model::Settings;
use rusqlite::{params, Connection, OptionalExtension};

const KEY: &str = "settings";

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
    Ok(serde_json::from_value(doc).unwrap_or_default())
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
    let mut doc = serde_json::to_value(get(conn)?)?;
    if let (Some(target), Some(patch)) = (doc.as_object_mut(), patch.as_object()) {
        for (key, value) in patch {
            target.insert(key.clone(), value.clone());
        }
    }
    let merged: Settings = serde_json::from_value(doc)?;
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
        assert_eq!(merged.discard_hotkey, "Escape");
        assert_eq!(get(&conn).unwrap().hotkey, "Ctrl+Alt+K");
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
        assert_eq!(s.discard_hotkey, "Escape", "the absent one defaults");
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
