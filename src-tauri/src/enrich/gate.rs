//! What may be asked about an entry, and what may not.
//!
//! Re-enforced here rather than trusted from the UI. The frontend computes the
//! same rules to decide whether to draw a button; this decides whether a
//! question exists. §3.2's asymmetry is the whole argument: a missed question
//! costs nothing, a heavy probe on an entry about someone's grief is
//! unrecoverable.
//!
//! Port of the gates in `lib/scene/classification.ts`.

use crate::model::{Entry, Register, Role};

/// §3.2 -- under this, nothing fires on its own.
pub const MIN_AUTOMATIC_MS: i64 = 30_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Probe {
    /// Safe tier.
    Boundary,
    Disconfirming,
    /// Heavy tier -- invoked only, except Feynman, which takes no stance.
    Steelman,
    Munchhausen,
    Feynman,
}

impl Probe {
    pub fn hint(&self) -> &'static str {
        match self {
            Probe::Boundary => "where does this stop holding?",
            Probe::Disconfirming => "what would make you drop this?",
            Probe::Steelman => "state it better than the note did, then push",
            Probe::Munchhausen => "follow the reasons until they bottom out",
            Probe::Feynman => "apply it to a case it has not been given",
        }
    }
}

/// §7.3 -- false when the entry is wholly someone else's words. Attributed
/// spans may be quoted and connected; they may not be pushed on.
pub fn has_own_span(entry: &Entry) -> bool {
    // Nothing borrowed means nothing borrowed, whatever the transcript is.
    // Without this an empty transcript reads as wholly attributed, and an
    // entry recorded before a transcription model was installed is empty.
    if !entry.spans.iter().any(|s| s.attributed) {
        return true;
    }

    let covered: u32 = entry
        .spans
        .iter()
        .filter(|s| s.attributed)
        .map(|s| s.end.saturating_sub(s.start))
        .sum();
    covered < entry.transcript.encode_utf16().count() as u32
}

/// What may fire without being asked for.
///
/// The three checks come first and fail closed. Only then does role decide,
/// and role may only ever *narrow* what is offered -- classification suppresses
/// and never selects, so a misclassification costs a missing question rather
/// than an intrusive one.
pub fn automatic_probes(entry: &Entry) -> Vec<Probe> {
    if entry.register == Register::Live
        || entry.duration_ms < MIN_AUTOMATIC_MS
        || !has_own_span(entry)
    {
        return Vec::new();
    }

    match entry.role {
        Role::Position => vec![Probe::Boundary, Probe::Disconfirming],
        // Feynman takes no stance and cannot misfire the way a steelman can.
        Role::Evidence => vec![Probe::Feynman],
        Role::Note => Vec::new(),
    }
}

