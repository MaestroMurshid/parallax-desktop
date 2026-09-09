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

/// The text a span covers, stored so it can be re-found after a correction.
/// Byte offsets from the wire are clamped to char boundaries -- a transcript is
/// UTF-8 and slicing mid-character would panic.
pub fn quoted(transcript: &str, span: &Span) -> String {
    let start = span.start as usize;
    let end = (span.end as usize).min(transcript.len());
    if start >= end || !transcript.is_char_boundary(start) || !transcript.is_char_boundary(end) {
        return String::new();
    }
    transcript[start..end].to_string()
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
            "INSERT INTO spans (entry_id, start_offset, end_offset, attributed, quoted_text)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                entry.id,
                span.start,
                span.end,
                span.attributed,
                quoted(&entry.transcript, span)
            ],
        )?;
    }

    for item in &entry.action_items {
        conn.execute(
            "INSERT INTO action_items
             (id, entry_id, span_start, span_end, span_attributed, span_quoted, text, done)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                item.id,
                entry.id,
                item.span.start,
                item.span.end,
                item.span.attributed,
                quoted(&entry.transcript, &item.span),
                item.text,
                item.done
            ],
        )?;
    }

    Ok(())
}

const ENTRY_COLUMNS: &str = "id, transcript, created_at, x, y, answers_entry_id,
     answers_question_id, role, register, type_id, resolved, resolution_text, title,
     summary, duration_ms, unfinished, local_only, is_sample";

