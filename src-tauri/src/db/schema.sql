-- Storage shape, not wire shape. The wire types in `model/` are a projection
-- over these tables; normalising here and flattening at the boundary is the
-- point, not a mismatch.

PRAGMA foreign_keys = ON;

-- Positions are frozen at insert and never recomputed (§5.1), so `created_at`
-- ordering is load-bearing: it is the order placement solved the field in.
CREATE TABLE entries (
    id                  TEXT PRIMARY KEY,
    transcript          TEXT    NOT NULL,
    created_at          TEXT    NOT NULL,
    x                   REAL    NOT NULL,
    y                   REAL    NOT NULL,

    -- Set only when this entry was recorded as an answer. Manual and proposed
    -- connections are edges and have no parent.
    answers_entry_id    TEXT    REFERENCES entries(id) ON DELETE SET NULL,
    answers_question_id TEXT,

    role                TEXT    NOT NULL,
    register            TEXT    NOT NULL,
    -- Deliberately no FK: types are user data, and deleting one must not
    -- cascade into the entries that carry it. Orphans fall back to the role.
    type_id             TEXT    NOT NULL,

    resolved            INTEGER NOT NULL DEFAULT 0,
    resolution_text     TEXT,

    title               TEXT    NOT NULL,
    -- NULL when register is 'live'.
    summary             TEXT,
    duration_ms         INTEGER NOT NULL,

    unfinished          INTEGER NOT NULL DEFAULT 0,
    local_only          INTEGER NOT NULL DEFAULT 0,
    is_sample           INTEGER NOT NULL DEFAULT 0,

    -- What the note does as a move, with its subject removed, so two notes
    -- about different things can be seen making the same one (§7.1). Tags
    -- find topical pairs; this is the signal they structurally cannot find.
    move_phrase         TEXT,

    -- The audio is the record; the transcript is a derivation of it and may be
    -- corrected toward accuracy. Nothing is versioned -- the recording is
    -- already the ground truth to check against.
    corrected_at        TEXT
);

-- A typed entry has no row here at all, which is the distinction the design
-- keeps drawing, expressed structurally rather than as a nullable column.
-- `codec` lets a corpus hold wav and opus side by side with no migration.
CREATE TABLE audio (
    entry_id    TEXT PRIMARY KEY REFERENCES entries(id) ON DELETE CASCADE,
    path        TEXT    NOT NULL,
    codec       TEXT    NOT NULL,
    sample_rate INTEGER NOT NULL,
    byte_size   INTEGER NOT NULL,
    -- 7-9 floats, read only ever as a unit.
    fingerprint TEXT    NOT NULL
);

-- `quoted_text` is the anchor, not the offsets. Correcting a transcript shifts
-- every offset after the edit, and a drifted `attributed` span would silently
-- put a probe on someone else's words, so a span is re-found by its text and
-- marked stale when the text is genuinely gone.
CREATE TABLE spans (
    id           INTEGER PRIMARY KEY,
    entry_id     TEXT    NOT NULL REFERENCES entries(id) ON DELETE CASCADE,
    start_offset INTEGER NOT NULL,
    end_offset   INTEGER NOT NULL,
    quoted_text  TEXT    NOT NULL DEFAULT '',
    stale        INTEGER NOT NULL DEFAULT 0,
    -- true = someone else's words. Only an own span may be pushed on (§7.3).
    attributed   INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE action_items (
    id              TEXT PRIMARY KEY,
    entry_id        TEXT    NOT NULL REFERENCES entries(id) ON DELETE CASCADE,
    span_start      INTEGER NOT NULL,
    span_end        INTEGER NOT NULL,
    span_attributed INTEGER NOT NULL DEFAULT 0,
    span_quoted     TEXT    NOT NULL DEFAULT '',
    stale           INTEGER NOT NULL DEFAULT 0,
    text            TEXT    NOT NULL,
    done            INTEGER NOT NULL DEFAULT 0
);

-- Questions accumulate and are never replaced; a bad one is dismissed, not
-- regenerated, and dismissals are kept because they are training signal.
CREATE TABLE questions (
    id              TEXT PRIMARY KEY,
    entry_id        TEXT    NOT NULL REFERENCES entries(id) ON DELETE CASCADE,
    text            TEXT    NOT NULL,
    span_start      INTEGER,
    span_end        INTEGER,
    span_attributed INTEGER,
    span_quoted     TEXT,
    answered        INTEGER NOT NULL DEFAULT 0,
    dismissed       INTEGER NOT NULL DEFAULT 0,
    provider_name   TEXT    NOT NULL,
    created_at      TEXT    NOT NULL
);

-- The UNIQUE constraint makes the duplicate-edge bug structurally impossible
-- rather than something the sample loader has to remember not to cause.
CREATE TABLE edges (
    id         TEXT PRIMARY KEY,
    entry_a    TEXT NOT NULL REFERENCES entries(id) ON DELETE CASCADE,
    entry_b    TEXT NOT NULL REFERENCES entries(id) ON DELETE CASCADE,
    relation   TEXT NOT NULL,
    question   TEXT,
    status     TEXT NOT NULL,
    created_at TEXT NOT NULL,
    UNIQUE (entry_a, entry_b, relation)
);

-- User-defined types (§3.6). `match_text` is the sentence the classifier
-- matches against, and becomes an enum member in the enrichment call's schema.
CREATE TABLE types (
    id         TEXT PRIMARY KEY,
    label      TEXT    NOT NULL,
    match_text TEXT    NOT NULL,
    prompt     TEXT,
    tier       TEXT    NOT NULL,
    role       TEXT,
    mark_kind  TEXT    NOT NULL,
    mark_value TEXT,
    built_in   INTEGER NOT NULL DEFAULT 0,
    created_at TEXT    NOT NULL
);

-- Tags are internal machinery, never a surface (the user judges proposed
-- connections, not the vocabulary underneath). `name` is stored normalised,
-- and is not the primary key: normalisation may change, and a renamed tag
-- must not orphan every note filed under it.
CREATE TABLE tags (
    id         TEXT PRIMARY KEY,
    name       TEXT NOT NULL UNIQUE,
    created_at TEXT NOT NULL
);

-- The inverted index. A note carries several tags and a tag carries several
-- notes, so finding candidates reads only the rows for this note's handful of
-- tags -- which is what makes proposal cost independent of corpus size.
CREATE TABLE entry_tags (
    entry_id TEXT NOT NULL REFERENCES entries(id) ON DELETE CASCADE,
    tag_id   TEXT NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
    PRIMARY KEY (entry_id, tag_id)
);

CREATE TABLE settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

CREATE INDEX idx_entries_created   ON entries(created_at);
CREATE INDEX idx_entries_answers   ON entries(answers_entry_id);
CREATE INDEX idx_spans_entry       ON spans(entry_id);
CREATE INDEX idx_actions_entry     ON action_items(entry_id);
CREATE INDEX idx_questions_entry   ON questions(entry_id);
CREATE INDEX idx_edges_a           ON edges(entry_a);
CREATE INDEX idx_edges_b           ON edges(entry_b);
-- By tag, not by entry: the lookup asks "who else carries this tag".
CREATE INDEX idx_entry_tags_tag    ON entry_tags(tag_id);
