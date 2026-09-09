//! Edges. The relation vocabulary crosses the SQL boundary as the same
//! strings it uses on the wire, spaces and all.

use crate::error::Result;
use crate::model::{Edge, EdgeStatus, Relation};
use rusqlite::{params, Connection, Row};

fn relation_from(s: &str) -> Relation {
    match s {
        "same move" => Relation::SameMove,
        "returns to" => Relation::ReturnsTo,
        "questions" => Relation::Questions,
        "extends" => Relation::Extends,
        "example of" => Relation::ExampleOf,
        "answers" => Relation::Answers,
        "related" => Relation::Related,
        _ => Relation::Contradicts,
    }
}

fn relation_str(r: Relation) -> &'static str {
    match r {
        Relation::Contradicts => "contradicts",
        Relation::SameMove => "same move",
        Relation::ReturnsTo => "returns to",
        Relation::Questions => "questions",
        Relation::Extends => "extends",
        Relation::ExampleOf => "example of",
        Relation::Answers => "answers",
        Relation::Related => "related",
    }
}

fn status_from(s: &str) -> EdgeStatus {
    match s {
        "accepted" => EdgeStatus::Accepted,
        "dismissed" => EdgeStatus::Dismissed,
        "manual" => EdgeStatus::Manual,
        _ => EdgeStatus::Proposed,
    }
}

fn status_str(s: EdgeStatus) -> &'static str {
    match s {
        EdgeStatus::Proposed => "proposed",
        EdgeStatus::Accepted => "accepted",
        EdgeStatus::Dismissed => "dismissed",
        EdgeStatus::Manual => "manual",
    }
}

fn row_to_edge(row: &Row) -> rusqlite::Result<Edge> {
    let relation: String = row.get(3)?;
    let status: String = row.get(5)?;
    Ok(Edge {
        id: row.get(0)?,
        entry_a: row.get(1)?,
        entry_b: row.get(2)?,
        relation: relation_from(&relation),
        question: row.get(4)?,
        status: status_from(&status),
        created_at: row.get(6)?,
    })
}

/// Dismissed edges are returned too. They are training signal, and the store
/// filters them for display rather than the database forgetting them.
pub fn list(conn: &Connection) -> Result<Vec<Edge>> {
    let mut stmt = conn.prepare(
        "SELECT id, entry_a, entry_b, relation, question, status, created_at
         FROM edges ORDER BY created_at ASC",
    )?;
    let rows = stmt.query_map([], row_to_edge)?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

pub fn list_proposed_for(conn: &Connection, entry_id: &str) -> Result<Vec<Edge>> {
    Ok(list(conn)?
        .into_iter()
        .filter(|e| {
            e.status == EdgeStatus::Proposed && (e.entry_a == entry_id || e.entry_b == entry_id)
        })
        .collect())
}

/// `INSERT OR IGNORE` leans on the unique constraint over
/// (entry_a, entry_b, relation): loading the same corpus twice cannot produce
/// a second copy of an edge, so idempotency is the schema's job.
pub fn insert(conn: &Connection, edge: &Edge) -> Result<()> {
    conn.execute(
        "INSERT OR IGNORE INTO edges
         (id, entry_a, entry_b, relation, question, status, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            edge.id,
            edge.entry_a,
            edge.entry_b,
            relation_str(edge.relation),
            edge.question,
            status_str(edge.status),
            edge.created_at,
        ],
    )?;
    Ok(())
}

pub fn set_status(conn: &Connection, id: &str, status: EdgeStatus) -> Result<()> {
    conn.execute(
        "UPDATE edges SET status = ?2 WHERE id = ?1",
        params![id, status_str(status)],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_in_memory;

    fn seed_entries(conn: &Connection) {
        for id in ["a", "b"] {
            conn.execute(
                "INSERT INTO entries (id, transcript, created_at, x, y, role, register,
                 type_id, resolved, title, duration_ms, unfinished, local_only, is_sample)
                 VALUES (?1, 't', '2024-01-01T00:00:00Z', 0, 0, 'position', 'neutral',
                 'position', 0, 'title', 1000, 0, 0, 0)",
                params![id],
            )
            .unwrap();
        }
    }

    fn edge() -> Edge {
        Edge {
            id: "edge-0".into(),
            entry_a: "a".into(),
            entry_b: "b".into(),
            relation: Relation::SameMove,
            question: Some("What is shared here?".into()),
            status: EdgeStatus::Proposed,
            created_at: "2024-01-02T00:00:00Z".into(),
        }
    }

    #[test]
    fn a_relation_keeps_its_spaces_through_sql() {
        let conn = open_in_memory().unwrap();
        seed_entries(&conn);
        insert(&conn, &edge()).unwrap();
        assert_eq!(list(&conn).unwrap()[0].relation, Relation::SameMove);
    }

    /// The bug this constraint exists for: loading a corpus twice used to
    /// produce two edge-0s, and a repeated React key strands the element.
    #[test]
    fn loading_the_same_edge_twice_is_a_no_op() {
        let conn = open_in_memory().unwrap();
        seed_entries(&conn);
        insert(&conn, &edge()).unwrap();

        let mut duplicate = edge();
        duplicate.id = "edge-99".into();
        insert(&conn, &duplicate).unwrap();

        assert_eq!(list(&conn).unwrap().len(), 1);
    }

    #[test]
    fn deleting_an_entry_takes_its_edges() {
        let conn = open_in_memory().unwrap();
        seed_entries(&conn);
        insert(&conn, &edge()).unwrap();
        conn.execute("DELETE FROM entries WHERE id = 'a'", []).unwrap();
        assert!(list(&conn).unwrap().is_empty());
    }
}
