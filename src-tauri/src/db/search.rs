//! Search over transcripts.
//!
//! Substring by default, because most searches are for a phrase half
//! remembered. A query in double quotes matches whole words only -- the way to
//! ask for a subject rather than any word containing it, since `ai` otherwise
//! finds maintain and explaining.

use crate::error::Result;
use rusqlite::Connection;
use serde::Serialize;

/// Offsets are into the transcript; the snippet ones are the same match
/// re-based into `snippet`, so the caller highlights without re-searching.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SearchHit {
    pub entry_id: String,
    pub start: u32,
    pub end: u32,
    pub snippet: String,
    pub snippet_start: u32,
    pub snippet_end: u32,
}

const SNIPPET_RADIUS: usize = 40;
const MAX_HITS: usize = 50;
const MAX_PER_ENTRY: usize = 3;

/// True when a match sits on word boundaries. Alphanumeric either side means
/// the needle is part of a longer word, which is what quoting excludes.
fn on_word_boundaries(haystack: &str, start: usize, end: usize) -> bool {
    let before = haystack[..start].chars().next_back();
    let after = haystack[end..].chars().next();
    let boundary = |c: Option<char>| !matches!(c, Some(ch) if ch.is_alphanumeric());
    boundary(before) && boundary(after)
}

/// A window around the match, cut on character boundaries -- a transcript is
/// whatever was said, and slicing mid-character panics.
fn snippet_around(transcript: &str, start: usize, end: usize) -> (String, u32, u32) {
    let mut from = start.saturating_sub(SNIPPET_RADIUS);
    let mut to = (end + SNIPPET_RADIUS).min(transcript.len());
    while !transcript.is_char_boundary(from) {
        from -= 1;
    }
    while !transcript.is_char_boundary(to) {
        to += 1;
    }

    let mut snippet = String::new();
    let mut offset = 0usize;
    if from > 0 {
        snippet.push_str("… ");
        offset = snippet.len();
    }
    snippet.push_str(&transcript[from..to]);
    if to < transcript.len() {
        snippet.push_str(" …");
    }

    let snippet_start = offset + (start - from);
    let snippet_end = snippet_start + (end - start);
    (snippet, snippet_start as u32, snippet_end as u32)
}

pub fn search(conn: &Connection, query: &str) -> Result<Vec<SearchHit>> {
    let trimmed = query.trim();
    // Quoting is the only modifier, and it means whole words.
    let whole_words = trimmed.len() >= 2 && trimmed.starts_with('"') && trimmed.ends_with('"');
    let needle = if whole_words {
        trimmed[1..trimmed.len() - 1].trim()
    } else {
        trimmed
    };
    if needle.is_empty() {
        return Ok(Vec::new());
    }
    let needle_lower = needle.to_lowercase();

    let mut stmt = conn.prepare("SELECT id, transcript FROM entries ORDER BY created_at ASC")?;
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;

    let mut hits = Vec::new();
    for row in rows {
        let (entry_id, transcript) = row?;
        let lower = transcript.to_lowercase();

        // Case folding can change byte length, so offsets from the lowered
        // copy would be wrong on anything non-ASCII. Only searched when the
        // two agree; otherwise the original is scanned directly.
        let searchable = if lower.len() == transcript.len() {
            lower
        } else {
            transcript.to_lowercase()
        };
        let aligned = searchable.len() == transcript.len();

        let mut found = 0;
        let mut from = 0usize;
        while found < MAX_PER_ENTRY && hits.len() < MAX_HITS {
            let hay: &str = if aligned { &searchable } else { &transcript };
            let Some(offset) = find_from(hay, &needle_lower, needle, aligned, from) else {
                break;
            };
            let end = offset + needle.len();
            from = offset + 1;

            if whole_words && !on_word_boundaries(&transcript, offset, end) {
                continue;
            }

            let (snippet, snippet_start, snippet_end) = snippet_around(&transcript, offset, end);
            hits.push(SearchHit {
                entry_id: entry_id.clone(),
                start: offset as u32,
                end: end as u32,
                snippet,
                snippet_start,
                snippet_end,
            });
            found += 1;
        }
        if hits.len() >= MAX_HITS {
            break;
        }
    }
    Ok(hits)
}

