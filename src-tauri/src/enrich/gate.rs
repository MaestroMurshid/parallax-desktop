//! What may be asked about an entry, and what may not.
//!
//! Re-enforced here rather than trusted from the UI. The frontend computes the
//! same rules to decide whether to draw a button; this decides whether a
//! question exists. §3.2's asymmetry is the whole argument: a missed question
//! costs nothing, a heavy probe on an entry about someone's grief is
//! unrecoverable.
//!
//! Port of the gates in `lib/scene/classification.ts`.

use crate::model::{Entry, ProbeTier, Register, Role};

/// §3.2 -- under this, nothing fires on its own.
pub const MIN_AUTOMATIC_MS: i64 = 10_000;

/// A debate tactic: one way of pushing on what a note claims.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Probe {
    Boundary,
    Disconfirming,
    Assumption,
    Counterexample,
    Definition,
    Consequence,
    Fallacy,
    Steelman,
    Munchhausen,
    /// The one move that takes no stance, and so the one evidence gets.
    Feynman,
}

impl Probe {
    pub const ALL: [Probe; 10] = [
        Probe::Boundary,
        Probe::Disconfirming,
        Probe::Assumption,
        Probe::Counterexample,
        Probe::Definition,
        Probe::Consequence,
        Probe::Fallacy,
        Probe::Steelman,
        Probe::Munchhausen,
        Probe::Feynman,
    ];

