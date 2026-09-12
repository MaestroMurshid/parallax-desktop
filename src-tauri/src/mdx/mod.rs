//! One note as one file, and back again.
//!
//! Transport and storage, never authoring. The only edit a note accepts is a
//! correction to its transcript; you do not append to a note, because the point
//! is to show how you got there verbatim. That is why this renders and parses
//! and does nothing else.
//!
//! **SQLite stays authoritative.** Offsets are frozen at insert (§5.1), so the
//! file carries them rather than deciding them: a round trip must return the
//! same numbers, not recompute them from the text it happens to hold.
//!
//! The frontmatter is JSON inside the `---` fence rather than YAML. YAML would
//! be the convention, but it coerces -- a bare `no` becomes false, an id of
//! digits becomes a number, a timestamp becomes a date -- and every field here
//! is an id, an offset or a verbatim string. A hand-edit that breaks JSON fails
//! loudly, which is the behaviour wanted from a file people are meant to edit.
//! No new dependency either, which matters for a project that still owes a
//! dependency audit.

pub mod corpus;

use crate::error::{Error, Result};
use crate::model::{Edge, Entry, Question};
use serde::{Deserialize, Serialize};

pub const FENCE: &str = "---";

/// Everything about one note that the corpus would otherwise hold in five
/// tables. Edges are carried by the note they leave, so each is written once.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Note {
    #[serde(flatten)]
    pub entry: Entry,
    /// Grounded and specific. In the file because this is where a power user
    /// edits them -- there is no panel for it and there is not going to be.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub anchors: Vec<String>,
    /// The shelf, and what candidates are drawn from.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub topics: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub questions: Vec<Question>,
    /// Only those where this note is `entry_a`, so the graph rebuilds from the
    /// files without every edge appearing twice and having to be reconciled.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub edges: Vec<Edge>,
}

/// The note as a file: JSON frontmatter, then the transcript verbatim.
pub fn render(note: &Note) -> Result<String> {
    let mut value = serde_json::to_value(note)
        .map_err(|e| Error::Other(format!("the note could not be written: {e}")))?;
    // The transcript is the body, not a field. A file carrying it in both
    // places could disagree with itself, and the body is the half a person
    // reads -- so the frontmatter is what gives way.
    if let Some(object) = value.as_object_mut() {
        object.remove("transcript");
    }
    let front = serde_json::to_string_pretty(&value)
        .map_err(|e| Error::Other(format!("the note could not be written: {e}")))?;

    // Exactly one newline after the transcript, so parsing can take exactly one
    // back off and a transcript that ends in a blank line still survives.
    Ok(format!(
        "{FENCE}\n{front}\n{FENCE}\n\n{}\n",
        note.entry.transcript
    ))
}

