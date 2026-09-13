//! User-defined note types (§3.6), persisted.
//!
//! Built-ins live in this table too, seeded on every open rather than kept as
//! a hardcoded fallback: the classifier's enum (`entries::type_ids`) and the
//! gate's tier lookup both read one table instead of a table plus a constant
//! that has to be kept in sync with it.

use crate::error::{Error, Result};
use crate::model::{Mark, NewType, ProbeTier, Role, TypeDef, TypePatch};
use rusqlite::{params, Connection, OptionalExtension};

/// The three the app ships with. Text kept byte-identical to
/// `BUILT_IN_TYPES` in `lib/scene/classification.ts` — that copy is the
/// classifier prompt's source of truth for what these mean, and a drifted
/// pair would describe one type two ways.
const BUILT_INS: &[(&str, &str, &str, ProbeTier, Role)] = &[
    (
        "position",
        "position",
        "your own reasoning, asserted with grounds",
        ProbeTier::Safe,
        Role::Position,
    ),
    (
        "evidence",
        "evidence",
        "a fact, a number, a thing you noticed, or something you are learning",
        ProbeTier::Silent,
        Role::Evidence,
    ),
    (
        "note",
        "note",
        "admin, lists, intents, reminders",
        ProbeTier::Silent,
        Role::Note,
    ),
];

pub fn is_built_in_id(id: &str) -> bool {
    BUILT_INS.iter().any(|(bid, ..)| *bid == id)
}

/// Idempotent: run on every `open`, so a database that predates this table —
/// or one that has simply never had a type created in it — still has
/// something for the classifier to offer and the gate to look up.
pub fn ensure_built_ins(conn: &Connection) -> Result<()> {
    for (id, label, match_text, tier, role) in BUILT_INS {
        conn.execute(
            "INSERT OR IGNORE INTO types
             (id, label, match_text, prompt, tier, role, mark_kind, mark_value, built_in, created_at)
             VALUES (?1, ?2, ?3, NULL, ?4, ?5, 'none', NULL, 1, ?6)",
            params![
                id,
                label,
                match_text,
                tier.as_str(),
                role_str(*role),
                chrono::Utc::now().to_rfc3339()
            ],
        )?;
    }
    Ok(())
}

fn role_str(role: Role) -> &'static str {
    match role {
        Role::Position => "position",
        Role::Evidence => "evidence",
        Role::Note => "note",
    }
}

fn role_from_str(s: &str) -> Option<Role> {
    match s {
        "position" => Some(Role::Position),
        "evidence" => Some(Role::Evidence),
        "note" => Some(Role::Note),
        _ => None,
    }
}

fn mark_columns(mark: &Option<Mark>) -> (&'static str, Option<String>) {
    match mark {
        None => ("none", None),
        Some(Mark::Glyph { id }) => ("glyph", Some(role_str(*id).to_string())),
        Some(Mark::Char { char }) => ("char", Some(char.clone())),
    }
}

fn mark_from_columns(kind: &str, value: Option<String>) -> Option<Mark> {
    match kind {
        "glyph" => role_from_str(value?.as_str()).map(|id| Mark::Glyph { id }),
        "char" => value.map(|char| Mark::Char { char }),
        _ => None,
    }
}

fn row_to_type(row: &rusqlite::Row) -> rusqlite::Result<TypeDef> {
    let tier_str: String = row.get(4)?;
    let role_str: Option<String> = row.get(5)?;
    let mark_kind: String = row.get(6)?;
    let mark_value: Option<String> = row.get(7)?;
    let built_in: bool = row.get(8)?;
    Ok(TypeDef {
        id: row.get(0)?,
        label: row.get(1)?,
        match_text: row.get(2)?,
        prompt: row.get(3)?,
        tier: ProbeTier::from_str(&tier_str).unwrap_or(ProbeTier::Heavy),
        role: role_str.and_then(|r| role_from_str(&r)),
        mark: mark_from_columns(&mark_kind, mark_value),
        built_in,
        auto_approved: true,
    })
}

