//! One error type for the whole command surface.
//!
//! Java notes:
//!   Result<T, Error>  ~  a checked exception the compiler will not let you ignore
//!   the `?` operator   ~  rethrow-and-propagate, one character wide
//!   thiserror          ~  boilerplate generator for the Exception subclass
//!
//! A Tauri command's error type has to be Serialize, because it crosses the IPC
//! boundary and lands in the frontend as a rejected promise.

use serde::{Serialize, Serializer};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("database: {0}")]
    Db(#[from] rusqlite::Error),

    #[error("json: {0}")]
    Json(#[from] serde_json::Error),

    #[error("io: {0}")]
    Io(#[from] std::io::Error),

    /// Something the caller asked for does not exist. Distinct from a failure:
    /// several commands are specified to return null rather than throw.
    #[error("not found: {0}")]
    NotFound(String),

    #[error("{0}")]
    Other(String),
}

/// The frontend only ever displays this, so a string is the whole contract.
impl Serialize for Error {
    // NOTE: `std::result::Result` spelled out on purpose -- the `Result<T>`
    // alias at the bottom of this file shadows the two-parameter std one.
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

/// Every command returns this. `Result<T, Error>` with the error half fixed.
pub type Result<T> = std::result::Result<T, Error>;