/// Case-insensitive find that reports offsets into the original string.
fn find_from(
    hay: &str,
    needle_lower: &str,
    needle: &str,
    aligned: bool,
    from: usize,
) -> Option<usize> {
    if from > hay.len() {
        return None;
    }
    let mut at = from;
    while !hay.is_char_boundary(at) {
        at += 1;
    }
    if aligned {
        hay[at..].find(needle_lower).map(|i| i + at)
    } else {
        // Fold both sides per candidate so offsets stay in the original.
        hay[at..]
            .char_indices()
            .find(|(i, _)| {
                let start = at + i;
                hay.get(start..start + needle.len())
                    .is_some_and(|slice| slice.to_lowercase() == needle_lower)
            })
            .map(|(i, _)| at + i)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_in_memory;

    fn seed(conn: &Connection, id: &str, created_at: &str, transcript: &str) {
        conn.execute(
            "INSERT INTO entries (id, transcript, created_at, x, y, role, register,
             type_id, resolved, title, duration_ms, unfinished, local_only, is_sample)
             VALUES (?1, ?2, ?3, 0, 0, 'position', 'neutral', 'position', 0, 't', 1000, 0, 0, 0)",
            rusqlite::params![id, transcript, created_at],
        )
        .unwrap();
    }

    fn corpus() -> Connection {
        let conn = open_in_memory().unwrap();
        seed(
            &conn,
            "a",
            "2024-01-01T00:00:00Z",
            "An AI system could be good at this.",
        );
        seed(
            &conn,
            "b",
            "2024-01-02T00:00:00Z",
            "You have to maintain the index, explaining the cost.",
        );
        seed(
            &conn,
            "c",
            "2024-01-03T00:00:00Z",
            "Reasoning about reasoning is still reasoning.",
        );
        conn
    }

    #[test]
    fn a_bare_query_matches_inside_words() {
        let hits = search(&corpus(), "reason").unwrap();
        assert!(
            !hits.is_empty(),
            "a half-remembered phrase should still find it"
        );
        assert!(hits.iter().all(|h| h.entry_id == "c"));
    }

    /// The whole reason quoting exists: `ai` should not find maintain.
    #[test]
    fn a_quoted_query_matches_whole_words_only() {
        let loose = search(&corpus(), "ai").unwrap();
        assert!(
            loose.iter().any(|h| h.entry_id == "b"),
            "unquoted, it should find maintain"
        );

        let exact = search(&corpus(), "\"ai\"").unwrap();
        assert!(
            exact.iter().all(|h| h.entry_id == "a"),
            "quoted, only the subject"
        );
        assert!(!exact.is_empty());
    }

    #[test]
    fn search_ignores_case() {
        assert!(!search(&corpus(), "AN ai SYSTEM").unwrap().is_empty());
    }

    #[test]
    fn offsets_point_at_the_match_in_the_transcript() {
        let hits = search(&corpus(), "index").unwrap();
        let hit = &hits[0];
        let transcript = "You have to maintain the index, explaining the cost.";
        assert_eq!(&transcript[hit.start as usize..hit.end as usize], "index");
    }

    /// The snippet offsets are what the UI highlights with, so they have to
    /// index the snippet rather than the transcript.
    #[test]
    fn snippet_offsets_index_the_snippet() {
        let hits = search(&corpus(), "index").unwrap();
        let hit = &hits[0];
        assert_eq!(
            &hit.snippet[hit.snippet_start as usize..hit.snippet_end as usize],
            "index"
        );
    }

    #[test]
    fn a_short_transcript_is_its_own_snippet() {
        let conn = open_in_memory().unwrap();
        seed(&conn, "x", "2024-01-01T00:00:00Z", "Short one.");
        let hits = search(&conn, "Short").unwrap();
        assert_eq!(hits[0].snippet, "Short one.");
    }

    #[test]
    fn a_long_transcript_is_trimmed_around_the_match() {
        let conn = open_in_memory().unwrap();
        let long = format!("{}NEEDLE{}", "a".repeat(500), "b".repeat(500));
        seed(&conn, "x", "2024-01-01T00:00:00Z", &long);

        let hits = search(&conn, "NEEDLE").unwrap();
        assert!(
            hits[0].snippet.len() < 120,
            "got {} chars",
            hits[0].snippet.len()
        );
        assert!(hits[0].snippet.contains("NEEDLE"));
    }

    /// One entry cannot flood the results.
    #[test]
    fn an_entry_contributes_at_most_three_hits() {
        let conn = open_in_memory().unwrap();
        seed(&conn, "x", "2024-01-01T00:00:00Z", "ab ab ab ab ab ab ab");
        assert_eq!(search(&conn, "ab").unwrap().len(), MAX_PER_ENTRY);
    }

    #[test]
    fn results_are_capped() {
        let conn = open_in_memory().unwrap();
        for i in 0..40 {
            seed(
                &conn,
                &format!("e{i}"),
                "2024-01-01T00:00:00Z",
                "ab ab ab ab",
            );
        }
        assert!(search(&conn, "ab").unwrap().len() <= MAX_HITS);
    }

    #[test]
    fn an_empty_query_finds_nothing() {
        assert!(search(&corpus(), "").unwrap().is_empty());
        assert!(search(&corpus(), "   ").unwrap().is_empty());
    }

    #[test]
    fn a_query_matching_nothing_returns_nothing() {
        assert!(search(&corpus(), "zebra").unwrap().is_empty());
    }

    /// Slicing a transcript mid-character would panic, and a transcript is
    /// whatever the speaker said.
    #[test]
    fn multibyte_transcripts_do_not_panic() {
        let conn = open_in_memory().unwrap();
        seed(
            &conn,
            "x",
            "2024-01-01T00:00:00Z",
            "café — naïve — 日本語 — needle — more",
        );
        let hits = search(&conn, "needle").unwrap();
        assert_eq!(hits.len(), 1);
        assert!(hits[0].snippet.contains("needle"));
    }
}
