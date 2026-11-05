//! Any chat-completions endpoint with a key: OpenAI itself, or a compatible
//! one -- OpenRouter, Groq, a server on this computer.
//!
//! Nothing model-specific is sent. No temperature, because reasoning models
//! refuse anything but the default; no token ceiling, because the parameter's
//! name differs between OpenAI and the servers copying it, and the schema
//! already bounds the reply.

use super::remote::{check_endpoint, conform, portable};
use super::{Ask, LlmProvider};
use crate::error::{Error, Result};
use serde_json::{json, Value};
use std::time::Duration;

pub struct OpenAi {
    key: String,
    model: String,
    endpoint: String,
    client: reqwest::blocking::Client,
}

impl OpenAi {
    pub fn new(key: String, model: String, base_url: &str) -> Result<Self> {
        check_endpoint(base_url)?;
        Ok(Self {
            key,
            model,
            endpoint: format!("{}/chat/completions", base_url.trim().trim_end_matches('/')),
            client: reqwest::blocking::Client::builder()
                .timeout(Duration::from_secs(300))
                .build()
                .map_err(|e| Error::Other(e.to_string()))?,
        })
    }
}

fn body(model: &str, ask: &Ask, schema: Option<&Value>) -> Value {
    let mut body = json!({
        "model": model,
        "messages": [
            { "role": "system", "content": ask.system },
            { "role": "user", "content": ask.user },
        ],
    });
    if let Some(schema) = schema {
        body["response_format"] = json!({
            "type": "json_schema",
            "json_schema": { "name": "reply", "strict": true, "schema": schema },
        });
    }
    body
}

fn reply_text(response: &Value) -> Result<String> {
    if let Some(message) = response["error"]["message"].as_str() {
        return Err(Error::Other(format!(
            "the provider refused the request: {message}"
        )));
    }
    let message = &response["choices"][0]["message"];
    if let Some(refusal) = message["refusal"].as_str().filter(|r| !r.is_empty()) {
        return Err(Error::Other(format!("the model declined: {refusal}")));
    }
    message["content"]
        .as_str()
        .map(str::to_string)
        .filter(|t| !t.trim().is_empty())
        .ok_or_else(|| Error::Other("the provider returned no content".into()))
}

impl LlmProvider for OpenAi {
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
        let portable = ask.schema.as_ref().map(|s| portable(s, true));
        let body = body(&self.model, &ask, portable.as_ref().map(|p| &p.schema));

        let response: Value = self
            .client
            .post(&self.endpoint)
            .bearer_auth(&self.key)
            .json(&body)
            .send()
            .map_err(|e| Error::Other(format!("could not reach the provider: {e}")))?
            .json()
            .map_err(|e| Error::Other(format!("the provider sent something unreadable: {e}")))?;

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
    fn the_endpoint_is_built_from_the_base_url() {
        let p = OpenAi::new("k".into(), "m".into(), "https://openrouter.ai/api/v1/").unwrap();
        assert_eq!(p.endpoint, "https://openrouter.ai/api/v1/chat/completions");
    }

    #[test]
    fn a_plain_http_endpoint_elsewhere_is_refused() {
        assert!(OpenAi::new("k".into(), "m".into(), "http://example.com/v1").is_err());
    }

    #[test]
    fn the_body_is_strict_and_model_neutral() {
        let ask = Ask::new("s", "u");
        let schema = json!({ "type": "object" });
        let b = body("m", &ask, Some(&schema));
        assert!(b.get("temperature").is_none());
        assert!(b.get("max_tokens").is_none());
        assert_eq!(b["response_format"]["json_schema"]["strict"], true);
        assert_eq!(b["messages"][0]["role"], "system");
    }

    #[test]
    fn refusals_and_errors_are_said() {
        let refusal = json!({ "choices": [{ "message": { "content": null, "refusal": "no" } }] });
        assert!(reply_text(&refusal).is_err());
        let error = json!({ "error": { "message": "Incorrect API key provided" } });
        assert!(reply_text(&error)
            .unwrap_err()
            .to_string()
            .contains("Incorrect API key"));
        let ok = json!({ "choices": [{ "message": { "content": "{}", "refusal": null } }] });
        assert_eq!(reply_text(&ok).unwrap(), "{}");
    }
}
