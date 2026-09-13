//! Proposing connections for a note that has just landed.
//!
//! Runs where enrichment runs: after the entry is durable, never before. It
//! may be slow, absent or wrong, and none of that may cost a recording (§9.4).
//!
//! The cost is one model call per candidate, which is why the cap is applied
//! by `candidates` before anything reaches here rather than being trusted to
//! a caller.

use super::{candidates::candidates, connect::judge};
use crate::db;
use crate::error::Result;
use crate::llm::LlmProvider;
use crate::model::{Edge, EdgeStatus};
use rusqlite::Connection;

/// Judges this note against its candidates and stores what survives.
///
/// Returns how many edges landed. A candidate the judge declines is not an
/// error and leaves no trace: most pairs sharing a shelf are merely on one
/// subject, and §5.4 would rather draw nothing than draw "related".
pub fn propose(
    conn: &Connection,
    provider: &dyn LlmProvider,
    entry_id: &str,
    cap: usize,
) -> Result<usize> {
    let Some(mine) = db::entries::get(conn, entry_id)? else {
        return Ok(0);
    };

    let mut landed = 0;
    for other_id in candidates(conn, entry_id, cap)? {
        // Checked before the call, not after: the model call is the expensive
        // thing, and a pair already judged once -- or dismissed -- must not
        // cost another.
        if db::edges::between(conn, &other_id, entry_id)? {
            continue;
        }
        let Some(other) = db::entries::get(conn, &other_id)? else {
            continue;
        };

        // The new note is the second, because the relation is what it does to
        // the one already there.
        let Some(proposal) = judge(provider, &other, &mine)? else {
            continue;
        };

        let edge = Edge {
            id: format!("edge-{}", uuid::Uuid::new_v4()),
            entry_a: other_id,
            entry_b: entry_id.to_string(),
            relation: proposal.relation,
            question: Some(proposal.question),
            status: EdgeStatus::Proposed,
            created_at: chrono::Utc::now().to_rfc3339(),
        };
        db::edges::insert(conn, &edge)?;
        landed += 1;
    }
    Ok(landed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::tags::Kind;
    use crate::llm::fake::ScriptedProvider;

    const A: &str = "Database indexes trade write performance for faster reads.";
    const B: &str = "Retries can make distributed systems less reliable under load.";

    fn note(conn: &Connection, transcript: &str) -> String {
        db::create::create(
            conn,
            db::create::NewEntry {
                transcript: transcript.into(),
                duration_ms: 5_000,
                fingerprint: vec![],
                parent_entry_id: None,
                local_only: None,
                typed: true,
            },
        )
        .unwrap()
        .id
    }

    fn shelve(conn: &Connection, id: &str, topic: &str) {
        let ids = db::tags::upsert(conn, &[topic.to_string()], Kind::Topic).unwrap();
        db::tags::set_for_entry(conn, id, &ids).unwrap();
    }

    /// Two notes on one shelf, and a judge willing to name what passes between
    /// them.
    fn pair(conn: &Connection) -> (String, String) {
        let a = note(conn, A);
        let b = note(conn, B);
        shelve(conn, &a, "systems");
        shelve(conn, &b, "systems");
        (a, b)
    }

    fn says(relation: &str, qa: &str, qb: &str) -> String {
        serde_json::json!({
            "relation": relation, "quoteA": qa, "quoteB": qb,
            "question": "Does each make the thing it improves worse elsewhere?"
        })
        .to_string()
    }

    #[test]
    fn a_judged_pair_lands_as_a_proposal() {
        let conn = db::open_in_memory().unwrap();
        let (a, b) = pair(&conn);
        let p = ScriptedProvider::with(&[&says(
            "same move",
            "trade write performance",
            "less reliable",
        )]);

        assert_eq!(propose(&conn, &p, &b, 8).unwrap(), 1);
        let edges = db::edges::list(&conn).unwrap();
        assert_eq!(edges.len(), 1);
        assert!(matches!(edges[0].status, EdgeStatus::Proposed));
        assert!(edges[0].question.is_some(), "an edge carries its reason");
        // The new note is the second: the relation is what it does to the one
        // already there, which is the direction the judge was asked about.
        assert_eq!(edges[0].entry_a, a);
        assert_eq!(edges[0].entry_b, b);
    }

    /// Found in the packaged app: loading the sample ran passes in whatever
    /// order they happened to finish, so an older note was often the one being
    /// enriched -- and 15 of 19 edges pointed from a newer note to an older
    /// one, with the judge shown the dates out of order. `returns to` and
    /// `extends` are claims about time; the older note is always the first.
    #[test]
    fn an_older_note_enriched_late_is_still_the_first() {
        let conn = db::open_in_memory().unwrap();
        let (older, newer) = pair(&conn);
        conn.execute(
            "UPDATE entries SET created_at = ?2 WHERE id = ?1",
            [&older, "2024-01-14T09:38:00.000Z"],
        )
        .unwrap();
        conn.execute(
            "UPDATE entries SET created_at = ?2 WHERE id = ?1",
            [&newer, "2025-12-08T16:56:00.000Z"],
        )
        .unwrap();
        let p = ScriptedProvider::with(&[&says(
            "extends",
            "trade write performance",
            "less reliable",
        )]);

        assert_eq!(propose(&conn, &p, &older, 8).unwrap(), 1);
        let edges = db::edges::list(&conn).unwrap();
        assert_eq!(edges[0].entry_a, older);
        assert_eq!(edges[0].entry_b, newer);
        let asked = &p.asked.lock().unwrap()[0];
        assert!(
            asked.find(A).unwrap() < asked.find(B).unwrap(),
            "the judge must read the older note first"
        );
    }

    #[test]
    fn a_declined_candidate_leaves_no_trace() {
        let conn = db::open_in_memory().unwrap();
        let (_, b) = pair(&conn);
        let p = ScriptedProvider::with(&[&says("none", "", "")]);

        assert_eq!(propose(&conn, &p, &b, 8).unwrap(), 0);
        assert!(db::edges::list(&conn).unwrap().is_empty());
    }

    /// Dismissals are kept so the app does not nag, and with the vocabulary
    /// invisible this is the only correction the user has.
    #[test]
    fn a_dismissed_pair_is_never_judged_again() {
        let conn = db::open_in_memory().unwrap();
        let (a, b) = pair(&conn);
        db::edges::insert(
            &conn,
            &Edge {
                id: "edge-old".into(),
                entry_a: a,
                entry_b: b.clone(),
                relation: crate::model::Relation::Extends,
                question: Some("asked once".into()),
                status: EdgeStatus::Dismissed,
                created_at: "2024-01-01T00:00:00Z".into(),
            },
        )
        .unwrap();

        // No replies scripted: reaching the model at all would panic.
        let p = ScriptedProvider::with(&[]);
        assert_eq!(propose(&conn, &p, &b, 8).unwrap(), 0);
        assert_eq!(db::edges::list(&conn).unwrap().len(), 1, "it re-proposed");
    }

    /// One model call per candidate, so the cap is the cost.
    #[test]
    fn no_more_judge_calls_than_the_cap() {
        let conn = db::open_in_memory().unwrap();
        let mine = note(&conn, A);
        shelve(&conn, &mine, "systems");
        for _ in 0..6 {
            let other = note(&conn, B);
            shelve(&conn, &other, "systems");
        }
        let reply = says("none", "", "");
        let replies: Vec<&str> = std::iter::repeat(reply.as_str()).take(2).collect();
        let p = ScriptedProvider::with(&replies);

        // Two scripted replies and a cap of two: a third call would panic.
        assert_eq!(propose(&conn, &p, &mine, 2).unwrap(), 0);
    }

    /// A note nothing shares a shelf with is not an error, just quiet.
    #[test]
    fn a_note_with_no_candidates_proposes_nothing() {
        let conn = db::open_in_memory().unwrap();
        let lonely = note(&conn, A);
        shelve(&conn, &lonely, "systems");
        let p = ScriptedProvider::with(&[]);
        assert_eq!(propose(&conn, &p, &lonely, 8).unwrap(), 0);
    }
}
