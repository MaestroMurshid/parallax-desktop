//! API keys, kept in the operating system's credential store.
//!
//! Not in settings: those are a JSON row inside corpus.db, and the corpus is
//! the thing people back up, export and move between machines. A key there
//! would travel with every copy. The credential store is per-user and
//! per-machine, which is exactly the reach a key should have.
//!
//! Keys only ever flow inwards. The frontend can set one, clear one and ask
//! whether one is saved -- never read one back.

use crate::error::{Error, Result};
use crate::model::ReasoningProvider;

/// Matches the bundle identifier, so the entry is recognisably this app's in
/// Keychain Access or Credential Manager.
const SERVICE: &str = "app.parallax.desktop";

fn account(provider: ReasoningProvider) -> Result<&'static str> {
    match provider {
        ReasoningProvider::Anthropic => Ok("anthropic-api-key"),
        ReasoningProvider::Openai => Ok("openai-api-key"),
        ReasoningProvider::Local => Err(Error::Other("the local model takes no API key".into())),
    }
}

fn entry(provider: ReasoningProvider) -> Result<keyring::Entry> {
    keyring::Entry::new(SERVICE, account(provider)?)
        .map_err(|e| Error::Other(format!("the system keychain is unavailable: {e}")))
}

/// The saved key, or `None` when there is none or the store cannot be read.
/// A store that is unavailable reads as "no key" here; `set` is where that
/// gets said out loud, because that is where someone is waiting on it.
pub fn get(provider: ReasoningProvider) -> Option<String> {
    entry(provider)
        .ok()?
        .get_password()
        .ok()
        .filter(|k| !k.trim().is_empty())
}

pub fn has(provider: ReasoningProvider) -> bool {
    get(provider).is_some()
}

pub fn set(provider: ReasoningProvider, key: &str) -> Result<()> {
    let key = key.trim();
    if key.is_empty() {
        return Err(Error::Other("that key is empty".into()));
    }
    entry(provider)?.set_password(key).map_err(|e| {
        Error::Other(format!(
            "could not save the key to the system keychain: {e}"
        ))
    })
}

pub fn clear(provider: ReasoningProvider) -> Result<()> {
    match entry(provider)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(Error::Other(format!("could not remove the key: {e}"))),
    }
}
