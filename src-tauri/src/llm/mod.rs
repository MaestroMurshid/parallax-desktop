//! §9.4 -- the reasoning model sits behind a provider interface, so the local
//! path and a remote one are the same shape to everything above them.
//!
//! Audio and embeddings are not here on purpose. They are local
//! unconditionally, so there is nothing for them to be abstracted over.

pub mod anthropic;
pub mod binary;
pub mod keys;
pub mod llama_server;
pub mod openai;
pub mod remote;

use crate::error::{Error, Result};
use crate::model::{ReasoningProvider, Settings};
use serde_json::Value;

/// Starts a child without giving it a console window.
///
/// llama-server and `--list-devices` are console programs, and a release build
/// is `windows_subsystem = "windows"`, so it has no console of its own to lend
/// them. Windows then allocates each child a fresh one: capturing a note put
/// two black terminals on screen, one for the reasoning server and one for the
/// embedder. Redirecting stdout and stderr does not prevent this -- the console
/// is allocated for the process, not for its streams, so the flag is the only
/// thing that suppresses it.
pub fn without_a_console(command: &mut std::process::Command) -> &mut std::process::Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // CREATE_NO_WINDOW. Named here rather than pulled from winapi, which
        // this crate does not otherwise depend on.
        command.creation_flags(0x0800_0000);
    }
    command
}

pub struct Ask<'a> {
    pub system: &'a str,
    pub user: &'a str,
    /// A JSON schema the reply is constrained to.
    ///
    /// llama.cpp converts this to a grammar and zeroes out any token that
    /// would break it, so malformed JSON is impossible rather than unlikely.
    /// Note the model never sees the schema -- it constrains the output and is
    /// not injected into the prompt, so the shape still has to be described in
    /// words if the model is meant to understand it.
    pub schema: Option<Value>,
    pub max_tokens: Option<u32>,
    pub temperature: f32,
}

impl<'a> Ask<'a> {
    pub fn new(system: &'a str, user: &'a str) -> Self {
        Self {
            system,
            user,
            schema: None,
            max_tokens: None,
            // Low but not zero. Deterministic output makes a bad question
            // reproducible, which is not the same as making it better.
            temperature: 0.3,
        }
    }

    pub fn constrained(mut self, schema: Value) -> Self {
        self.schema = Some(schema);
        self
    }
}

pub trait LlmProvider: Send + Sync {
    /// Shown in the UI verbatim -- the user always knows who answered (§9.4).
    fn name(&self) -> String;

    fn ask(&self, ask: Ask) -> Result<String>;

    /// Whether a call would land now, so a caller can degrade rather than wait.
    fn ready(&self) -> bool;

    /// Whether what is asked leaves this machine. A note marked local-only is
    /// never put to a provider that says yes here (§9.4).
    fn remote(&self) -> bool {
        false
    }
}

/// The model and key the cloud provider in `settings` would use, or why not.
/// `Ok(None)` for the local provider, which is not this function's business.
fn cloud_parts(settings: &Settings) -> Result<Option<(String, String)>> {
    let model = match settings.reasoning_provider {
        ReasoningProvider::Local => return Ok(None),
        ReasoningProvider::Anthropic => settings.anthropic_model.trim(),
        ReasoningProvider::Openai => settings.openai_model.trim(),
    };
    if model.is_empty() {
        return Err(Error::Other(
            "no model is named for the cloud provider yet".into(),
        ));
    }
    let key = keys::get(settings.reasoning_provider)
        .ok_or_else(|| Error::Other("no API key is saved for the cloud provider yet".into()))?;
    Ok(Some((model.to_string(), key)))
}

/// Whether the chosen cloud provider has everything it needs to be asked.
/// Always false for the local one; reads the keychain, so not for hot paths.
pub fn cloud_configured(settings: &Settings) -> bool {
    matches!(cloud_parts(settings), Ok(Some(_)))
}

/// The cloud provider `settings` chooses. `Ok(None)` when the choice is the
/// local model, and an error saying what is missing when it is a cloud one
/// that cannot be asked yet.
pub fn cloud(settings: &Settings) -> Result<Option<Box<dyn LlmProvider>>> {
    let Some((model, key)) = cloud_parts(settings)? else {
        return Ok(None);
    };
    Ok(Some(match settings.reasoning_provider {
        ReasoningProvider::Anthropic => Box::new(anthropic::Anthropic::new(key, model)?),
        ReasoningProvider::Openai => {
            Box::new(openai::OpenAi::new(key, model, &settings.openai_base_url)?)
        }
        ReasoningProvider::Local => unreachable!("cloud_parts returned None for local"),
    }))
}

#[cfg(test)]
pub mod fake {
    use super::*;
    use std::sync::Mutex;

    /// Records what it was asked and replies with whatever it was given, so
    /// prompt construction can be tested without a model.
    pub struct FakeProvider {
        pub reply: String,
        pub asked: Mutex<Vec<(String, String, Option<Value>)>>,
    }

    impl FakeProvider {
        pub fn replying(reply: &str) -> Self {
            Self {
                reply: reply.to_string(),
                asked: Mutex::new(Vec::new()),
            }
        }

        pub fn last_user_prompt(&self) -> String {
            self.asked.lock().unwrap().last().unwrap().1.clone()
        }

        pub fn last_schema(&self) -> Option<Value> {
            self.asked.lock().unwrap().last().unwrap().2.clone()
        }
    }

    impl LlmProvider for FakeProvider {
        fn name(&self) -> String {
            "fake".to_string()
        }

        fn ask(&self, ask: Ask) -> Result<String> {
            self.asked.lock().unwrap().push((
                ask.system.to_string(),
                ask.user.to_string(),
                ask.schema.clone(),
            ));
            Ok(self.reply.clone())
        }

        fn ready(&self) -> bool {
            true
        }
    }

    /// Any provider, presented as one that is not on this machine -- for the
    /// tests that local-only notes are never put to it.
    pub struct Remote<P>(pub P);

    impl<P: LlmProvider> LlmProvider for Remote<P> {
        fn name(&self) -> String {
            "remote".to_string()
        }

        fn ask(&self, ask: Ask) -> Result<String> {
            self.0.ask(ask)
        }

        fn ready(&self) -> bool {
            true
        }

        fn remote(&self) -> bool {
            true
        }
    }

    /// Replies in order, so one enrichment run can drive classify and the
    /// question with different output.
    pub struct ScriptedProvider {
        replies: Mutex<std::collections::VecDeque<String>>,
        pub asked: Mutex<Vec<String>>,
    }

    impl ScriptedProvider {
        pub fn with(replies: &[&str]) -> Self {
            Self {
                replies: Mutex::new(replies.iter().map(|r| r.to_string()).collect()),
                asked: Mutex::new(Vec::new()),
            }
        }

        pub fn calls(&self) -> usize {
            self.asked.lock().unwrap().len()
        }
    }

    impl LlmProvider for ScriptedProvider {
        fn name(&self) -> String {
            "scripted".to_string()
        }

        fn ask(&self, ask: Ask) -> Result<String> {
            self.asked.lock().unwrap().push(ask.user.to_string());
            self.replies
                .lock()
                .unwrap()
                .pop_front()
                .ok_or_else(|| crate::error::Error::Other("the script ran out".into()))
        }

        fn ready(&self) -> bool {
            true
        }
    }
}
