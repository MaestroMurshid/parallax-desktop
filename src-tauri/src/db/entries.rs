//! Reading and writing entries. The wire `Entry` is flat; storage is not, so
//! every read reassembles one from four tables.

use crate::error::Result;
use crate::model::{ActionItem, Entry, Register, Role, Span};
use rusqlite::{params, Connection, Row};
use std::collections::HashMap;

/// Enums cross the SQL boundary as the same strings they use on the wire, so a
/// database dump stays readable and matches what the frontend sees.
fn role_from(s: &str) -> Role {
    match s {
        "evidence" => Role::Evidence,
        "note" => Role::Note,
        _ => Role::Position,
    }
}

fn role_str(r: Role) -> &'static str {
    match r {
        Role::Position => "position",
        Role::Evidence => "evidence",
        Role::Note => "note",
    }
}

/// Unknown values resolve to `Live`, the safe direction: a false `live` costs a
/// missed question, a false `neutral` costs the thing that cannot be undone.
fn register_from(s: &str) -> Register {
    match s {
        "neutral" => Register::Neutral,
        _ => Register::Live,
    }
}

fn register_str(r: Register) -> &'static str {
    match r {
        Register::Live => "live",
        Register::Neutral => "neutral",
    }
}

pub fn insert(conn: &Connection, entry: &Entry) -> Result<()> {
    conn.execute(
        "INSERT INTO entries (
            id, transcript, created_at, x, y, answers_entry_id, answers_question_id,
            role, register, type_id, resolved, resolution_text, title, summary,
            duration_ms, unfinished, local_only, is_sample
         ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18)",
        params![
            entry.id,
            entry.transcript,
            entry.created_at,
            entry.x,
            entry.y,
            entry.parent_entry_id,
            entry.answers_question_id,
            role_str(entry.role),
            register_str(entry.register),
            entry.type_id,
            entry.resolved,
            entry.resolution_text,
            entry.title,
            entry.summary,
            entry.duration_ms,
            entry.unfinished,
            entry.local_only,
            entry.is_sample.unwrap_or(false),
        ],
    )?;

    // No row at all for a typed entry, rather than a null path and an empty
    // fingerprint. Codec is fixed while PCM is the only writer.
    if let Some(path) = &entry.audio_path {
        conn.execute(
            "INSERT INTO audio (entry_id, path, codec, sample_rate, byte_size, fingerprint)
             VALUES (?1, ?2, 'wav', 16000, 0, ?3)",
            params![entry.id, path, serde_json::to_string(&entry.fingerprint)?],
        )?;
    }

    for span in &entry.spans {
        conn.execute(
            "INSERT INTO spans (entry_id, start_offset, end_offset, attributed)
             VALUES (?1, ?2, ?3, ?4)",
            params![entry.id, span.start, span.end, span.attributed],
        )?;
    }

    for item in &entry.action_items {
        conn.execute(
            "INSERT INTO action_items
             (id, entry_id, span_start, span_end, span_attributed, text, done)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                item.id,
                entry.id,
                item.span.start,
                item.span.end,
                item.span.attributed,
                item.text,
                item.done
            ],
        )?;
    }

    Ok(())
}

/// Chronological, because placement solved the field in this order and the
/// positions it produced are frozen (§5.1).
///
/// Four queries rather than one per entry: the children are fetched whole and
/// bucketed in memory, so the cost does not grow with corpus size.
pub fn list(conn: &Connection) -> Result<Vec<Entry>> {
    let mut audio = audio_by_entry(conn)?;
    let mut spans = spans_by_entry(conn)?;
    let mut actions = actions_by_entry(conn)?;

    let mut stmt = conn.prepare(
        "SELECT id, transcript, created_at, x, y, answers_entry_id, answers_question_id,
                role, register, type_id, resolved, resolution_text, title, summary,
                duration_ms, unfinished, local_only, is_sample
         FROM entries ORDER BY created_at ASC",
    )?;

    let rows = stmt.query_map([], bare_entry)?;

    let mut out = Vec::new();
    for row in rows {
        let mut entry = row?;
        if let Some((path, fingerprint)) = audio.remove(&entry.id) {
            entry.audio_path = Some(path);
            entry.fingerprint = fingerprint;
        }
        entry.spans = spans.remove(&entry.id).unwrap_or_default();
        entry.action_items = actions.remove(&entry.id).unwrap_or_default();
        out.push(entry);
    }
    Ok(out)
}

pub fn get(conn: &Connection, id: &str) -> Result<Option<Entry>> {
    Ok(list(conn)?.into_iter().find(|e| e.id == id))
}

/// Everything the entries table itself holds. Audio, spans and action items
/// are attached by the caller.
fn bare_entry(row: &Row) -> rusqlite::Result<Entry> {
    let role: String = row.get(7)?;
    let register: String = row.get(8)?;
    Ok(Entry {
        id: row.get(0)?,
        audio_path: None,
        transcript: row.get(1)?,
        created_at: row.get(2)?,
        x: row.get(3)?,
        y: row.get(4)?,
        parent_entry_id: row.get(5)?,
        answers_question_id: row.get(6)?,
        role: role_from(&role),
        register: register_from(&register),
        type_id: row.get(9)?,
        resolved: row.get(10)?,
        resolution_text: row.get(11)?,
        title: row.get(12)?,
        summary: row.get(13)?,
        duration_ms: row.get(14)?,
        fingerprint: Vec::new(),
        unfinished: row.get(15)?,
        local_only: row.get(16)?,
        spans: Vec::new(),
        action_items: Vec::new(),
        is_sample: if row.get::<_, bool>(17)? { Some(true) } else { None },
    })
}

