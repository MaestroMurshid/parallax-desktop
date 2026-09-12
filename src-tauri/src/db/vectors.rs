//! One vector per note, and what makes two notes about one idea ever meet.
//!
//! Tags were meant to be this and cannot be: measured over the sixteen
//! fixtures they put 28 names on 16 notes, 28 of them used exactly once, and
//! surfaced 0 of the 12 authored edges. A tag grounded in the note's own words
//! is too specific to collide and an abstract one lands on everything, so the
//! candidate set was empty either way. Cosine does not need two notes to reach
//! for the same word.
//!
//! Internal machinery, never a surface, for the same reason `tags` was.

use crate::error::{Error, Result};
use crate::scene::vector;
use rusqlite::{params, Connection};

/// Vectors are stored already normalised, so ranking is a dot product rather
/// than a cosine -- the scan touches every note in the corpus and that is the
/// whole budget. Measured: 10,000 notes at 384 dims is about 3ms.
pub fn set(conn: &Connection, entry_id: &str, model: &str, vec: &[f32]) -> Result<()> {
    let length = vector::norm(vec);
    if !length.is_finite() || length == 0.0 {
        return Err(Error::Other(
            "a vector with no direction cannot be compared".into(),
        ));
    }
    let unit: Vec<f32> = vec.iter().map(|x| x / length).collect();
    let mut blob = Vec::with_capacity(unit.len() * 4);
    for f in &unit {
        blob.extend_from_slice(&f.to_le_bytes());
    }

    // Replace rather than accumulate: a corrected transcript is re-embedded,
    // and two vectors for one note would rank it against itself twice.
    conn.execute(
        "INSERT INTO entry_vectors (entry_id, model, dims, vec, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(entry_id) DO UPDATE SET
             model = excluded.model, dims = excluded.dims,
             vec = excluded.vec, created_at = excluded.created_at",
        params![
            entry_id,
            model,
            unit.len() as i64,
            blob,
            chrono::Utc::now().to_rfc3339()
        ],
    )?;
    Ok(())
}

fn decode(blob: &[u8]) -> Vec<f32> {
    blob.chunks_exact(4)
        .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .collect()
}

pub fn get(conn: &Connection, entry_id: &str) -> Result<Option<(String, Vec<f32>)>> {
    let found = conn
        .query_row(
            "SELECT model, vec FROM entry_vectors WHERE entry_id = ?1",
            params![entry_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?)),
        )
        .ok();
    Ok(found.map(|(model, blob)| (model, decode(&blob))))
}