    /// The wire name, matching the ids in `lib/scene/classification.ts`.
    /// `run_probe` is invoked with whatever the frontend calls a probe, so the
    /// two spellings have to be the same one.
    pub fn id(&self) -> &'static str {
        match self {
            Probe::Boundary => "boundary",
            Probe::Disconfirming => "disconfirming",
            Probe::Assumption => "assumption",
            Probe::Counterexample => "counterexample",
            Probe::Definition => "definition",
            Probe::Consequence => "consequence",
            Probe::Fallacy => "fallacy",
            Probe::Steelman => "steelman",
            Probe::Munchhausen => "munchhausen",
            Probe::Feynman => "feynman",
        }
    }

    /// `None` for anything else. A probe id arrives over IPC, so it is parsed
    /// rather than trusted -- an unknown one must not fall through to a probe
    /// the gate would have refused.
    pub fn from_id(id: &str) -> Option<Probe> {
        Probe::ALL.into_iter().find(|probe| probe.id() == id)
    }

    /// The move, as the model is told it. Described as a move and never
    /// phrased as a question: phrased as one, the model handed the description
    /// back as its question -- "What does this take for granted without saying
    /// so?", word for word.
    pub fn hint(&self) -> &'static str {
        match self {
            Probe::Boundary => "find the case or condition where the claim stops holding",
            Probe::Disconfirming => {
                "ask what evidence or experience would make them give the claim up"
            }
            Probe::Assumption => "expose an unstated premise the claim depends on",
            Probe::Counterexample => {
                "confront the claim with a specific, concrete case that cuts against it"
            }
            Probe::Definition => "press on one key word whose meaning the claim depends on",
            Probe::Consequence => {
                "draw out something else that must be true if the claim is, and test it"
            }
            Probe::Fallacy => {
                "name a specific reasoning error the note actually makes, quoting where it makes it"
            }
            Probe::Steelman => {
                "state the strongest opposing view and ask how the claim survives it"
            }
            Probe::Munchhausen => "ask for the reason behind the reason the note gives",
            Probe::Feynman => "ask them to apply the idea to a new case it was not stated for",
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
///
/// A position is offered every tactic, and the model picks the one that fits
/// the note. §3.2 had kept the opening move to boundary or disconfirming, and
/// in use that made every question the same question; decided 13 Sep 2026 that
/// which move to make is the model's call. Whether to ask at all is still not.
/// `live_register` is the app-wide setting. When it is off the facet is not
/// consulted at all, so a note the classifier called live is probed like any
/// other -- the stored value is left alone rather than rewritten, so turning
/// the setting back on restores the old behaviour instead of having quietly
/// destroyed what the model decided.
///
/// `tier` is the entry's *custom* type's tier -- `None` for a built-in or a
/// type that no longer exists, since a built-in's own `tier` column is
/// decorative (`db::types::tier_for`). §3.6: a type may only ever narrow what
/// role/register/duration/provenance already allow, never widen it -- silent
/// and heavy both close the automatic path, safe leaves it exactly as role
/// already decided.
pub fn automatic_probes(entry: &Entry, live_register: bool, tier: Option<ProbeTier>) -> Vec<Probe> {
    if (live_register && entry.register == Register::Live)
        || entry.duration_ms < MIN_AUTOMATIC_MS
        || !has_own_span(entry)
        || matches!(tier, Some(ProbeTier::Silent) | Some(ProbeTier::Heavy))
    {
        return Vec::new();
    }

    match entry.role {
        Role::Position => Probe::ALL.to_vec(),
        // Evidence is held, not argued, so it is only asked to explain itself.
        Role::Evidence => vec![Probe::Feynman],
        Role::Note => Vec::new(),
    }
}

/// How many moves one question chooses between. Measured on the real model:
/// offered all ten, it took `assumption` 26 of 26 times; offered three drawn at
/// random, it used nine different moves across 26 questions.
pub const OFFERED: usize = 3;

/// The moves one question is offered: `OFFERED` of `candidates`, drawn by
/// `seed`. Seeded rather than random inside, so a test can pin a draw; callers
/// pass a fresh UUID.
pub fn offer(candidates: &[Probe], seed: u128) -> Vec<Probe> {
    // splitmix64 over the seed: enough to shuffle ten items fairly, and no
    // dependency for it.
    let mut state = (seed as u64) ^ ((seed >> 64) as u64);
    let mut next = || {
        state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    };
    let mut pool = candidates.to_vec();
    for i in (1..pool.len()).rev() {
        let j = (next() % (i as u64 + 1)) as usize;
        pool.swap(i, j);
    }
    pool.truncate(OFFERED);
    pool
}

/// What may fire when the user selects a passage and asks. Register does not
/// gate here: §3.2 gives the invoked path to the user, so the risk is theirs.
///
/// `tier`, as in `automatic_probes` -- `None` unless the entry carries a
/// custom type. Only silent closes this path: heavy is "only when you ask",
/// so an invited question is exactly what it still allows.
pub fn invoked_probes(entry: &Entry, tier: Option<ProbeTier>) -> Vec<Probe> {
    if !has_own_span(entry) || tier == Some(ProbeTier::Silent) {
        return Vec::new();
    }

    match entry.role {
        Role::Position => Probe::ALL.to_vec(),
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

    /// The setting exists because the classifier calls a note live when it uses
    /// a personal example to argue an impersonal point -- the example mentions
    /// the speaker's life, so nothing is at stake but the prompt reads as if
    /// something is. That costs the probe, which is the thing the note was
    /// worth keeping for.
    #[test]
    fn turning_the_register_off_probes_a_live_note() {
        let live = entry(Role::Position, Register::Live, 60_000);

        assert!(
            automatic_probes(&live, true, None).is_empty(),
            "with the facet on, a live note is still left alone"
        );
        let neutral = entry(Role::Position, Register::Neutral, 60_000);
        assert_eq!(
            automatic_probes(&live, false, None),
            automatic_probes(&neutral, true, None),
            "with the facet off, a live position is probed like any other"
        );
    }

    /// Off means only the register stops applying. The other two gates are not
    /// about what the note is about, and §3.2 does not hand those to a setting.
    #[test]
    fn turning_the_register_off_does_not_lift_the_other_gates() {
        let brief = entry(Role::Position, Register::Neutral, 1_000);
        assert!(
            automatic_probes(&brief, false, None).is_empty(),
            "a note under 10s was probed because the register was off"
        );

        let mut borrowed = entry(Role::Position, Register::Neutral, 60_000);
        borrowed.spans = vec![Span {
            start: 0,
            end: borrowed.transcript.encode_utf16().count() as u32,
            attributed: true,
        }];
        assert!(
            automatic_probes(&borrowed, false, None).is_empty(),
            "a wholly attributed note was probed because the register was off"
        );
    }

    /// A note is still filed as live, so re-enabling the setting brings the
    /// suppression back rather than finding the value overwritten.
    #[test]
    fn the_setting_does_not_rewrite_what_the_classifier_decided() {
        let live = entry(Role::Position, Register::Live, 60_000);
        let _ = automatic_probes(&live, false, None);
        assert_eq!(live.register, Register::Live);
    }

    /// The three unconditional suppressions. Each of these on its own is
    /// enough to keep the app quiet.
    #[test]
    fn a_live_entry_is_never_asked_about_unprompted() {
        let e = entry(Role::Position, Register::Live, 120_000);
        assert!(automatic_probes(&e, true, None).is_empty());
    }

    #[test]
    fn something_said_in_under_ten_seconds_is_left_alone() {
        let e = entry(Role::Position, Register::Neutral, 9_900);
        assert!(automatic_probes(&e, true, None).is_empty());
    }

    /// The boundary is inclusive: ten seconds exactly is eligible, not skipped.
    #[test]
    fn ten_seconds_exactly_is_eligible() {
        let e = entry(Role::Position, Register::Neutral, 10_000);
        assert!(!automatic_probes(&e, true, None).is_empty());
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
        assert!(automatic_probes(&e, true, None).is_empty());
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
        assert!(!automatic_probes(&e, true, None).is_empty());
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
        assert!(automatic_probes(&e, true, None).is_empty());
    }

    /// Found in the packaged app: every question on a position was the same
    /// move, "where does this stop holding?", because the first tactic allowed
    /// was always the one used. Decided 13 Sep 2026 that a position is argued
    /// with, and which move fits -- a fallacy where there is one, a definition
    /// where one word carries the claim -- is the model's to judge from the
    /// note, asked or not. Whether to ask at all stays gated above.
    #[test]
    fn a_position_offers_every_debate_tactic_whether_asked_or_not() {
        let e = entry(Role::Position, Register::Neutral, 120_000);
        let automatic = automatic_probes(&e, true, None);
        for tactic in [
            Probe::Boundary,
            Probe::Disconfirming,
            Probe::Assumption,
            Probe::Counterexample,
            Probe::Definition,
            Probe::Consequence,
            Probe::Fallacy,
            Probe::Steelman,
        ] {
            assert!(automatic.contains(&tactic), "{tactic:?} is not offered");
        }
        assert_eq!(automatic, invoked_probes(&e, None));
    }

    /// A deliberate departure from "never F": being asked to say something
    /// back takes no stance and cannot wound, and withholding it until someone
    /// thinks to ask means it only ever fires for people who already know to
    /// want it.
    /// Measured against the real model over the position notes: offered all
    /// ten, it chose `assumption` 26 of 26 times, and 21 of 26 with the list
    /// shuffled. Offered three drawn at random, nine different moves across 26
    /// questions, every quote anchored, and `fallacy` taken once in six offers
    /// rather than forced onto notes that make no error.
    #[test]
    fn a_question_is_offered_three_moves_drawn_at_random() {
        let mut seen = std::collections::HashSet::new();
        for seed in 0..40u128 {
            let offered = offer(
                &Probe::ALL,
                seed.wrapping_mul(0x9E37_79B9_7F4A_7C15_F39C_C060_5CED_C834),
            );
            assert_eq!(offered.len(), 3, "{offered:?}");
            let distinct: std::collections::HashSet<_> = offered.iter().collect();
            assert_eq!(distinct.len(), 3, "a move offered twice: {offered:?}");
            let mut key: Vec<&str> = offered.iter().map(|p| p.id()).collect();
            key.sort();
            seen.insert(key);
        }
        assert!(
            seen.len() >= 10,
            "the draw barely varies: {} sets",
            seen.len()
        );
    }

    #[test]
    fn fewer_than_three_candidates_are_all_offered() {
        assert_eq!(offer(&[Probe::Feynman], 7), vec![Probe::Feynman]);
        assert!(offer(&[], 7).is_empty());
    }

    /// §3.6: a custom type may only narrow what role already allows. A silent
    /// custom type on a position closes both paths -- the same effect as the
    /// built-in `note` role has, arrived at from a different role entirely.
    #[test]
    fn a_silent_custom_type_closes_both_paths_on_a_position() {
        let e = entry(Role::Position, Register::Neutral, 120_000);
        assert!(automatic_probes(&e, true, Some(ProbeTier::Silent)).is_empty());
        assert!(invoked_probes(&e, Some(ProbeTier::Silent)).is_empty());
    }

    /// Heavy is "only when you ask": the automatic path closes, but the
    /// invoked one is untouched -- narrower than safe, not narrower than
    /// invoking already is.
    #[test]
    fn a_heavy_custom_type_closes_only_the_automatic_path() {
        let e = entry(Role::Position, Register::Neutral, 120_000);
        assert!(automatic_probes(&e, true, Some(ProbeTier::Heavy)).is_empty());
        assert!(!invoked_probes(&e, Some(ProbeTier::Heavy)).is_empty());
    }

    /// Safe behaves exactly as if there were no custom type at all -- it
    /// narrows nothing, which is what makes it "asks on its own".
    #[test]
    fn a_safe_custom_type_behaves_like_its_role() {
        let e = entry(Role::Position, Register::Neutral, 120_000);
        assert_eq!(
            automatic_probes(&e, true, Some(ProbeTier::Safe)),
            automatic_probes(&e, true, None)
        );
        assert_eq!(
            invoked_probes(&e, Some(ProbeTier::Safe)),
            invoked_probes(&e, None)
        );
    }

    /// A note offers nothing whatever role allows, and a tier is not allowed
    /// to widen that -- only narrow it.
    #[test]
    fn a_tier_never_widens_what_a_note_offers() {
        let e = entry(Role::Note, Register::Neutral, 120_000);
        assert!(automatic_probes(&e, true, Some(ProbeTier::Safe)).is_empty());
        assert!(invoked_probes(&e, Some(ProbeTier::Safe)).is_empty());
    }

    /// The evidence branch had no tier check at all before this: a custom
    /// type with heavy tier on an evidence-role entry still auto-fired
    /// feynman, which is exactly the automatic question heavy exists to
    /// suppress.
    #[test]
    fn a_heavy_custom_type_on_evidence_does_not_auto_fire_feynman() {
        let e = entry(Role::Evidence, Register::Neutral, 120_000);
        assert!(automatic_probes(&e, true, Some(ProbeTier::Heavy)).is_empty());
        assert!(invoked_probes(&e, Some(ProbeTier::Heavy)).contains(&Probe::Feynman));
    }

    #[test]
    fn evidence_opens_with_feynman() {
        let probes = automatic_probes(
            &entry(Role::Evidence, Register::Neutral, 120_000),
            true,
            None,
        );
        assert_eq!(probes, vec![Probe::Feynman]);
    }

    #[test]
    fn a_note_is_silent() {
        assert!(
            automatic_probes(&entry(Role::Note, Register::Neutral, 120_000), true, None).is_empty()
        );
    }

    /// The invoked path is the user's own risk, so register does not gate it.
    #[test]
    fn a_live_entry_can_still_be_asked_about_when_invited() {
        let e = entry(Role::Position, Register::Live, 120_000);
        assert!(automatic_probes(&e, true, None).is_empty());
        assert!(
            !invoked_probes(&e, None).is_empty(),
            "asking is the user's call"
        );
    }

    /// Duration gates the automatic path only. A short note you select a
    /// sentence in is still fair game.
    #[test]
    fn a_short_entry_can_still_be_asked_about_when_invited() {
        let e = entry(Role::Position, Register::Neutral, 5_000);
        assert!(automatic_probes(&e, true, None).is_empty());
        assert!(!invoked_probes(&e, None).is_empty());
    }

    /// Feynman needs only "not a note" -- it is the one move that makes the
    /// person do the thinking, and gating it behind `position` made it the
    /// least reachable thing in the app.
    #[test]
    fn evidence_can_be_asked_to_explain_itself_but_not_steelmanned() {
        let probes = invoked_probes(&entry(Role::Evidence, Register::Neutral, 120_000), None);
        assert!(probes.contains(&Probe::Feynman));
        assert!(!probes.contains(&Probe::Steelman));
    }

    #[test]
    fn a_note_reaches_nothing_even_when_invited() {
        assert!(invoked_probes(&entry(Role::Note, Register::Neutral, 120_000), None).is_empty());
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
        assert!(invoked_probes(&e, None).is_empty());
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

    /// `run_probe` carries a probe id over IPC, so the name has to survive the
    /// round trip. A probe whose id cannot be parsed back is unreachable.
    #[test]
    fn every_probe_survives_its_wire_name() {
        for probe in Probe::ALL {
            assert_eq!(Probe::from_id(probe.id()), Some(probe), "{probe:?}");
        }
    }

    /// Falling back to a probe would turn a typo into a question the gate was
    /// never asked about.
    #[test]
    fn an_unknown_probe_id_is_not_a_probe() {
        assert_eq!(Probe::from_id("regenerate"), None);
        assert_eq!(Probe::from_id(""), None);
    }

    /// An attributed span longer than the transcript must not underflow into
    /// a huge number and read as fully covered by accident.
    /// The gates above are a port, and every other test here checks the port
    /// against itself -- rename or re-tier a probe in `classification.ts` and
    /// nothing in Rust goes red. This reads the contract itself, so the drift
    /// that matters is the one it catches.
    ///
    /// Only the probe inventory and tiers are parsed. The role rules are one
    /// line of TypeScript each and extracting them would be a parser, not a
    /// test; they are covered case by case above.
    mod against_the_typescript_contract {
        use super::*;

        const CONTRACT: &str = include_str!("../../../lib/scene/classification.ts");

        /// `{ id: 'boundary', label: ..., hint: ... }` -> "boundary", for the
        /// probe table only. The type table above it has no `hint`, which is
        /// what identifies a probe row.
        fn probes_in_the_contract() -> Vec<String> {
            CONTRACT
                .lines()
                .filter(|line| line.contains("hint:") && line.contains("id:"))
                .map(|line| {
                    let at = line.find("id:").expect("an id") + "id:".len();
                    let rest = &line[at..];
                    let open = rest.find('\'').expect("quoted value") + 1;
                    let close = rest[open..].find('\'').expect("closing quote") + open;
                    rest[open..close].to_string()
                })
                .collect()
        }

        #[test]
        fn the_contract_is_parseable_at_all() {
            let found = probes_in_the_contract();
            assert_eq!(found.len(), Probe::ALL.len(), "parsed {found:?}");
        }

        /// A probe added or renamed on one side and not the other.
        #[test]
        fn every_probe_in_the_contract_exists_in_rust_and_no_others() {
            let mut from_contract: Vec<String> = probes_in_the_contract()
                .into_iter()
                .map(|id| {
                    assert!(
                        Probe::from_id(&id).is_some(),
                        "{id} is in classification.ts and not in Rust"
                    );
                    id
                })
                .collect();
            from_contract.sort();

            let mut from_rust: Vec<String> =
                Probe::ALL.iter().map(|p| p.id().to_string()).collect();
            from_rust.sort();

            assert_eq!(from_contract, from_rust);
        }

        /// A position opens on whichever move the model judges fits, so what
        /// may open is every tactic the contract names; evidence is still only
        /// asked to explain itself.
        #[test]
        fn a_position_opens_with_every_tactic_in_the_contract() {
            let mut expected = probes_in_the_contract();
            expected.sort();

            let mut actual: Vec<String> = automatic_probes(
                &entry(Role::Position, Register::Neutral, 120_000),
                true,
                None,
            )
            .iter()
            .map(|p| p.id().to_string())
            .collect();
            actual.sort();

            assert_eq!(actual, expected);

            let automatic_on_evidence = automatic_probes(
                &entry(Role::Evidence, Register::Neutral, 120_000),
                true,
                None,
            );
            assert_eq!(
                automatic_on_evidence
                    .iter()
                    .map(|p| p.id())
                    .collect::<Vec<_>>(),
                vec!["feynman"]
            );
        }

        /// Asking reaches everything the contract knows about, and nothing more.
        #[test]
        fn an_invited_position_reaches_every_probe_in_the_contract() {
            let mut expected = probes_in_the_contract();
            expected.sort();

            let mut actual: Vec<String> =
                invoked_probes(&entry(Role::Position, Register::Neutral, 120_000), None)
                    .iter()
                    .map(|p| p.id().to_string())
                    .collect();
            actual.sort();

            assert_eq!(actual, expected);
        }
    }

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
