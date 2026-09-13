//! Reading and writing entries. The wire `Entry` is flat; storage is not, so
//! every read reassembles one from four tables.

use crate::error::Result;
use crate::model::{ActionItem, Entry, Register, Role, Span};
use rusqlite::{params, Connection, OptionalExtension, Row};
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
///
/// The only place a span offset becomes a byte offset. `utf16_to_byte` clamps
/// to a char boundary, so half a surrogate pair -- a legal UTF-16 index --
/// widens to the character it sits in rather than losing the quote.
pub fn quoted(transcript: &str, span: &Span) -> String {
    let start = crate::text::utf16_to_byte(transcript, span.start);
    let end = crate::text::utf16_to_byte(transcript, span.end);
    if start >= end {
        return String::new();
    }
    transcript[start..end].to_string()
}

/// Four tables make one entry, so they commit together. The sample loader
/// already holds a transaction and SQLite will not nest one.
pub fn insert(conn: &Connection, entry: &Entry) -> Result<()> {
    if !conn.is_autocommit() {
        return insert_rows(conn, entry);
    }
    let tx = conn.unchecked_transaction()?;
    insert_rows(&tx, entry)?;
    tx.commit()?;
    Ok(())
}

fn insert_rows(conn: &Connection, entry: &Entry) -> Result<()> {
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

/// These ids in the order the notes were recorded; ids no longer present are
/// dropped.
pub fn oldest_first(conn: &Connection, ids: &[String]) -> Result<Vec<String>> {
    let wanted: std::collections::HashSet<&str> = ids.iter().map(String::as_str).collect();
    // The same order `load` reads the field in, so a bulk pass and placement
    // can never disagree about which note came first.
    let mut stmt = conn.prepare("SELECT id FROM entries ORDER BY created_at ASC")?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
    let mut out = Vec::new();
    for id in rows {
        let id = id?;
        if wanted.contains(id.as_str()) {
            out.push(id);
        }
    }
    Ok(out)
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

/// §6.3 -- resolution is declared, never inferred, and the text is the point.
/// A bare flag records that you stopped rather than what you concluded, which
/// is the only part worth keeping.
pub fn resolve(conn: &Connection, id: &str, text: &str) -> Result<()> {
    let text = text.trim();
    if text.is_empty() {
        return Err(crate::error::Error::Other(
            "a resolution has to say what was resolved".to_string(),
        ));
    }
    let n = conn.execute(
        "UPDATE entries SET resolved = 1, resolution_text = ?2 WHERE id = ?1",
        params![id, text],
    )?;
    if n == 0 {
        return Err(crate::error::Error::NotFound(id.to_string()));
    }
    Ok(())
}

/// The text stays. Reopening says you are not done, not that you never reached
/// the conclusion -- and the thread layer renders that sentence (§6.2).
pub fn reopen(conn: &Connection, id: &str) -> Result<()> {
    let n = conn.execute("UPDATE entries SET resolved = 0 WHERE id = ?1", params![id])?;
    if n == 0 {
        return Err(crate::error::Error::NotFound(id.to_string()));
    }
    Ok(())
}

/// Audio, spans, action items, questions and edges go with it -- the schema
/// cascades those. Children are orphaned instead, by `ON DELETE SET NULL` on
/// `answers_entry_id`: an answer is still something you said.
/// Read before deleting: the cascade takes the audio row, not the file.
pub fn audio_path(conn: &Connection, id: &str) -> Result<Option<String>> {
    Ok(conn
        .query_row(
            "SELECT path FROM audio WHERE entry_id = ?1",
            params![id],
            |r| r.get(0),
        )
        .optional()?)
}

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
    // Blanking a note is deletion wearing a correction's clothes: it empties the
    // verbatim record and stales every anchor on it, with no audit trail and no
    // undo. Delete is the verb that admits to doing that.
    if transcript.trim().is_empty() {
        return Err(crate::error::Error::Other(
            "a correction cannot empty the note".to_string(),
        ));
    }
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

/// Where a quote went, given where it used to be. Returns UTF-16 offsets.
///
/// A quote can occur more than once, and taking the first hit collapses every
/// row that shares that text onto the same place -- which would move an
/// attributed span onto the user's own words and let a probe push on someone
/// else's sentence (§3.3). The occurrence nearest the old offset is the one
/// that belongs to this row.
///
/// The conversion is the point of returning a pair rather than a start: every
/// offset column counts UTF-16, `match_indices` and `len` count bytes, and a
/// single em dash above the quote was enough to re-anchor every row below it
/// two units past its own words. `was_at` arrives from the database in UTF-16,
/// so the candidates are converted before the comparison and not after -- the
/// two units disagree about distance, which is what picks the occurrence.
fn nearest_occurrence(transcript: &str, quote: &str, was_at: i64) -> Option<(i64, i64)> {
    if quote.is_empty() {
        return None;
    }
    let width = quote.encode_utf16().count() as i64;
    transcript
        .match_indices(quote)
        .map(|(at, _)| crate::text::byte_to_utf16(transcript, at) as i64)
        .min_by_key(|at| (*at - was_at).abs())
        .map(|at| (at, at + width))
}

fn reanchor_spans(conn: &Connection, entry_id: &str, transcript: &str) -> Result<()> {
    let mut stmt =
        conn.prepare("SELECT id, quoted_text, start_offset FROM spans WHERE entry_id = ?1")?;
    let rows: Vec<(i64, String, i64)> = stmt
        .query_map(params![entry_id], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })?
        .collect::<rusqlite::Result<_>>()?;

    for (id, quote, was_at) in rows {
        match nearest_occurrence(transcript, &quote, was_at) {
            Some((at, end)) => {
                conn.execute(
                    "UPDATE spans SET start_offset = ?2, end_offset = ?3, stale = 0 WHERE id = ?1",
                    params![id, at, end],
                )?;
            }
            None => {
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
        "SELECT id, span_quoted, COALESCE(span_start, 0) FROM questions
         WHERE entry_id = ?1 AND span_quoted IS NOT NULL",
    )?;
    let rows: Vec<(String, String, i64)> = stmt
        .query_map(params![entry_id], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })?
        .collect::<rusqlite::Result<_>>()?;

    for (id, quote, was_at) in rows {
        match nearest_occurrence(transcript, &quote, was_at) {
            Some((at, end)) => conn.execute(
                "UPDATE questions SET span_start = ?2, span_end = ?3 WHERE id = ?1",
                params![id, at, end],
            )?,
            None => conn.execute(
                "UPDATE questions SET span_start = NULL, span_end = NULL WHERE id = ?1",
                params![id],
            )?,
        };
    }
    Ok(())
}

/// Same rule: the item survives, ticked or not. Only its anchor can go stale.
fn reanchor_action_items(conn: &Connection, entry_id: &str, transcript: &str) -> Result<()> {
    let mut stmt =
        conn.prepare("SELECT id, span_quoted, span_start FROM action_items WHERE entry_id = ?1")?;
    let rows: Vec<(String, String, i64)> = stmt
        .query_map(params![entry_id], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })?
        .collect::<rusqlite::Result<_>>()?;

    for (id, quote, was_at) in rows {
        match nearest_occurrence(transcript, &quote, was_at) {
            Some((at, end)) => conn.execute(
                "UPDATE action_items SET span_start = ?2, span_end = ?3, stale = 0 WHERE id = ?1",
                params![id, at, end],
            )?,
            None => conn.execute(
                "UPDATE action_items SET stale = 1 WHERE id = ?1",
                params![id],
            )?,
        };
    }
    Ok(())
}

/// Type ids the classifier may choose between. Falls back to the built-in set
/// when the table is empty, because an empty enum would constrain the reply to
/// nothing at all.
pub fn type_ids(conn: &Connection) -> Result<Vec<String>> {
    let mut stmt = conn.prepare("SELECT id FROM types ORDER BY built_in DESC, created_at ASC")?;
    let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
    let found: Vec<String> = rows.collect::<rusqlite::Result<Vec<_>>>()?;
    if found.is_empty() {
        return Ok(vec![
            "position".to_string(),
            "evidence".to_string(),
            "note".to_string(),
        ]);
    }
    Ok(found)
}

/// True when no classification has ever been written for this note.
///
/// Read off `move_phrase` because it is the one column only `set_classification`
/// ever writes. Title, summary, role and register all have innocent values a
/// real classification can produce -- a live note legitimately has no summary,
/// and §1.1 is why -- so none of them can tell "not yet" from "decided".
pub fn never_classified(conn: &Connection, id: &str) -> Result<bool> {
    let missing: Option<bool> = conn
        .query_row(
            "SELECT move_phrase IS NULL FROM entries WHERE id = ?1",
            params![id],
            |row| row.get(0),
        )
        .optional()?;
    Ok(missing.unwrap_or(false))
}

/// Changes one note's register, leaving everything else the classifier decided
/// alone.
///
/// Separate from `set_classification` because this is the user overruling one
/// judgement, not enrichment rewriting its own work: reusing that would make
/// the caller resupply a title and summary it has no business restating.
pub fn set_register(conn: &Connection, id: &str, register: Register) -> Result<()> {
    let changed = conn.execute(
        "UPDATE entries SET register = ?2 WHERE id = ?1",
        params![id, register_str(register)],
    )?;
    if changed == 0 {
        return Err(crate::error::Error::NotFound(format!("no entry {id}")));
    }
    Ok(())
}

/// Enrichment's only write to an entry. The move phrase has no column yet, so
/// the caller drops it; it belongs with embeddings, which do not exist.
pub fn set_classification(
    conn: &Connection,
    id: &str,
    title: &str,
    role: Role,
    register: Register,
    type_id: &str,
    summary: Option<&str>,
    // §7.1 -- what the note does with its subject removed. Stored but not on
    // the wire: machinery for finding two notes making the same move, not
    // something the panel renders.
    move_phrase: Option<&str>,
) -> Result<()> {
    let changed = conn.execute(
        "UPDATE entries SET title = ?2, role = ?3, register = ?4, type_id = ?5, summary = ?6,
         move_phrase = ?7
         WHERE id = ?1",
        params![
            id,
            title,
            role_str(role),
            register_str(register),
            type_id,
            summary,
            move_phrase
        ],
    )?;
    if changed == 0 {
        return Err(crate::error::Error::NotFound(format!("no entry {id}")));
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

/// Stale spans stay in SQLite and stop at this boundary. A span exists to mark
/// a region of the transcript; once a correction loses its words, its offsets
/// are the last place they were and index nothing, so handing them over would
/// paint "someone else said this" across whatever moved into that place.
///
/// Dropping it here rather than deleting the row also keeps the damage local:
/// an export carries spans and `insert` re-derives `quoted_text` from whatever
/// offsets arrive, so a dangling span that survived this far would come back
/// from a round trip attributing words nobody said.
fn spans_by_entry(
    conn: &Connection,
    scope: &str,
    param: &[&dyn rusqlite::ToSql],
) -> Result<HashMap<String, Vec<Span>>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT entry_id, start_offset, end_offset, attributed FROM spans
         WHERE {scope} AND stale = 0 ORDER BY id"
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

    /// The signal that decides whether opening a note asks for a pass. Title,
    /// summary, role and register all have innocent values a real
    /// classification produces -- a live note has no summary by design (§1.1) --
    /// so only the column classification alone writes can tell "not yet" from
    /// "decided".
    #[test]
    fn a_note_is_unclassified_until_a_move_phrase_is_written() {
        let conn = open_in_memory().unwrap();
        let made = entry("e1", "2024-01-01T00:00:00Z", false);
        insert(&conn, &made).unwrap();

        assert!(
            never_classified(&conn, "e1").unwrap(),
            "a freshly inserted note read as already classified"
        );

        set_classification(
            &conn,
            "e1",
            "a title",
            Role::Position,
            Register::Live,
            "position",
            // A live note carries no summary, which is exactly why the summary
            // cannot be the signal.
            None,
            Some("trades one cost for another"),
        )
        .unwrap();

        assert!(
            !never_classified(&conn, "e1").unwrap(),
            "a classified live note still read as never classified"
        );
    }

    /// A note that is not there has nothing owing.
    #[test]
    fn an_unknown_note_is_not_reported_as_unclassified() {
        let conn = open_in_memory().unwrap();
        assert!(!never_classified(&conn, "nope").unwrap());
    }

    /// `quoted` is the one place a span offset has to become a byte offset:
    /// everything above SQLite counts UTF-16 units, and Rust slices bytes.
    #[test]
    fn quoted_reads_utf16_offsets() {
        let transcript = "Observability — not logging — is the claim here.";
        let at = transcript.find("the claim").unwrap();
        let span = Span {
            start: crate::text::byte_to_utf16(transcript, at),
            end: crate::text::byte_to_utf16(transcript, at + "the claim".len()),
            attributed: false,
        };
        assert_eq!(quoted(transcript, &span), "the claim");
    }

    /// Half a surrogate pair is a legal UTF-16 index and not a character
    /// boundary. Returning nothing here would lose the stored quote.
    #[test]
    fn quoted_clamps_a_split_surrogate_pair() {
        let transcript = "a\u{1f3a7}b";
        let span = Span {
            start: 1,
            end: 4,
            attributed: false,
        };
        assert_eq!(quoted(transcript, &span), "\u{1f3a7}b");
    }

    /// §6.3 -- the text is the point. A bare flag records that you stopped,
    /// not what you concluded, which is the one thing worth keeping.
    #[test]
    fn resolving_stores_what_was_resolved() {
        let conn = open_in_memory().unwrap();
        insert(&conn, &entry("e1", "2024-02-03T10:21:00.000Z", false)).unwrap();

        resolve(&conn, "e1", "Split the table and stopped worrying about it").unwrap();

        let after = get(&conn, "e1").unwrap().unwrap();
        assert!(after.resolved);
        assert_eq!(
            after.resolution_text.as_deref(),
            Some("Split the table and stopped worrying about it")
        );
    }

    #[test]
    fn a_resolution_with_no_text_is_refused() {
        let conn = open_in_memory().unwrap();
        insert(&conn, &entry("e1", "2024-02-03T10:21:00.000Z", false)).unwrap();

        assert!(resolve(&conn, "e1", "   ").is_err());
        assert!(!get(&conn, "e1").unwrap().unwrap().resolved);
    }

    /// Reopening says you are not done, not that you never said it. Clearing
    /// the text would delete a conclusion you actually reached.
    #[test]
    fn reopening_keeps_the_text() {
        let conn = open_in_memory().unwrap();
        insert(&conn, &entry("e1", "2024-02-03T10:21:00.000Z", false)).unwrap();
        resolve(&conn, "e1", "Split the table").unwrap();

        reopen(&conn, "e1").unwrap();

        let after = get(&conn, "e1").unwrap().unwrap();
        assert!(!after.resolved);
        assert_eq!(after.resolution_text.as_deref(), Some("Split the table"));
    }

    /// An update against an id that is not there changes nothing and must not
    /// report success: the command returns the entry it claims to have written.
    #[test]
    fn resolving_something_that_is_not_there_is_not_found() {
        let conn = open_in_memory().unwrap();
        assert!(resolve(&conn, "nobody", "done").is_err());
        assert!(reopen(&conn, "nobody").is_err());
    }

    /// Generated on every capture since enrichment landed and thrown away for
    /// want of a column. It is stored and deliberately not on the wire.
    #[test]
    fn classification_stores_the_move_phrase() {
        let conn = open_in_memory().unwrap();
        insert(&conn, &entry("e1", "2024-02-03T10:21:00.000Z", false)).unwrap();

        set_classification(
            &conn,
            "e1",
            "indexes trade writes",
            Role::Evidence,
            Register::Neutral,
            "evidence",
            Some("a summary"),
            Some("trades one cost for another"),
        )
        .unwrap();

        let stored: Option<String> = conn
            .query_row("SELECT move_phrase FROM entries WHERE id = 'e1'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(stored.as_deref(), Some("trades one cost for another"));
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

    /// A bulk load is read back in the order it was said, so the early notes
    /// seed the shelves the later ones fold onto, and each note meets the
    /// corpus as it stood when it was recorded -- the same as a capture does.
    #[test]
    fn a_bulk_load_is_read_back_oldest_first() {
        let conn = open_in_memory().unwrap();
        insert(&conn, &entry("later", "2025-12-08T16:56:00.000Z", false)).unwrap();
        insert(&conn, &entry("earlier", "2024-01-14T09:38:00.000Z", false)).unwrap();
        insert(&conn, &entry("between", "2024-11-11T08:00:00.000Z", false)).unwrap();

        let ids: Vec<String> = ["later", "earlier", "gone", "between"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        // `gone` stands for a note deleted between the load and its pass.
        assert_eq!(
            oldest_first(&conn, &ids).unwrap(),
            vec!["earlier", "between", "later"]
        );
    }

    /// Cascades exist so a delete cannot leave spans or action items behind.
    #[test]
    fn deleting_an_entry_takes_its_own_rows() {
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

    /// Every offset column counts UTF-16 units; `match_indices` counts bytes.
    /// One em dash above the quote is enough to separate them, and a corrected
    /// note is exactly where non-ASCII shows up -- the user is fixing the
    /// transcript by hand, in a text field that produces real punctuation.
    #[test]
    fn reanchoring_stores_utf16_offsets_not_byte_offsets() {
        let conn = open_in_memory().unwrap();
        let said = "Observability — not logging — is the clam here.";
        let mut e = entry("e1", "2024-01-01T00:00:00Z", false);
        e.transcript = said.into();
        let at = crate::text::byte_to_utf16(said, said.find("logging").unwrap());
        e.spans = vec![Span {
            start: at,
            end: at + 7,
            attributed: true,
        }];
        e.action_items = vec![];
        insert(&conn, &e).unwrap();

        // A typo below the span: the quote does not move, so any drift is the
        // units, not the edit.
        let fixed = said.replace("clam", "claim");
        correct_transcript(&conn, "e1", &fixed).unwrap();

        let span = &get(&conn, "e1").unwrap().unwrap().spans[0];
        assert_eq!(
            quoted(&fixed, span),
            "logging",
            "the highlight landed somewhere other than the words it stores"
        );
    }

    /// Marking a span stale in SQLite and then handing the frontend its old
    /// offsets anyway is the dangling pointer the stale flag exists to stop:
    /// the panel highlights by offset, so a stale span that reaches the wire
    /// paints "someone else said this" over whatever moved into its place.
    #[test]
    fn a_stale_span_does_not_reach_the_wire() {
        let conn = open_in_memory().unwrap();
        let mut e = entry("e1", "2024-01-01T00:00:00Z", false);
        e.transcript = "Indexes trade writes for reads.".into();
        e.spans = vec![
            Span {
                start: 0,
                end: 7,
                attributed: true,
            },
            Span {
                start: 8,
                end: 13,
                attributed: true,
            },
        ];
        insert(&conn, &e).unwrap();

        // "Indexes" survives the correction; "trade" does not.
        correct_transcript(&conn, "e1", "Indexes are the whole argument.").unwrap();

        let spans = &get(&conn, "e1").unwrap().unwrap().spans;
        assert_eq!(spans.len(), 1, "the lost anchor must not be handed over");
        assert_eq!(
            quoted("Indexes are the whole argument.", &spans[0]),
            "Indexes"
        );

        let kept: i64 = conn
            .query_row(
                "SELECT count(*) FROM spans WHERE entry_id = 'e1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(kept, 2, "dropped from the wire, still in the record");
    }

    /// Blanking a note is deletion wearing a correction's clothes: it empties
    /// the verbatim record and stales every anchor on it. Delete says so.
    #[test]
    fn a_correction_cannot_empty_the_note() {
        let conn = open_in_memory().unwrap();
        insert(&conn, &entry("e1", "2024-02-03T10:21:00.000Z", false)).unwrap();

        assert!(correct_transcript(&conn, "e1", "   ").is_err());
        assert_eq!(
            get(&conn, "e1").unwrap().unwrap().transcript,
            "Indexes trade write performance for faster reads.",
            "a refused correction must not have written anything"
        );
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

    /// The doc comment claims children are orphaned rather than deleted --
    /// an answer is still something you said. Untested until now.
    #[test]
    fn deleting_a_parent_orphans_its_answers_rather_than_deleting_them() {
        let conn = open_in_memory().unwrap();
        insert(&conn, &entry("parent", "2024-01-01T00:00:00Z", false)).unwrap();

        let mut answer = entry("answer", "2024-01-02T00:00:00Z", false);
        answer.parent_entry_id = Some("parent".into());
        insert(&conn, &answer).unwrap();

        delete(&conn, "parent").unwrap();

        let survivors = list(&conn).unwrap();
        assert_eq!(survivors.len(), 1, "the answer must survive its parent");
        assert_eq!(survivors[0].id, "answer");
        assert!(
            survivors[0].parent_entry_id.is_none(),
            "and be orphaned, not dangling"
        );
    }

    #[test]
    fn deleting_an_entry_takes_its_questions() {
        let conn = open_in_memory().unwrap();
        insert(&conn, &entry("e1", "2024-01-01T00:00:00Z", false)).unwrap();
        conn.execute(
            "INSERT INTO questions (id, entry_id, text, answered, dismissed, provider_name, created_at)
             VALUES ('q1','e1','why?',0,0,'llama-server','now')",
            [],
        )
        .unwrap();

        delete(&conn, "e1").unwrap();

        let n: i64 = conn
            .query_row("SELECT count(*) FROM questions", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 0);
    }

    /// A quote that appears twice must not collapse onto the first occurrence.
    /// An attributed span drifting onto the user's own words would let a probe
    /// push on someone else's sentence -- the case §3.3 forbids.
    #[test]
    fn a_repeated_quote_reanchors_to_its_own_occurrence() {
        let conn = open_in_memory().unwrap();
        let said = "I think that is fine. Later on, he said I think that is fine.";
        let mut e = entry("e1", "2024-01-01T00:00:00Z", false);
        e.transcript = said.into();
        let second = said.rfind("I think that is fine").unwrap() as u32;
        e.spans = vec![
            Span {
                start: 0,
                end: 20,
                attributed: false,
            },
            Span {
                start: second,
                end: second + 20,
                attributed: true,
            },
        ];
        insert(&conn, &e).unwrap();

        // A correction earlier in the text shifts everything after it.
        let fixed = said.replace("Later on,", "Later,");
        correct_transcript(&conn, "e1", &fixed).unwrap();

        let spans = &get(&conn, "e1").unwrap().unwrap().spans;
        let attributed: Vec<&Span> = spans.iter().filter(|s| s.attributed).collect();
        assert_eq!(attributed.len(), 1);
        let expected = fixed.rfind("I think that is fine").unwrap() as u32;
        assert_eq!(
            attributed[0].start, expected,
            "the attributed span belongs to the second occurrence, not the first"
        );
    }

    /// Same rule as spans: an action item whose anchor is gone must say so
    /// rather than keep offsets that now index unrelated text.
    #[test]
    fn an_action_item_with_a_lost_anchor_goes_stale() {
        let conn = open_in_memory().unwrap();
        let mut e = entry("e1", "2024-01-01T00:00:00Z", false);
        e.transcript = "Buy a cable and renew the token.".into();
        e.action_items = vec![ActionItem {
            id: "t1".into(),
            entry_id: "e1".into(),
            span: Span {
                start: 0,
                end: 11,
                attributed: false,
            },
            text: "Buy a cable".into(),
            done: false,
        }];
        insert(&conn, &e).unwrap();

        correct_transcript(&conn, "e1", "Something else entirely.").unwrap();

        let (stale, kept): (bool, i64) = conn
            .query_row(
                "SELECT stale, (SELECT count(*) FROM action_items) FROM action_items WHERE id = 't1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert!(
            stale,
            "a lost action-item anchor must not keep pointing somewhere"
        );
        assert_eq!(kept, 1, "stale, not deleted -- the tick is still yours");
    }

    /// Four tables make one entry, so a failure writing the fourth must not
    /// leave the first three behind.
    #[test]
    fn a_failed_insert_leaves_no_partial_entry() {
        let conn = open_in_memory().unwrap();
        let mut e = entry("e1", "2024-01-01T00:00:00Z", true);
        e.spans = vec![Span {
            start: 0,
            end: 7,
            attributed: false,
        }];
        let item = ActionItem {
            id: "same".into(),
            entry_id: e.id.clone(),
            span: Span {
                start: 0,
                end: 7,
                attributed: false,
            },
            text: "Check the index".into(),
            done: false,
        };
        // The duplicate key fails only after entry, audio and span rows land.
        e.action_items = vec![item.clone(), item];

        assert!(insert(&conn, &e).is_err());

        assert!(
            list(&conn).unwrap().is_empty(),
            "the entry outlived its own failed insert"
        );
        for table in ["audio", "spans", "action_items"] {
            let n: i64 = conn
                .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
                .unwrap();
            assert_eq!(n, 0, "{table} kept a row");
        }
    }
}