fn audio_by_entry(conn: &Connection) -> Result<HashMap<String, (String, Vec<f32>)>> {
    let mut stmt = conn.prepare("SELECT entry_id, path, fingerprint FROM audio")?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
        ))
    })?;

    let mut out = HashMap::new();
    for row in rows {
        let (id, path, raw) = row?;
        out.insert(id, (path, serde_json::from_str(&raw)?));
    }
    Ok(out)
}

fn spans_by_entry(conn: &Connection) -> Result<HashMap<String, Vec<Span>>> {
    let mut stmt = conn.prepare(
        "SELECT entry_id, start_offset, end_offset, attributed FROM spans ORDER BY id",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            Span {
                start: row.get(1)?,
                end: row.get(2)?,
                attributed: row.get(3)?,
            },
        ))
    })?;

    let mut out: HashMap<String, Vec<Span>> = HashMap::new();
    for row in rows {
        let (id, span) = row?;
        out.entry(id).or_default().push(span);
    }
    Ok(out)
}

fn actions_by_entry(conn: &Connection) -> Result<HashMap<String, Vec<ActionItem>>> {
    let mut stmt = conn.prepare(
        "SELECT id, entry_id, span_start, span_end, span_attributed, text, done
         FROM action_items ORDER BY rowid",
    )?;
    let rows = stmt.query_map([], |row| {
        let entry_id: String = row.get(1)?;
        Ok((
            entry_id.clone(),
            ActionItem {
                id: row.get(0)?,
                entry_id,
                span: Span {
                    start: row.get(2)?,
                    end: row.get(3)?,
                    attributed: row.get(4)?,
                },
                text: row.get(5)?,
                done: row.get(6)?,
            },
        ))
    })?;

    let mut out: HashMap<String, Vec<ActionItem>> = HashMap::new();
    for row in rows {
        let (id, item) = row?;
        out.entry(id).or_default().push(item);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_in_memory;

    fn entry(id: &str, created_at: &str, audio: bool) -> Entry {
        Entry {
            id: id.into(),
            audio_path: if audio { Some(format!("audio/{id}.wav")) } else { None },
            transcript: "Indexes trade write performance for faster reads.".into(),
            created_at: created_at.into(),
            x: 10.5,
            y: -22.25,
            parent_entry_id: None,
            answers_question_id: None,
            role: Role::Evidence,
            register: Register::Neutral,
            type_id: "evidence".into(),
            resolved: false,
            resolution_text: None,
            title: "indexes trade writes".into(),
            summary: Some("An index buys read speed with write cost.".into()),
            duration_ms: 19_000,
            fingerprint: if audio { vec![0.2, 0.9, 0.44] } else { vec![] },
            unfinished: false,
            local_only: false,
            spans: vec![Span { start: 0, end: 7, attributed: true }],
            action_items: vec![ActionItem {
                id: format!("{id}-task-0"),
                entry_id: id.into(),
                span: Span { start: 8, end: 13, attributed: false },
                text: "Check the write path".into(),
                done: false,
            }],
            is_sample: Some(true),
        }
    }

    #[test]
    fn an_entry_survives_the_round_trip() {
        let conn = open_in_memory().unwrap();
        let before = entry("e1", "2024-02-03T10:21:00.000Z", true);
        insert(&conn, &before).unwrap();

        let after = list(&conn).unwrap();
        assert_eq!(after.len(), 1);
        assert_eq!(
            serde_json::to_value(&before).unwrap(),
            serde_json::to_value(&after[0]).unwrap()
        );
    }

    /// A typed entry has no audio row at all, so both halves come back empty
    /// without anything having to remember to null them.
    #[test]
    fn a_typed_entry_has_no_audio() {
        let conn = open_in_memory().unwrap();
        insert(&conn, &entry("typed", "2024-02-27T11:09:00.000Z", false)).unwrap();

        let rows: i64 = conn
            .query_row("SELECT count(*) FROM audio", [], |r| r.get(0))
            .unwrap();
        assert_eq!(rows, 0);

        let after = &list(&conn).unwrap()[0];
        assert!(after.audio_path.is_none());
        assert!(after.fingerprint.is_empty());
    }

    /// Placement solved the field in this order and froze the result, so the
    /// order entries come back in is part of the contract, not a convenience.
    #[test]
    fn entries_come_back_chronologically() {
        let conn = open_in_memory().unwrap();
        insert(&conn, &entry("later", "2025-12-08T16:56:00.000Z", false)).unwrap();
        insert(&conn, &entry("earlier", "2024-01-14T09:38:00.000Z", false)).unwrap();

        let ids: Vec<String> = list(&conn).unwrap().into_iter().map(|e| e.id).collect();
        assert_eq!(ids, vec!["earlier", "later"]);
    }

    /// Cascades exist so a delete cannot leave spans or action items behind.
    #[test]
    fn deleting_an_entry_takes_its_children() {
        let conn = open_in_memory().unwrap();
        insert(&conn, &entry("e1", "2024-02-03T10:21:00.000Z", true)).unwrap();
        conn.execute("DELETE FROM entries WHERE id = ?1", ["e1"]).unwrap();

        for table in ["audio", "spans", "action_items"] {
            let n: i64 = conn
                .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
                .unwrap();
            assert_eq!(n, 0, "{table} kept a row after its entry was deleted");
        }
    }
}
