//! The things a note says to do, kept in the note's own words.
//!
//! Only a note filed as `note` is read for them: an argument or an observation
//! that happens to contain "I should" is not a to-do list.

use super::run::anchor;
use crate::error::Result;
use crate::llm::{Ask, LlmProvider};
use crate::model::{Entry, Span};
use serde::Deserialize;
use serde_json::{json, Value};

const TASKS_SYSTEM: &str = "\
You list what a spoken note says the speaker has to do.
Follow these constraints strictly.

### Fields to Extract

- **tasks**: Each thing the speaker needs or intends to do, one per item, copied word for word from the note. Copy the shortest passage that still says what to do. Leave out anything already done and anything only wondered about. Return an empty list if the note says nothing to do.";

#[derive(Debug, Deserialize)]
struct Reply {
    #[serde(default)]
    tasks: Vec<String>,
}

fn tasks_schema() -> Value {
    todo!()
}

/// Each task anchored in the transcript, with the words it quotes.
pub fn extract(provider: &dyn LlmProvider, entry: &Entry) -> Result<Vec<(Span, String)>> {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::fake::FakeProvider;

    const ERRANDS: &str = "I need to buy a pen and some books. I already renewed the token. \
        Book the dentist for next week.";

    fn note(transcript: &str) -> Entry {
        Entry {
            id: "e1".into(),
            audio_path: None,
            transcript: transcript.into(),
            created_at: "2024-01-01T00:00:00Z".into(),
            x: 0.0,
            y: 0.0,
            parent_entry_id: None,
            answers_question_id: None,
            role: crate::model::Role::Note,
            register: crate::model::Register::Neutral,
            type_id: "note".into(),
            resolved: false,
            resolution_text: None,
            title: "errands".into(),
            summary: None,
            duration_ms: 9_000,
            fingerprint: Vec::new(),
            unfinished: false,
            local_only: false,
            spans: Vec::new(),
            action_items: Vec::new(),
            is_sample: None,
        }
    }

    fn js_slice(s: &str, span: &Span) -> String {
        let units: Vec<u16> = s.encode_utf16().collect();
        String::from_utf16_lossy(&units[span.start as usize..span.end as usize])
    }

    #[test]
    fn each_task_is_anchored_in_the_speakers_words() {
        let provider = FakeProvider::replying(
            r#"{"tasks":["buy a pen and some books","Book the dentist for next week."]}"#,
        );
        let tasks = extract(&provider, &note(ERRANDS)).unwrap();

        let quoted: Vec<&str> = tasks.iter().map(|(_, q)| q.as_str()).collect();
        assert_eq!(
            quoted,
            vec!["buy a pen and some books", "Book the dentist for next week."]
        );
        for (span, quote) in &tasks {
            assert_eq!(&js_slice(ERRANDS, span), quote);
        }
    }

    /// The same rule as every other quote the model returns: a task the note
    /// does not say is a task invented for the speaker.
    #[test]
    fn a_task_the_note_does_not_say_is_dropped() {
        let provider =
            FakeProvider::replying(r#"{"tasks":["call the bank","buy a pen and some books"]}"#);
        let tasks = extract(&provider, &note(ERRANDS)).unwrap();
        let quoted: Vec<&str> = tasks.iter().map(|(_, q)| q.as_str()).collect();
        assert_eq!(quoted, vec!["buy a pen and some books"]);
    }

    #[test]
    fn a_task_returned_twice_lands_once() {
        let provider = FakeProvider::replying(
            r#"{"tasks":["buy a pen and some books"," buy a pen and some books "]}"#,
        );
        assert_eq!(extract(&provider, &note(ERRANDS)).unwrap().len(), 1);
    }

    #[test]
    fn a_note_with_nothing_to_do_has_no_tasks() {
        let provider = FakeProvider::replying(r#"{"tasks":[]}"#);
        assert!(extract(&provider, &note("The wifi password is on the fridge."))
            .unwrap()
            .is_empty());
    }

    /// Bounded, or a looping model fills the array until the context runs out.
    #[test]
    fn the_schema_requires_a_bounded_list_of_strings() {
        let provider = FakeProvider::replying(r#"{"tasks":[]}"#);
        extract(&provider, &note(ERRANDS)).unwrap();
        let schema = provider.last_schema().expect("a constrained call");
        assert_eq!(schema["required"], json!(["tasks"]));
        assert_eq!(schema["properties"]["tasks"]["items"]["type"], "string");
        assert!(schema["properties"]["tasks"]["maxItems"].is_u64());
    }
}