/// The file back as a note.
pub fn parse(text: &str) -> Result<Note> {
    // CRLF is not cosmetic here. Offsets are UTF-16 indices into the
    // transcript, so an editor that rewrote the line endings of a file it was
    // handed would move every span, action item and question in the note by one
    // unit per line. Normalising on the way in is what keeps them pointing at
    // the words they were frozen against (§5.1).
    let text = text.replace("\r\n", "\n");

    let opened = text
        .strip_prefix(&format!("{FENCE}\n"))
        .ok_or_else(|| Error::Other("the file does not begin with frontmatter".into()))?;
    let close = format!("\n{FENCE}\n");
    // The first closing fence only. A transcript is allowed to say `---` on a
    // line of its own; it is the record, and it does not get reparsed.
    let end = opened
        .find(&close)
        .ok_or_else(|| Error::Other("the frontmatter is never closed".into()))?;

    let front = &opened[..end];
    let after = &opened[end + close.len()..];
    let body = after.strip_prefix('\n').unwrap_or(after);
    let transcript = body.strip_suffix('\n').unwrap_or(body);

    let mut value: serde_json::Value = serde_json::from_str(front)
        .map_err(|e| Error::Other(format!("the frontmatter is not readable: {e}")))?;
    value
        .as_object_mut()
        .ok_or_else(|| Error::Other("the frontmatter is not an object".into()))?
        .insert(
            "transcript".into(),
            serde_json::Value::String(transcript.to_string()),
        );

    serde_json::from_value(value)
        .map_err(|e| Error::Other(format!("the note is missing something: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ActionItem, EdgeStatus, Register, Relation, Role, Span};

    pub fn example() -> Note {
        note("Database indexes trade write performance for faster reads.")
    }

    fn note(transcript: &str) -> Note {
        Note {
            entry: Entry {
                id: "entry-1".into(),
                audio_path: Some("audio/entry-1.wav".into()),
                transcript: transcript.into(),
                created_at: "2026-02-03T10:00:00Z".into(),
                x: -412.5,
                y: 88.25,
                parent_entry_id: None,
                answers_question_id: None,
                role: Role::Position,
                register: Register::Neutral,
                type_id: "position".into(),
                resolved: false,
                resolution_text: None,
                title: "indexes trade writes".into(),
                summary: Some("A claim about indexes.".into()),
                duration_ms: 7_400,
                fingerprint: vec![0.1, 0.75, 0.2],
                unfinished: false,
                local_only: false,
                spans: vec![Span {
                    start: 0,
                    end: 16,
                    attributed: false,
                }],
                action_items: vec![ActionItem {
                    id: "action-1".into(),
                    entry_id: "entry-1".into(),
                    text: "check the write path".into(),
                    done: false,
                    span: Span {
                        start: 5,
                        end: 12,
                        attributed: false,
                    },
                }],
                is_sample: None,
            },
            anchors: vec!["database-indexes".into()],
            topics: vec!["databases".into()],
            questions: Vec::new(),
            edges: vec![Edge {
                id: "edge-1".into(),
                entry_a: "entry-1".into(),
                entry_b: "entry-2".into(),
                // Spaces in the relation, which is the thing a looser format
                // would quietly turn into something else.
                relation: Relation::SameMove,
                question: Some("Does each trade one cost for another?".into()),
                status: EdgeStatus::Proposed,
                created_at: "2026-02-03T10:00:05Z".into(),
            }],
        }
    }

    fn round_trip(n: &Note) -> Note {
        parse(&render(n).unwrap()).unwrap()
    }

    #[test]
    fn a_note_survives_the_round_trip() {
        let before = note("Database indexes trade write performance for faster reads.");
        let after = round_trip(&before);

        assert_eq!(after.entry.transcript, before.entry.transcript);
        assert_eq!(after.entry.id, before.entry.id);
        assert_eq!(after.entry.title, before.entry.title);
        assert_eq!(after.anchors, before.anchors);
        assert_eq!(after.topics, before.topics);
        assert_eq!(after.entry.audio_path, before.entry.audio_path);
    }

    /// §5.1 freezes positions at insert, so the file carries them rather than
    /// deciding them. A round trip that moved a note would re-solve the field,
    /// which is the one thing the layout rule forbids.
    #[test]
    fn positions_come_back_exactly() {
        let before = note("said");
        let after = round_trip(&before);
        assert_eq!(after.entry.x, before.entry.x);
        assert_eq!(after.entry.y, before.entry.y);
    }

    /// Offsets are UTF-16 and index into the transcript. If they shift, every
    /// quote, action item and question in the corpus points at the wrong words.
    #[test]
    fn offsets_are_unchanged_by_a_round_trip() {
        let before = note("Database indexes trade write performance for faster reads.");
        let after = round_trip(&before);
        assert_eq!(after.entry.spans[0].start, before.entry.spans[0].start);
        assert_eq!(after.entry.spans[0].end, before.entry.spans[0].end);
        assert_eq!(after.entry.action_items[0].span.end, 12);
    }

    /// The transcript is the record and is never rewritten. A note that says
    /// `---` on a line of its own must not be read as a second fence.
    #[test]
    fn a_transcript_containing_the_fence_is_still_verbatim() {
        let said = "First I thought this\n---\nthen I thought the opposite.";
        let before = note(said);
        let after = round_trip(&before);
        assert_eq!(after.entry.transcript, said);
    }

    /// UTF-16 offsets and astral characters are how the span bug got in last
    /// time. A round trip must not touch either.
    #[test]
    fn an_astral_transcript_round_trips() {
        let said = "The chart said 📈 and I did not believe it.";
        let mut before = note(said);
        before.entry.spans = vec![Span {
            start: 15,
            end: 17,
            attributed: true,
        }];
        let after = round_trip(&before);
        assert_eq!(after.entry.transcript, said);
        assert_eq!(after.entry.spans[0].start, 15);
        assert_eq!(after.entry.spans[0].end, 17);
        assert!(after.entry.spans[0].attributed);
    }

    /// The hazard peculiar to a text file that carries offsets: an editor, or
    /// git with autocrlf, rewrites the line endings of a file it was handed,
    /// and every UTF-16 offset in the note shifts by one unit per line before
    /// it. The transcript would still look right and every span would be wrong.
    #[test]
    fn windows_line_endings_do_not_move_the_offsets() {
        let said = "First line.\nSecond line.\nThird line.";
        let mut before = note(said);
        // Pointing at "Third", which is 25 units in with unix endings.
        before.entry.spans = vec![Span {
            start: 25,
            end: 30,
            attributed: false,
        }];

        let rewritten = render(&before).unwrap().replace("\n", "\r\n");
        let after = parse(&rewritten).unwrap();

        assert_eq!(after.entry.transcript, said, "the endings came back in");
        assert_eq!(after.entry.spans[0].start, 25);
        let units: Vec<u16> = after.entry.transcript.encode_utf16().collect();
        assert_eq!(
            String::from_utf16(&units[25..30]).unwrap(),
            "Third",
            "the offset no longer points where it was frozen"
        );
    }

    #[test]
    fn a_relation_keeps_its_spaces() {
        let after = round_trip(&note("said"));
        assert_eq!(after.edges[0].relation, Relation::SameMove);
        assert_eq!(after.edges[0].entry_b, "entry-2");
        assert!(matches!(after.edges[0].status, EdgeStatus::Proposed));
    }

    /// A file is often the only other copy. Refusing loudly beats guessing.
    /// Not an assertion so much as a look at the artefact, since this format
    /// is meant to be read by people. Run with
    /// `cargo test --lib what_a_note_looks_like -- --nocapture`.
    #[test]
    fn what_a_note_looks_like_on_disk() {
        eprintln!("{}", render(&example()).unwrap());
    }

    #[test]
    fn a_file_with_no_frontmatter_is_refused() {
        assert!(parse("just some text with no fence at all").is_err());
    }

    #[test]
    fn a_file_with_unreadable_frontmatter_is_refused() {
        assert!(parse("---\n{ not json ]\n---\n\nsaid").is_err());
    }

    /// Readable on purpose: this is the power user's editing surface, and a
    /// single line of minified JSON is not one.
    #[test]
    fn the_frontmatter_is_laid_out_to_be_read() {
        let text = render(&note("said")).unwrap();
        assert!(text.starts_with("---\n"), "{text}");
        assert!(
            text.contains("\n  \"topics\": [\n"),
            "frontmatter should be indented: {text}"
        );
        assert!(
            text.trim_end().ends_with("said"),
            "the transcript comes last"
        );
    }
}