/// What may fire when the user selects a passage and asks. Register does not
/// gate here: §3.2 gives the invoked path to the user, so the risk is theirs.
pub fn invoked_probes(entry: &Entry) -> Vec<Probe> {
    if !has_own_span(entry) {
        return Vec::new();
    }

    match entry.role {
        Role::Position => vec![
            Probe::Boundary,
            Probe::Disconfirming,
            Probe::Steelman,
            Probe::Munchhausen,
            Probe::Feynman,
        ],
        Role::Evidence => vec![Probe::Feynman],
        Role::Note => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Span;

    fn entry(role: Role, register: Register, duration_ms: i64) -> Entry {
        Entry {
            id: "e1".into(),
            audio_path: None,
            transcript: "Indexes trade write performance for faster reads.".into(),
            created_at: "2024-01-01T00:00:00Z".into(),
            x: 0.0,
            y: 0.0,
            parent_entry_id: None,
            answers_question_id: None,
            role,
            register,
            type_id: "position".into(),
            resolved: false,
            resolution_text: None,
            title: "t".into(),
            summary: None,
            duration_ms,
            fingerprint: vec![],
            unfinished: false,
            local_only: false,
            spans: vec![],
            action_items: vec![],
            is_sample: None,
        }
    }

    /// The three unconditional suppressions. Each of these on its own is
    /// enough to keep the app quiet.
    #[test]
    fn a_live_entry_is_never_asked_about_unprompted() {
        let e = entry(Role::Position, Register::Live, 120_000);
        assert!(automatic_probes(&e).is_empty());
    }

    #[test]
    fn something_said_in_under_thirty_seconds_is_left_alone() {
        let e = entry(Role::Position, Register::Neutral, MIN_AUTOMATIC_MS - 1);
        assert!(automatic_probes(&e).is_empty());
    }

    #[test]
    fn an_entry_that_is_wholly_someone_elses_words_is_left_alone() {
        let mut e = entry(Role::Position, Register::Neutral, 120_000);
        e.spans = vec![Span {
            start: 0,
            end: e.transcript.len() as u32,
            attributed: true,
        }];
        assert!(!has_own_span(&e));
        assert!(automatic_probes(&e).is_empty());
    }

    #[test]
    fn a_note_quoting_someone_still_has_its_own_words() {
        let mut e = entry(Role::Position, Register::Neutral, 120_000);
        e.spans = vec![Span {
            start: 0,
            end: 10,
            attributed: true,
        }];
        assert!(has_own_span(&e), "only part of it is borrowed");
        assert!(!automatic_probes(&e).is_empty());
    }

    /// Span offsets are UTF-16 units, so the coverage check has to be too.
    /// Against a byte length a wholly-quoted non-ASCII entry reads as having
    /// words of its own, and the app pushes on someone else's grief.
    #[test]
    fn a_wholly_attributed_non_ascii_entry_has_no_own_span() {
        let mut e = entry(Role::Position, Register::Neutral, 120_000);
        e.transcript = "«Наблюдаемость важнее логов», — сказал он.".into();
        e.spans = vec![Span {
            start: 0,
            end: e.transcript.encode_utf16().count() as u32,
            attributed: true,
        }];
        assert!(!has_own_span(&e));
        assert!(automatic_probes(&e).is_empty());
    }

    /// §3.2 -- a position opens with safe probes only. A steelman on an entry
    /// nobody asked about is the failure the tiers exist to prevent.
    #[test]
    fn a_position_opens_with_safe_probes_and_never_a_steelman() {
        let probes = automatic_probes(&entry(Role::Position, Register::Neutral, 120_000));

        assert!(probes.contains(&Probe::Boundary));
        assert!(probes.contains(&Probe::Disconfirming));
        assert!(!probes.contains(&Probe::Steelman));
        assert!(!probes.contains(&Probe::Munchhausen));
    }

    /// A deliberate departure from "never F": being asked to say something
    /// back takes no stance and cannot wound, and withholding it until someone
    /// thinks to ask means it only ever fires for people who already know to
    /// want it.
    #[test]
    fn evidence_opens_with_feynman() {
        let probes = automatic_probes(&entry(Role::Evidence, Register::Neutral, 120_000));
        assert_eq!(probes, vec![Probe::Feynman]);
    }

    #[test]
    fn a_note_is_silent() {
        assert!(automatic_probes(&entry(Role::Note, Register::Neutral, 120_000)).is_empty());
    }

    /// The invoked path is the user's own risk, so register does not gate it.
    #[test]
    fn a_live_entry_can_still_be_asked_about_when_invited() {
        let e = entry(Role::Position, Register::Live, 120_000);
        assert!(automatic_probes(&e).is_empty());
        assert!(!invoked_probes(&e).is_empty(), "asking is the user's call");
    }

    /// Duration gates the automatic path only. A short note you select a
    /// sentence in is still fair game.
    #[test]
    fn a_short_entry_can_still_be_asked_about_when_invited() {
        let e = entry(Role::Position, Register::Neutral, 5_000);
        assert!(automatic_probes(&e).is_empty());
        assert!(!invoked_probes(&e).is_empty());
    }

    /// The heavy probes exist, and only here.
    #[test]
    fn the_heavy_probes_are_reachable_only_on_request() {
        let probes = invoked_probes(&entry(Role::Position, Register::Neutral, 120_000));
        assert!(probes.contains(&Probe::Steelman));
        assert!(probes.contains(&Probe::Munchhausen));
    }

    /// Feynman needs only "not a note" -- it is the one move that makes the
    /// person do the thinking, and gating it behind `position` made it the
    /// least reachable thing in the app.
    #[test]
    fn evidence_can_be_asked_to_explain_itself_but_not_steelmanned() {
        let probes = invoked_probes(&entry(Role::Evidence, Register::Neutral, 120_000));
        assert!(probes.contains(&Probe::Feynman));
        assert!(!probes.contains(&Probe::Steelman));
    }

    #[test]
    fn a_note_reaches_nothing_even_when_invited() {
        assert!(invoked_probes(&entry(Role::Note, Register::Neutral, 120_000)).is_empty());
    }

    /// Provenance gates both paths, unlike register and duration.
    #[test]
    fn borrowed_words_cannot_be_pushed_on_even_when_invited() {
        let mut e = entry(Role::Position, Register::Neutral, 120_000);
        e.spans = vec![Span {
            start: 0,
            end: e.transcript.len() as u32,
            attributed: true,
        }];
        assert!(invoked_probes(&e).is_empty());
    }

    /// Reachable: an entry recorded before a transcription model is installed
    /// has an empty transcript. Nothing borrowed means nothing borrowed, and
    /// treating it as wholly someone else's words silenced it for good.
    #[test]
    fn an_empty_transcript_with_no_borrowed_spans_is_still_your_own() {
        let mut e = entry(Role::Position, Register::Neutral, 120_000);
        e.transcript = String::new();
        assert!(has_own_span(&e));
    }

    /// An attributed span longer than the transcript must not underflow into
    /// a huge number and read as fully covered by accident.
    #[test]
    fn a_span_beyond_the_transcript_does_not_underflow() {
        let mut e = entry(Role::Position, Register::Neutral, 120_000);
        e.spans = vec![Span {
            start: 900,
            end: 5,
            attributed: true,
        }];
        assert!(has_own_span(&e), "an impossible span covers nothing");
    }
}
