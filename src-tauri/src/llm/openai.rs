//! Any chat-completions endpoint: OpenAI itself, or the many that copy its
//! shape -- OpenRouter, Groq, Mistral, DeepSeek, or a server on this computer
//! such as Ollama or LM Studio, which needs no key at all.
//!
//! Nothing model-specific is sent. No temperature, because reasoning models
//! refuse anything but the default; no token ceiling, because the parameter's
//! name differs between OpenAI and the servers copying it, and the schema
//! already bounds the reply.

use super::remote::{conform, portable, Endpoint};
use super::{Ask, LlmProvider};
use crate::error::{Error, Result};
use serde_json::{json, Value};
use std::time::Duration;

pub struct OpenAi {
    /// `None` for a server on this computer that asks for none.
    key: Option<String>,
    model: String,
    endpoint: String,
    on_this_machine: bool,
    client: reqwest::blocking::Client,
}

impl OpenAi {
    pub fn new(key: Option<String>, model: String, endpoint: &Endpoint) -> Result<Self> {
        Ok(Self {
            key,
            model,
            endpoint: endpoint.call("chat/completions"),
            on_this_machine: endpoint.on_this_machine(),
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
        !self.on_this_machine
    }

    fn ask(&self, ask: Ask) -> Result<String> {
        let portable = ask.schema.as_ref().map(|s| portable(s, true));
        let body = body(&self.model, &ask, portable.as_ref().map(|p| &p.schema));

        let mut request = self.client.post(&self.endpoint);
        if let Some(key) = &self.key {
            request = request.bearer_auth(key);
        }
        let response: Value = request
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

    fn at(url: &str) -> OpenAi {
        OpenAi::new(None, "m".into(), &Endpoint::parse(url).unwrap()).unwrap()
    }

    #[test]
    fn the_endpoint_is_built_from_the_base_url() {
        assert_eq!(
            at("https://openrouter.ai/api/v1/").endpoint,
            "https://openrouter.ai/api/v1/chat/completions"
        );
    }

    /// Ollama on this computer: local-only notes may go to it.
    #[test]
    fn a_server_on_this_computer_is_not_remote() {
        assert!(!at("http://localhost:11434/v1").remote());
        assert!(at("https://api.openai.com/v1").remote());
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
