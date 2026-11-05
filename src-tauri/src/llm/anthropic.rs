//! Claude, through the Anthropic Messages API, with the user's own key.
//! Chosen whenever the API URL is api.anthropic.com (`remote::Wire`).
//!
//! Raw HTTP because there is no official Anthropic SDK for Rust. The shape
//! follows the current API version: structured output through
//! `output_config.format`, depth through `output_config.effort`, and no
//! sampling parameters (`temperature` is refused from API version 2026-10-21).

use super::remote::{conform, portable};
use super::{Ask, LlmProvider};
use crate::error::{Error, Result};
use serde_json::{json, Value};
use std::time::Duration;

const URL: &str = "https://api.anthropic.com/v1/messages";
const API_VERSION: &str = "2026-10-21";
/// A refusal on these models is re-run on another model inside the same call
/// rather than coming back as an empty note.
const FALLBACK_BETA: &str = "server-side-fallback-2026-07-01";
const WITH_FALLBACKS: &[&str] = &[
    "claude-fable-5-1",
    "claude-opus-5-5",
    "claude-opus-5",
    "claude-sonnet-5-5",
];

/// Room for the model's thinking as well as the reply. `Ask::max_tokens` is
/// sized for a local model that answers straight away; current Claude models
/// think first, and the thinking counts against this ceiling.
const MAX_TOKENS: u32 = 16_000;

pub struct Anthropic {
    key: String,
    model: String,
    client: reqwest::blocking::Client,
}

impl Anthropic {
    pub fn new(key: String, model: String) -> Result<Self> {
        Ok(Self {
            key,
            model,
            client: reqwest::blocking::Client::builder()
                .timeout(Duration::from_secs(300))
                .build()
                .map_err(|e| Error::Other(e.to_string()))?,
        })
    }
}

/// The request body. Separate from the call so its shape is testable without
/// a key or a network.
fn body(model: &str, ask: &Ask, schema: Option<&Value>) -> Value {
    let mut body = json!({
        "model": model,
        "max_tokens": MAX_TOKENS,
        "system": ask.system,
        "messages": [{ "role": "user", "content": ask.user }],
        // Every call here is a short reading of one or two notes. Medium is
        // Claude Opus 5.5's own default, named so it does not move under us.
        "output_config": { "effort": "medium" },
    });
    if let Some(schema) = schema {
        body["output_config"]["format"] = json!({ "type": "json_schema", "schema": schema });
    }
    if WITH_FALLBACKS.contains(&model) {
        body["fallbacks"] = json!("default");
    }
    body
}

/// The text of a Messages response, or why there is none.
fn reply_text(response: &Value) -> Result<String> {
    if let Some(message) = response["error"]["message"].as_str() {
        return Err(Error::Other(format!(
            "Anthropic refused the request: {message}"
        )));
    }
    match response["stop_reason"].as_str() {
        Some("refusal") => {
            return Err(Error::Other("Claude declined to answer this one".into()));
        }
        Some("max_tokens") => {
            return Err(Error::Other(
                "Claude ran out of room before it finished".into(),
            ));
        }
        _ => {}
    }
    let text: String = response["content"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|block| block["type"] == "text")
        .filter_map(|block| block["text"].as_str())
        .collect();
    if text.trim().is_empty() {
        return Err(Error::Other("Claude returned no content".into()));
    }
    Ok(text)
}

impl LlmProvider for Anthropic {
    fn name(&self) -> String {
        self.model.clone()
    }

    fn ready(&self) -> bool {
        true
    }

    fn remote(&self) -> bool {
        true
    }

    fn ask(&self, ask: Ask) -> Result<String> {
        let portable = ask.schema.as_ref().map(|s| portable(s, false));
        let body = body(&self.model, &ask, portable.as_ref().map(|p| &p.schema));

        let mut request = self
            .client
            .post(URL)
            .header("x-api-key", &self.key)
            .header("anthropic-version", API_VERSION);
        if body.get("fallbacks").is_some() {
            request = request.header("anthropic-beta", FALLBACK_BETA);
        }
        let response: Value = request
            .json(&body)
            .send()
            .map_err(|e| Error::Other(format!("could not reach Anthropic: {e}")))?
            .json()
            .map_err(|e| Error::Other(format!("Anthropic sent something unreadable: {e}")))?;

        let text = reply_text(&response)?;
        match (&ask.schema, &portable) {
            (Some(original), Some(p)) => conform(original, p, &text),
            _ => Ok(text.trim().to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_body_sends_no_sampling_parameters() {
        let ask = Ask::new("system", "user");
        let b = body("claude-opus-5-5", &ask, None);
        assert!(
            b.get("temperature").is_none(),
            "refused on API version 2026-10-21"
        );
        assert!(
            b.get("thinking").is_none(),
            "Opus 5.5 refuses thinking: disabled"
        );
        assert_eq!(b["system"], "system");
        assert_eq!(b["messages"][0]["content"], "user");
        assert_eq!(b["output_config"]["effort"], "medium");
    }

    #[test]
    fn a_schema_goes_in_output_config() {
        let ask = Ask::new("s", "u");
        let schema = json!({ "type": "object" });
        let b = body("claude-opus-5-5", &ask, Some(&schema));
        assert_eq!(b["output_config"]["format"]["type"], "json_schema");
        assert_eq!(b["output_config"]["format"]["schema"], schema);
    }

    #[test]
    fn fallbacks_only_on_models_that_take_them() {
        let ask = Ask::new("s", "u");
        assert_eq!(body("claude-opus-5-5", &ask, None)["fallbacks"], "default");
        assert!(body("claude-haiku-5-5", &ask, None)
            .get("fallbacks")
            .is_none());
    }

    #[test]
    fn text_blocks_are_joined_and_thinking_is_skipped() {
        let response = json!({
            "stop_reason": "end_turn",
            "content": [
                { "type": "thinking", "thinking": "" },
                { "type": "text", "text": "{\"a\":" },
                { "type": "text", "text": "1}" },
            ],
        });
        assert_eq!(reply_text(&response).unwrap(), "{\"a\":1}");
    }

    #[test]
    fn a_refusal_or_an_error_is_said_rather_than_parsed() {
        let refusal = json!({ "stop_reason": "refusal", "content": [] });
        assert!(reply_text(&refusal)
            .unwrap_err()
            .to_string()
            .contains("declined"));
        let error = json!({ "type": "error", "error": { "message": "invalid x-api-key" } });
        assert!(reply_text(&error)
            .unwrap_err()
            .to_string()
            .contains("invalid x-api-key"));
    }
}