/// Chronological, because placement solved the field in this order and the
/// positions it produced are frozen (§5.1).
///
/// The predicate is pushed into SQL rather than applied after loading, and the
/// three child tables are scoped by the same predicate, so fetching one entry
/// reads one entry rather than the corpus.
fn load(conn: &Connection, predicate: &str, param: &[&dyn rusqlite::ToSql]) -> Result<Vec<Entry>> {
    let scope = format!("entry_id IN (SELECT id FROM entries WHERE {predicate})");
    let mut audio = audio_by_entry(conn, &scope, param)?;
    let mut spans = spans_by_entry(conn, &scope, param)?;
    let mut actions = actions_by_entry(conn, &scope, param)?;

    let mut stmt = conn.prepare(&format!(
        "SELECT {ENTRY_COLUMNS} FROM entries WHERE {predicate} ORDER BY created_at ASC"
    ))?;
    let rows = stmt.query_map(param, bare_entry)?;

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

pub fn list(conn: &Connection) -> Result<Vec<Entry>> {
    load(conn, "1 = 1", &[])
}

/// Answers to this entry. A manual or proposed connection is an edge and has
/// no parent, so nothing here is about edges despite the wire field's name.
pub fn children_of(conn: &Connection, entry_id: &str) -> Result<Vec<Entry>> {
    load(conn, "answers_entry_id = ?1", &[&entry_id])
}

/// Overwrites the frozen position. Never re-solves the field (§5.1).
pub fn move_to(conn: &Connection, id: &str, x: f64, y: f64) -> Result<()> {
    let n = conn.execute(
        "UPDATE entries SET x = ?2, y = ?3 WHERE id = ?1",
        params![id, x, y],
    )?;
    if n == 0 {
        return Err(crate::error::Error::NotFound(id.to_string()));
    }
    Ok(())
}

/// Audio, spans, action items, questions and edges go with it -- the schema
/// cascades those. Children are orphaned instead, by `ON DELETE SET NULL` on
/// `answers_entry_id`: an answer is still something you said.
pub fn delete(conn: &Connection, id: &str) -> Result<()> {
    conn.execute("DELETE FROM entries WHERE id = ?1", params![id])?;
    Ok(())
}

/// Corrections overwrite. The audio is the record and the transcript is a
/// derivation of it, so there is nothing to version -- the recording is already
/// the thing to check against.
///
/// Offsets shift under an edit, so spans re-anchor by their stored text rather
/// than being trusted. A span whose text is gone is marked stale instead of
/// silently pointing at whatever now occupies those offsets.
pub fn correct_transcript(conn: &Connection, id: &str, transcript: &str) -> Result<()> {
    let n = conn.execute(
        "UPDATE entries SET transcript = ?2, corrected_at = ?3 WHERE id = ?1",
        params![id, transcript, chrono::Utc::now().to_rfc3339()],
    )?;
    if n == 0 {
        return Err(crate::error::Error::NotFound(id.to_string()));
    }
    reanchor_spans(conn, id, transcript)?;
    reanchor_questions(conn, id, transcript)?;
    reanchor_action_items(conn, id, transcript)?;
    Ok(())
}

fn reanchor_spans(conn: &Connection, entry_id: &str, transcript: &str) -> Result<()> {
    let mut stmt = conn.prepare("SELECT id, quoted_text FROM spans WHERE entry_id = ?1")?;
    let rows: Vec<(i64, String)> = stmt
        .query_map(params![entry_id], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;

    for (id, quote) in rows {
        match (quote.is_empty(), transcript.find(&quote)) {
            (false, Some(at)) => {
                conn.execute(
                    "UPDATE spans SET start_offset = ?2, end_offset = ?3, stale = 0 WHERE id = ?1",
                    params![id, at as i64, (at + quote.len()) as i64],
                )?;
            }
            _ => {
                conn.execute("UPDATE spans SET stale = 1 WHERE id = ?1", params![id])?;
            }
        }
    }
    Ok(())
}

/// A question is never dropped by a correction, answered or not: it is the
/// record of something the app actually asked and, often, of something you
/// actually said back. When its anchor cannot be found the offsets go null and
/// the question stays -- it loses a highlight, not its existence.
fn reanchor_questions(conn: &Connection, entry_id: &str, transcript: &str) -> Result<()> {
    let mut stmt = conn.prepare(
        "SELECT id, span_quoted FROM questions WHERE entry_id = ?1 AND span_quoted IS NOT NULL",
    )?;
    let rows: Vec<(String, String)> = stmt
        .query_map(params![entry_id], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;

    for (id, quote) in rows {
        match transcript.find(&quote) {
            Some(at) if !quote.is_empty() => conn.execute(
                "UPDATE questions SET span_start = ?2, span_end = ?3 WHERE id = ?1",
                params![id, at as i64, (at + quote.len()) as i64],
            )?,
            _ => conn.execute(
                "UPDATE questions SET span_start = NULL, span_end = NULL WHERE id = ?1",
                params![id],
            )?,
        };
    }
    Ok(())
}

/// Same rule: the item survives, ticked or not. Only its anchor can go stale.
fn reanchor_action_items(conn: &Connection, entry_id: &str, transcript: &str) -> Result<()> {
    let mut stmt = conn.prepare("SELECT id, span_quoted FROM action_items WHERE entry_id = ?1")?;
    let rows: Vec<(String, String)> = stmt
        .query_map(params![entry_id], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;

    for (id, quote) in rows {
        if !quote.is_empty() {
            if let Some(at) = transcript.find(&quote) {
                conn.execute(
                    "UPDATE action_items SET span_start = ?2, span_end = ?3 WHERE id = ?1",
                    params![id, at as i64, (at + quote.len()) as i64],
                )?;
            }
        }
    }
    Ok(())
}

pub fn get(conn: &Connection, id: &str) -> Result<Option<Entry>> {
    Ok(load(conn, "id = ?1", &[&id])?.pop())
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
        is_sample: if row.get::<_, bool>(17)? {
            Some(true)
        } else {
            None
        },
    })
}

fn audio_by_entry(
    conn: &Connection,
    scope: &str,
    param: &[&dyn rusqlite::ToSql],
) -> Result<HashMap<String, (String, Vec<f32>)>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT entry_id, path, fingerprint FROM audio WHERE {scope}"
    ))?;
    let rows = stmt.query_map(param, |row| {
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

fn spans_by_entry(
    conn: &Connection,
    scope: &str,
    param: &[&dyn rusqlite::ToSql],
) -> Result<HashMap<String, Vec<Span>>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT entry_id, start_offset, end_offset, attributed FROM spans
         WHERE {scope} ORDER BY id"
    ))?;
    let rows = stmt.query_map(param, |row| {
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

fn actions_by_entry(
    conn: &Connection,
    scope: &str,
    param: &[&dyn rusqlite::ToSql],
) -> Result<HashMap<String, Vec<ActionItem>>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT id, entry_id, span_start, span_end, span_attributed, text, done
         FROM action_items WHERE {scope} ORDER BY rowid"
    ))?;
    let rows = stmt.query_map(param, |row| {
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
            audio_path: if audio {
                Some(format!("audio/{id}.wav"))
            } else {
                None
            },
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
            spans: vec![Span {
                start: 0,
                end: 7,
                attributed: true,
            }],
            action_items: vec![ActionItem {
                id: format!("{id}-task-0"),
                entry_id: id.into(),
                span: Span {
                    start: 8,
                    end: 13,
                    attributed: false,
                },
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
        conn.execute("DELETE FROM entries WHERE id = ?1", ["e1"])
            .unwrap();

        for table in ["audio", "spans", "action_items"] {
            let n: i64 = conn
                .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
                .unwrap();
            assert_eq!(n, 0, "{table} kept a row after its entry was deleted");
        }
    }

    /// The whole point of the rule: a correction must never cost you an
    /// exchange that actually happened.
    #[test]
    fn a_correction_keeps_an_answered_question() {
        let conn = open_in_memory().unwrap();
        let mut e = entry("e1", "2024-02-03T10:21:00.000Z", true);
        e.transcript = "Indexes trade write perfrmance for faster reads.".into();
        insert(&conn, &e).unwrap();

        conn.execute(
            "INSERT INTO questions
             (id, entry_id, text, span_start, span_end, span_quoted, answered,
              dismissed, provider_name, created_at)
             VALUES ('q1','e1','What does that cost?',8,13,'trade',1,0,'llama-server','now')",
            [],
        )
        .unwrap();

        correct_transcript(
            &conn,
            "e1",
            "Indexes trade write performance for faster reads.",
        )
        .unwrap();

        let (answered, start): (bool, Option<i64>) = conn
            .query_row(
                "SELECT answered, span_start FROM questions WHERE id = 'q1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert!(answered, "an answered question must survive a correction");
        assert_eq!(start, Some(8), "its anchor should re-find itself");
    }

    /// An anchor that genuinely no longer exists loses its offsets and keeps
    /// everything else, rather than pointing at whatever moved into its place.
    #[test]
    fn a_lost_anchor_goes_stale_rather_than_drifting() {
        let conn = open_in_memory().unwrap();
        let mut e = entry("e1", "2024-02-03T10:21:00.000Z", false);
        e.transcript = "Indexes trade writes for reads.".into();
        e.spans = vec![Span {
            start: 8,
            end: 13,
            attributed: true,
        }];
        insert(&conn, &e).unwrap();

        correct_transcript(&conn, "e1", "Something else entirely.").unwrap();

        let stale: bool = conn
            .query_row("SELECT stale FROM spans WHERE entry_id = 'e1'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert!(stale, "an attributed span must not drift onto other words");

        let kept: i64 = conn
            .query_row(
                "SELECT count(*) FROM spans WHERE entry_id = 'e1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(kept, 1, "stale, not deleted");
    }

    #[test]
    fn a_correction_is_not_a_new_version() {
        let conn = open_in_memory().unwrap();
        insert(&conn, &entry("e1", "2024-02-03T10:21:00.000Z", false)).unwrap();
        correct_transcript(&conn, "e1", "Corrected text.").unwrap();

        let rows: i64 = conn
            .query_row("SELECT count(*) FROM entries", [], |r| r.get(0))
            .unwrap();
        assert_eq!(rows, 1);
        assert_eq!(
            get(&conn, "e1").unwrap().unwrap().transcript,
            "Corrected text."
        );
    }

    /// The three loads share one query path, so scoping has to be proven and
    /// not assumed: `get` must not read the corpus to return one row.
    #[test]
    fn get_returns_one_entry_with_its_own_children() {
        let conn = open_in_memory().unwrap();
        insert(&conn, &entry("e1", "2024-01-01T00:00:00Z", true)).unwrap();
        insert(&conn, &entry("e2", "2024-01-02T00:00:00Z", true)).unwrap();

        let got = get(&conn, "e1").unwrap().unwrap();
        assert_eq!(got.id, "e1");
        assert_eq!(got.spans.len(), 1);
        assert_eq!(got.action_items.len(), 1);
        assert_eq!(got.action_items[0].entry_id, "e1");
        assert!(got.audio_path.is_some());

        assert!(get(&conn, "nobody").unwrap().is_none());
    }

    #[test]
    fn children_are_only_the_answers_to_that_entry() {
        let conn = open_in_memory().unwrap();
        insert(&conn, &entry("parent", "2024-01-01T00:00:00Z", false)).unwrap();
        insert(&conn, &entry("unrelated", "2024-01-02T00:00:00Z", false)).unwrap();

        let mut answer = entry("answer", "2024-01-03T00:00:00Z", false);
        answer.parent_entry_id = Some("parent".into());
        insert(&conn, &answer).unwrap();

        let children = children_of(&conn, "parent").unwrap();
        assert_eq!(children.len(), 1);
        assert_eq!(children[0].id, "answer");
        assert!(children_of(&conn, "unrelated").unwrap().is_empty());
    }
}