const SELECT_COLUMNS: &str =
    "id, label, match_text, prompt, tier, role, mark_kind, mark_value, built_in";

/// Built-ins first, then custom in the order they were made — the order the
/// editor and the classifier enum both show them in.
pub fn list(conn: &Connection) -> Result<Vec<TypeDef>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {SELECT_COLUMNS} FROM types ORDER BY built_in DESC, created_at ASC"
    ))?;
    let rows = stmt.query_map([], row_to_type)?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

pub fn get(conn: &Connection, id: &str) -> Result<Option<TypeDef>> {
    conn.query_row(
        &format!("SELECT {SELECT_COLUMNS} FROM types WHERE id = ?1"),
        params![id],
        row_to_type,
    )
    .optional()
    .map_err(Error::from)
}

/// A slug the editor already shows before submitting: lowercase, hyphenated,
/// non-empty. Enforced here too — the id arrives over IPC, and a caller that
/// is not the shipped UI is under none of the frontend's validation.
fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        && !id.starts_with('-')
        && !id.ends_with('-')
}

/// §3.6 rule 1's teeth, moved to where it can't be skipped: a custom type
/// reaching the gate with `retrieval` would auto-fire like a built-in that
/// pairs notes up, which the tier picker in the editor never even offers as a
/// choice.
fn valid_custom_tier(tier: ProbeTier) -> bool {
    !matches!(tier, ProbeTier::Retrieval)
}

fn validate_shape(id: &str, label: &str, tier: ProbeTier) -> Result<()> {
    if !valid_id(id) {
        return Err(Error::Other(format!("{id:?} is not a valid type id")));
    }
    if is_built_in_id(id) {
        return Err(Error::Other(format!("{id} is a built-in type id")));
    }
    if label.trim().is_empty() {
        return Err(Error::Other("a type needs a label".into()));
    }
    if !valid_custom_tier(tier) {
        return Err(Error::Other(
            "a user-defined type may not claim the retrieval tier".into(),
        ));
    }
    Ok(())
}

pub fn create(conn: &Connection, draft: NewType) -> Result<TypeDef> {
    validate_shape(&draft.id, &draft.label, draft.tier)?;
    let (mark_kind, mark_value) = mark_columns(&draft.mark);
    let inserted = conn.execute(
        "INSERT OR IGNORE INTO types
         (id, label, match_text, prompt, tier, role, mark_kind, mark_value, built_in, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 0, ?9)",
        params![
            draft.id,
            draft.label,
            draft.match_text,
            draft.prompt,
            draft.tier.as_str(),
            draft.role.map(role_str),
            mark_kind,
            mark_value,
            chrono::Utc::now().to_rfc3339()
        ],
    )?;
    if inserted == 0 {
        return Err(Error::Other(format!("a type called {} already exists", draft.id)));
    }
    get(conn, &draft.id)?.ok_or_else(|| Error::NotFound(draft.id))
}

pub fn update(conn: &Connection, id: &str, patch: TypePatch) -> Result<TypeDef> {
    if is_built_in_id(id) {
        return Err(Error::Other(format!("{id} is a built-in type and cannot be edited")));
    }
    validate_shape(id, &patch.label, patch.tier)?;
    let (mark_kind, mark_value) = mark_columns(&patch.mark);
    let changed = conn.execute(
        "UPDATE types SET label = ?2, match_text = ?3, prompt = ?4, tier = ?5, role = ?6,
         mark_kind = ?7, mark_value = ?8 WHERE id = ?1 AND built_in = 0",
        params![
            id,
            patch.label,
            patch.match_text,
            patch.prompt,
            patch.tier.as_str(),
            patch.role.map(role_str),
            mark_kind,
            mark_value,
        ],
    )?;
    if changed == 0 {
        return Err(Error::NotFound(format!("no user type {id}")));
    }
    get(conn, id)?.ok_or_else(|| Error::NotFound(id.to_string()))
}

