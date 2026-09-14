//! Tasks from a note filed as `note`, in the note's own words.

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

- **tasks**: Each thing the speaker needs or intends to do, copied word for word from the note. Things to do together in one breath are one task. Copy the shortest passage that still says what to do. Leave out anything already done and anything only wondered about. Return an empty list if the note says nothing to do.";

#[derive(Debug, Deserialize)]
struct Reply {
    #[serde(default)]
    tasks: Vec<String>,
}

fn tasks_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "tasks": {
                "type": "array",
                "items": { "type": "string", "maxLength": 200 },
                "maxItems": 10,
            },
        },
        "required": ["tasks"],
        "additionalProperties": false,
    })
}

/// Each task with its span. A task not found verbatim in the note is dropped.
pub fn extract(provider: &dyn LlmProvider, entry: &Entry) -> Result<Vec<(Span, String)>> {
    let user = super::within(&entry.transcript, super::CLASSIFY_TRANSCRIPT_BYTES);
    let ask = Ask::new(TASKS_SYSTEM, user).constrained(tasks_schema());
    let reply: Reply = super::repair_and_parse_json(&provider.ask(ask)?)?;

    let mut tasks: Vec<(Span, String)> = Vec::new();
    for said in reply.tasks {
        let said = said.trim();
        if tasks.iter().any(|(_, q)| q == said) {
            continue;
        }
        if let Some(span) = anchor(entry, said) {
            tasks.push((span, said.to_string()));
        }
    }
    Ok(tasks)
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
            vec![
                "buy a pen and some books",
                "Book the dentist for next week."
            ]
        );
        for (span, quote) in &tasks {
            assert_eq!(&js_slice(ERRANDS, span), quote);
        }
    }

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
        assert!(
            extract(&provider, &note("The wifi password is on the fridge."))
                .unwrap()
                .is_empty()
        );
    }

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
