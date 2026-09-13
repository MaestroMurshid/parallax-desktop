//! The embedding model, in its own llama-server.
//!
//! Its own, because one server cannot do both: asked for an embedding, a
//! generative server answers `501 This server does not support embeddings.
//! Start it with --embeddings`, and that flag disables generation. Loading
//! Qwen3 twice would cost 2x2.5GB of RAM, so this is a second model rather
//! than a second endpoint -- tens of megabytes, and on the CPU.

use super::Embedder;
use crate::error::{Error, Result};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub struct LlamaEmbedder {
    port: u16,
    model_id: String,
    child: Mutex<Option<Child>>,
    client: reqwest::blocking::Client,
}

impl LlamaEmbedder {
    /// The arguments, as a value, so the flags this depends on can be asserted
    /// without spawning anything. `--embedding` is not a preference: without it
    /// the server answers 501 to every embedding request, and `/health` still
    /// says yes -- so a silent removal would look alive and fail at use.
    fn spawn_args(model: &PathBuf, port: u16) -> Vec<String> {
        vec![
            "-m".into(),
            model.to_string_lossy().into_owned(),
            "--port".into(),
            port.to_string(),
            "--no-webui".into(),
            "--embedding".into(),
            // Mean over the token vectors, which is what these models were
            // trained against. The default comes from the model's metadata and
            // is not the same across the three that are offered.
            "--pooling".into(),
            "mean".into(),
            // On the CPU deliberately. It is a 22M-137M parameter model and
            // runs in milliseconds there, while the 4GB card is the scarce
            // thing the reasoning model is already waiting for.
            "-ngl".into(),
            "0".into(),
            // No -c: the three models have trained context windows from 256 to
            // 8192, and naming one number here would silently truncate the
            // long-context model that was chosen precisely for it.
        ]
    }

    pub fn spawn(binary: &PathBuf, model: &PathBuf, model_id: &str) -> Result<Self> {
        let port = crate::llm::llama_server::free_port()?;

        let mut command = Command::new(binary);
        command
            .args(Self::spawn_args(model, port))
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let child = crate::llm::without_a_console(&mut command)
            .spawn()
            .map_err(|e| Error::Other(format!("could not start the embedder: {e}")))?;

        let embedder = Self {
            port,
            model_id: model_id.to_string(),
            child: Mutex::new(Some(child)),
            client: reqwest::blocking::Client::builder()
                .timeout(Duration::from_secs(60))
                .build()
                .map_err(|e| Error::Other(e.to_string()))?,
        };
        embedder.wait_until_ready(Duration::from_secs(60))?;
        Ok(embedder)
    }

    pub fn ready(&self) -> bool {
        self.client
            .get(format!("http://127.0.0.1:{}/health", self.port))
            .timeout(Duration::from_millis(500))
            .send()
            .map(|r| r.status().is_success())
            .unwrap_or(false)
    }

    /// Ready means "can embed", not "is alive".
    ///
    /// `/health` answers yes on a server started without `--embedding`, and on
    /// one loaded with a generative model, so waiting on it alone lets a
    /// process that can never embed report itself started -- and every call
    /// after it fails somewhere far from the cause. One real embedding is the
    /// cheap way to find out here instead.
    fn wait_until_ready(&self, budget: Duration) -> Result<()> {
        let deadline = Instant::now() + budget;
        while Instant::now() < deadline && !self.ready() {
            std::thread::sleep(Duration::from_millis(100));
        }
        if !self.ready() {
            return Err(Error::Other("the embedder did not start in time".into()));
        }
        self.embed("ready?").map_err(|e| {
            Error::Other(format!(
                "the embedder started but cannot embed, so every note would fail rather than this: {e}"
            ))
        })?;
        Ok(())
    }