/// The notes most like this one, best first.
///
/// Only ever compares vectors written by the same model: two embeddings from
/// different models share no space, and ranking across them would return
/// plausible-looking nonsense rather than failing.
pub fn similar(conn: &Connection, entry_id: &str, k: usize) -> Result<Vec<(String, f32)>> {
    let Some((model, mine)) = get(conn, entry_id)? else {
        // Not an error: a note is unembedded between landing and enrichment,
        // and having no candidates then is the correct answer.
        return Ok(Vec::new());
    };

    let mut stmt = conn.prepare(
        "SELECT entry_id, vec FROM entry_vectors
         WHERE model = ?1 AND dims = ?2 AND entry_id != ?3",
    )?;
    let rows = stmt.query_map(params![model, mine.len() as i64, entry_id], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?))
    })?;

    let mut scored: Vec<(String, f32)> = rows
        .collect::<rusqlite::Result<Vec<_>>>()?
        .into_iter()
        .map(|(id, blob)| {
            // Both sides are unit vectors, so the dot product is the cosine.
            let score = vector::dot(&mine, &decode(&blob));
            (id, score)
        })
        .collect();

    // Ties broken by id so the candidate set is stable between runs -- an
    // unstable order would re-propose different pairs on every capture.
    scored.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    scored.truncate(k);
    Ok(scored)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{self, open_in_memory};

    const M: &str = "bge-small";

    fn note(conn: &Connection) -> String {
        db::create::create(
            conn,
            db::create::NewEntry {
                transcript: "said".into(),
                duration_ms: 1_000,
                fingerprint: vec![],
                parent_entry_id: None,
                local_only: None,
                typed: true,
            },
        )
        .unwrap()
        .id
    }

    #[test]
    fn a_vector_round_trips() {
        let conn = open_in_memory().unwrap();
        let a = note(&conn);
        set(&conn, &a, M, &[3.0, 4.0]).unwrap();
        let (model, v) = get(&conn, &a).unwrap().unwrap();
        assert_eq!(model, M);
        // Normalised on the way in: 3-4-5, so the unit vector is 0.6/0.8.
        assert!((v[0] - 0.6).abs() < 1e-6, "got {v:?}");
        assert!((v[1] - 0.8).abs() < 1e-6, "got {v:?}");
    }

    #[test]
    fn similar_ranks_by_closeness_and_excludes_the_note_itself() {
        let conn = open_in_memory().unwrap();
        let (me, near, far, opposite) = (note(&conn), note(&conn), note(&conn), note(&conn));
        set(&conn, &me, M, &[1.0, 0.0]).unwrap();
        set(&conn, &near, M, &[0.9, 0.1]).unwrap();
        set(&conn, &far, M, &[0.0, 1.0]).unwrap();
        set(&conn, &opposite, M, &[-1.0, 0.0]).unwrap();

        let got: Vec<String> = similar(&conn, &me, 10)
            .unwrap()
            .into_iter()
            .map(|(id, _)| id)
            .collect();
        assert_eq!(got, vec![near, far, opposite]);
    }

    /// The cap is the only thing between a large corpus and one judge call per
    /// note, so it is not decoration.
    #[test]
    fn similar_caps_at_k() {
        let conn = open_in_memory().unwrap();
        let me = note(&conn);
        set(&conn, &me, M, &[1.0, 0.0]).unwrap();
        for i in 0..10 {
            let other = note(&conn);
            set(&conn, &other, M, &[1.0, i as f32 / 10.0]).unwrap();
        }
        assert_eq!(similar(&conn, &me, 3).unwrap().len(), 3);
    }

    /// Two models do not share a vector space. Ranking across them returns
    /// plausible-looking nonsense, which is worse than returning nothing.
    #[test]
    fn a_vector_from_another_model_is_not_a_candidate() {
        let conn = open_in_memory().unwrap();
        let (me, same, other) = (note(&conn), note(&conn), note(&conn));
        set(&conn, &me, M, &[1.0, 0.0]).unwrap();
        set(&conn, &same, M, &[0.5, 0.5]).unwrap();
        set(&conn, &other, "a-different-model", &[1.0, 0.0]).unwrap();

        let got = similar(&conn, &me, 10).unwrap();
        assert_eq!(got.len(), 1, "only the matching model may be compared");
        assert_eq!(got[0].0, same);
    }

    #[test]
    fn a_note_with_no_vector_yet_has_no_candidates() {
        let conn = open_in_memory().unwrap();
        let me = note(&conn);
        let other = note(&conn);
        set(&conn, &other, M, &[1.0, 0.0]).unwrap();
        assert!(similar(&conn, &me, 5).unwrap().is_empty());
    }

    /// Re-embedding a corrected transcript must replace, not accumulate.
    #[test]
    fn re_embedding_replaces_the_vector() {
        let conn = open_in_memory().unwrap();
        let a = note(&conn);
        set(&conn, &a, M, &[1.0, 0.0]).unwrap();
        set(&conn, &a, M, &[0.0, 1.0]).unwrap();
        let (_, v) = get(&conn, &a).unwrap().unwrap();
        assert!((v[1] - 1.0).abs() < 1e-6, "got {v:?}");
    }

    /// A zero vector has no direction, so it cannot be normalised and must not
    /// be stored -- it would rank identically against everything.
    #[test]
    fn a_zero_vector_is_refused() {
        let conn = open_in_memory().unwrap();
        let a = note(&conn);
        assert!(set(&conn, &a, M, &[0.0, 0.0]).is_err());
    }

    /// Deleting a note takes its vector with it, or the scan ranks ghosts.
    #[test]
    fn a_deleted_note_takes_its_vector() {
        let conn = open_in_memory().unwrap();
        let a = note(&conn);
        set(&conn, &a, M, &[1.0, 0.0]).unwrap();
        conn.execute("DELETE FROM entries WHERE id = ?1", params![a])
            .unwrap();
        assert!(get(&conn, &a).unwrap().is_none());
    }
}