/// Deletes a user type. Notes carrying it fall back to their own role's
/// built-in id, in the same transaction — a note is never left pointing at a
/// type row that no longer exists.
pub fn delete(conn: &mut Connection, id: &str) -> Result<()> {
    if is_built_in_id(id) {
        return Err(Error::Other(format!("{id} is a built-in type and cannot be deleted")));
    }
    let tx = conn.transaction()?;
    // The built-in ids are exactly the role names, so an orphaned entry's own
    // `role` column is already the fallback id — no lookup table needed.
    tx.execute(
        "UPDATE entries SET type_id = role WHERE type_id = ?1",
        params![id],
    )?;
    let changed = tx.execute("DELETE FROM types WHERE id = ?1 AND built_in = 0", params![id])?;
    if changed == 0 {
        tx.rollback()?;
        return Err(Error::NotFound(format!("no user type {id}")));
    }
    tx.commit()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_in_memory;

    fn draft(id: &str) -> NewType {
        NewType {
            id: id.into(),
            label: id.into(),
            match_text: "when the speaker is wondering aloud".into(),
            prompt: None,
            tier: ProbeTier::Heavy,
            role: Some(Role::Position),
            mark: Some(Mark::Char { char: "†".into() }),
        }
    }

    #[test]
    fn a_fresh_database_already_carries_the_three_built_ins() {
        let conn = open_in_memory().unwrap();
        let types = list(&conn).unwrap();
        let ids: Vec<&str> = types.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids, vec!["position", "evidence", "note"]);
        assert!(types.iter().all(|t| t.built_in));
    }

    #[test]
    fn seeding_twice_does_not_duplicate() {
        let conn = open_in_memory().unwrap();
        ensure_built_ins(&conn).unwrap();
        ensure_built_ins(&conn).unwrap();
        assert_eq!(list(&conn).unwrap().len(), 3);
    }

    #[test]
    fn a_custom_type_is_created_and_listed_after_the_built_ins() {
        let conn = open_in_memory().unwrap();
        let created = create(&conn, draft("wondering")).unwrap();
        assert_eq!(created.id, "wondering");
        assert!(!created.built_in);
        assert!(created.auto_approved, "§3.6 rule 1 -- always true once stored");

        let types = list(&conn).unwrap();
        assert_eq!(types.last().unwrap().id, "wondering");
    }

    #[test]
    fn a_type_id_colliding_with_a_built_in_is_refused() {
        let conn = open_in_memory().unwrap();
        assert!(create(&conn, draft("position")).is_err());
    }

    #[test]
    fn a_duplicate_custom_id_is_refused() {
        let conn = open_in_memory().unwrap();
        create(&conn, draft("wondering")).unwrap();
        assert!(create(&conn, draft("wondering")).is_err());
    }

    #[test]
    fn an_id_with_spaces_or_uppercase_is_refused() {
        let conn = open_in_memory().unwrap();
        let mut bad = draft("Not A Slug");
        bad.id = "Not A Slug".into();
        assert!(create(&conn, bad).is_err());
    }

    #[test]
    fn a_blank_label_is_refused() {
        let conn = open_in_memory().unwrap();
        let mut bad = draft("wondering");
        bad.label = "   ".into();
        assert!(create(&conn, bad).is_err());
    }

    /// §3.6 rule 1's teeth: retrieval auto-fires like a built-in that pairs
    /// notes up, and the editor never offers it as a choice to begin with.
    #[test]
    fn a_custom_type_may_not_claim_the_retrieval_tier() {
        let conn = open_in_memory().unwrap();
        let mut bad = draft("wondering");
        bad.tier = ProbeTier::Retrieval;
        assert!(create(&conn, bad).is_err());
    }

    #[test]
    fn updating_a_custom_type_changes_it_in_place() {
        let conn = open_in_memory().unwrap();
        create(&conn, draft("wondering")).unwrap();
        let patch = TypePatch {
            label: "wondering aloud".into(),
            match_text: "musing without a claim yet".into(),
            prompt: Some("ask what would settle it".into()),
            tier: ProbeTier::Safe,
            role: None,
            mark: None,
        };
        let updated = update(&conn, "wondering", patch).unwrap();
        assert_eq!(updated.label, "wondering aloud");
        assert_eq!(updated.tier, ProbeTier::Safe);
        assert_eq!(updated.role, None);
        assert_eq!(updated.mark, None);
    }

    #[test]
    fn a_built_in_cannot_be_updated_or_deleted() {
        let mut conn = open_in_memory().unwrap();
        let patch = TypePatch {
            label: "renamed".into(),
            match_text: "x".into(),
            prompt: None,
            tier: ProbeTier::Safe,
            role: None,
            mark: None,
        };
        assert!(update(&conn, "position", patch).is_err());
        assert!(delete(&mut conn, "position").is_err());
    }

    #[test]
    fn deleting_an_unknown_type_is_an_error() {
        let mut conn = open_in_memory().unwrap();
        assert!(delete(&mut conn, "no-such-type").is_err());
    }

    /// The rule from §3.6: a note never loses its type entirely, it falls back
    /// to whatever its own role already is -- and the built-in ids are the
    /// role names, so that fallback needs no lookup that could itself be wrong.
    #[test]
    fn deleting_a_type_falls_every_note_carrying_it_back_to_its_own_role() {
        let mut conn = open_in_memory().unwrap();
        create(&conn, draft("wondering")).unwrap();
        conn.execute(
            "INSERT INTO entries (id, transcript, created_at, x, y, role, register,
             type_id, resolved, title, duration_ms, unfinished, local_only, is_sample)
             VALUES ('e1', 'said', '2024-01-01T00:00:00Z', 0, 0, 'position', 'neutral',
             'wondering', 0, 't', 40000, 0, 0, 0)",
            [],
        )
        .unwrap();

        delete(&mut conn, "wondering").unwrap();

        let type_id: String = conn
            .query_row("SELECT type_id FROM entries WHERE id = 'e1'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(type_id, "position", "fell back to the entry's own role");
        assert!(get(&conn, "wondering").unwrap().is_none());
    }

    /// The transaction is one write, not two: the note must never observe a
    /// state where the type is gone but its type_id has not moved yet.
    #[test]
    fn the_fallback_and_the_delete_happen_together() {
        let mut conn = open_in_memory().unwrap();
        create(&conn, draft("wondering")).unwrap();
        conn.execute(
            "INSERT INTO entries (id, transcript, created_at, x, y, role, register,
             type_id, resolved, title, duration_ms, unfinished, local_only, is_sample)
             VALUES ('e1', 'said', '2024-01-01T00:00:00Z', 0, 0, 'evidence', 'neutral',
             'wondering', 0, 't', 40000, 0, 0, 0)",
            [],
        )
        .unwrap();
        delete(&mut conn, "wondering").unwrap();
        let type_id: String = conn
            .query_row("SELECT type_id FROM entries WHERE id = 'e1'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(type_id, "evidence");
    }

    #[test]
    fn a_glyph_mark_round_trips_through_storage() {
        let conn = open_in_memory().unwrap();
        let mut d = draft("wondering");
        d.mark = Some(Mark::Glyph { id: Role::Evidence });
        let created = create(&conn, d).unwrap();
        assert_eq!(created.mark, Some(Mark::Glyph { id: Role::Evidence }));
        let fetched = get(&conn, "wondering").unwrap().unwrap();
        assert_eq!(fetched.mark, Some(Mark::Glyph { id: Role::Evidence }));
    }

    #[test]
    fn no_mark_round_trips_as_none() {
        let conn = open_in_memory().unwrap();
        let mut d = draft("wondering");
        d.mark = None;
        let created = create(&conn, d).unwrap();
        assert_eq!(created.mark, None);
    }
}