    pub fn stop(&self) {
        if let Some(mut child) = self.child.lock().unwrap_or_else(|p| p.into_inner()).take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// The same backstop `LlamaServer` carries: Tauri exits through
/// `std::process::exit`, which runs no destructors, so the child is killed
/// explicitly from the exit handler and this only covers the other ways out.
impl Drop for LlamaEmbedder {
    fn drop(&mut self) {
        self.stop();
    }
}

impl Embedder for LlamaEmbedder {
    fn model_id(&self) -> String {
        self.model_id.clone()
    }

    fn embed(&self, text: &str) -> Result<Vec<f32>> {
        let body = json!({ "input": text, "model": self.model_id });
        let response: Value = self
            .client
            .post(format!("http://127.0.0.1:{}/v1/embeddings", self.port))
            .json(&body)
            .send()
            .map_err(|e| Error::Other(format!("the embedder did not answer: {e}")))?
            .json()
            .map_err(|e| Error::Other(format!("the embedder's reply was not json: {e}")))?;

        let first = response["data"]
            .get(0)
            .and_then(|d| d.get("embedding"))
            .ok_or_else(|| Error::Other("the embedder returned no vector".into()))?;

        // Some builds return one vector per token rather than a pooled one,
        // depending on the model's own pooling metadata. Averaging here means
        // the caller never has to know which shape arrived.
        let vector = match first.as_array() {
            Some(rows) if rows.first().is_some_and(Value::is_array) => mean(rows),
            Some(values) => values
                .iter()
                .filter_map(|v| v.as_f64().map(|f| f as f32))
                .collect(),
            None => Vec::new(),
        };
        if vector.is_empty() {
            return Err(Error::Other("the embedder returned an empty vector".into()));
        }
        Ok(vector)
    }
}

fn mean(rows: &[Value]) -> Vec<f32> {
    let width = rows.first().and_then(Value::as_array).map_or(0, Vec::len);
    let mut out = vec![0.0f32; width];
    for row in rows {
        for (i, v) in row.as_array().into_iter().flatten().enumerate() {
            if let Some(f) = v.as_f64() {
                out[i] += f as f32;
            }
        }
    }
    let n = rows.len().max(1) as f32;
    out.iter_mut().for_each(|x| *x /= n);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The flags are load-bearing and this runs in the ordinary suite, unlike
    /// the spawning test below. Without `--embedding` the server answers 501
    /// to every request while `/health` still says yes, so its removal has to
    /// be caught here rather than by whoever next remembers `--ignored`.
    #[test]
    fn the_arguments_carry_the_flags_the_endpoint_depends_on() {
        let args = LlamaEmbedder::spawn_args(&PathBuf::from("m.gguf"), 1234);
        assert!(args.contains(&"--embedding".to_string()), "{args:?}");

        let pooling = args.iter().position(|a| a == "--pooling").expect("pooling");
        assert_eq!(
            args[pooling + 1],
            "mean",
            "the three models pool differently"
        );

        let ngl = args.iter().position(|a| a == "-ngl").expect("-ngl");
        assert_eq!(args[ngl + 1], "0", "it must not compete for the card");

        assert!(
            !args.iter().any(|a| a == "-c"),
            "a fixed context would silently truncate the 8192-token model: {args:?}"
        );
    }

    /// Spawns the real binary against a real model, because this is the part
    /// unit tests cannot reach: the flags, the endpoint, and the shape of what
    /// comes back. Ignored by default -- it needs a downloaded model -- and run
    /// with `cargo test -- --ignored embeds_against_the_real_binary`.
    #[test]
    #[ignore]
    fn embeds_against_the_real_binary() {
        let binary =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("binaries/llama/llama-server.exe");
        let model = PathBuf::from(
            std::env::var("PARALLAX_TEST_EMBED_MODEL")
                .expect("set PARALLAX_TEST_EMBED_MODEL to a gguf"),
        );

        let embedder = LlamaEmbedder::spawn(&binary, &model, "test-model").unwrap();
        let a = embedder
            .embed("Database indexes trade write performance for faster reads.")
            .unwrap();
        let b = embedder
            .embed("Hash table lookup is O(1) on average.")
            .unwrap();
        let c = embedder
            .embed("Buy a new USB-C cable and book the dentist.")
            .unwrap();
        embedder.stop();

        assert!(
            a.len() > 100,
            "a sentence vector, not a scalar: {}",
            a.len()
        );
        assert_eq!(a.len(), b.len());

        let cos = |x: &[f32], y: &[f32]| {
            let dot: f32 = x.iter().zip(y).map(|(p, q)| p * q).sum();
            let nx: f32 = x.iter().map(|p| p * p).sum::<f32>().sqrt();
            let ny: f32 = y.iter().map(|q| q * q).sum::<f32>().sqrt();
            dot / (nx * ny)
        };
        // The whole premise in one assertion: two notes about storage are
        // nearer each other than either is to an errands list, without having
        // chosen the same words.
        assert!(
            cos(&a, &b) > cos(&a, &c),
            "indexes/hash {:.3} should beat indexes/errands {:.3}",
            cos(&a, &b),
            cos(&a, &c)
        );
    }
}
