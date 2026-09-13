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
            // And not even opened. `-ngl 0` keeps the layers off the card but
            // the Vulkan build still initialises the device, and a device held
            // open cannot power down: measured on the RTX 3050, the embedder
            // alone kept it in D0 for as long as the app ran. Unused, it sits in
            // D3 -- powered off -- which on battery is the difference that
            // matters.
            "--device".into(),
            "none".into(),
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

    /// A long note is cut to fit rather than refused.
    ///
    /// Measured against the packaged app: bge-small takes 512 tokens, about 450
    /// words, about three minutes of speech. Past that the server answered 500,
    /// the note got no vector, and `ask` -- which only searches notes that have
    /// one -- could never find it. The longest notes are the ones most worth
    /// recalling. The front of a note stands in fairly for ranking and recall;
    /// nothing at all does not.
    fn embed(&self, text: &str) -> Result<Vec<f32>> {
        let mut input = text.to_string();
        // The proportion lands it first time in practice; the spare tries
        // absorb tokenizer drift the ratio cannot see.
        for _ in 0..3 {
            let response = self.request(&input)?;
            match response["error"]["message"].as_str().and_then(too_large) {
                Some((tokens, limit)) => input = fit_words(&input, tokens, limit),
                None => return vector_from(&response),
            }
        }
        Err(Error::Other(
            "the note is too long for the embedder even when cut".into(),
        ))
    }
}

impl LlamaEmbedder {
    fn request(&self, text: &str) -> Result<Value> {
        let body = json!({ "input": text, "model": self.model_id });
        let response: Value = self
            .client
            .post(format!("http://127.0.0.1:{}/v1/embeddings", self.port))
            .json(&body)
            .send()
            .map_err(|e| Error::Other(format!("the embedder did not answer: {e}")))?
            .json()
            .map_err(|e| Error::Other(format!("the embedder's reply was not json: {e}")))?;
        Ok(response)
    }
}

fn vector_from(response: &Value) -> Result<Vec<f32>> {
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

/// llama-server's refusal of an input longer than the model's batch, as
/// `(tokens it had, tokens it takes)`.
fn too_large(message: &str) -> Option<(usize, usize)> {
    if !message.contains("too large") {
        return None;
    }
    let number_after = |marker: &str| -> Option<usize> {
        let rest = &message[message.find(marker)? + marker.len()..];
        rest.trim_start()
            .chars()
            .take_while(char::is_ascii_digit)
            .collect::<String>()
            .parse()
            .ok()
    };
    Some((number_after("input (")?, number_after("batch size:")?))
}

/// The front of `text`, sized to fit `limit` given that the whole ran to
/// `tokens`.
fn fit_words(text: &str, tokens: usize, limit: usize) -> String {
    let words: Vec<&str> = text.split_whitespace().collect();
    // Proportion with a margin, because tokens per word is not constant across
    // a note; capped below one so a confused refusal still shrinks the input
    // and the retry cannot spin on the same text.
    let share = (limit as f64 / tokens.max(1) as f64 * 0.85).min(0.85);
    let keep = (words.len() as f64 * share) as usize;
    let most = words.len().saturating_sub(1).max(1);
    words[..keep.clamp(1, most)].join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The message the embedder actually returned, measured against the
    /// packaged app: bge-small refuses anything past 512 tokens, which is about
    /// 450 words, which is about three minutes of speech.
    #[test]
    fn the_refusal_is_read_for_both_numbers() {
        let said = "input (702 tokens) is too large to process. increase the physical batch size (current batch size: 512)";
        assert_eq!(too_large(said), Some((702, 512)));
    }

    #[test]
    fn any_other_error_is_not_mistaken_for_a_length_problem() {
        assert_eq!(too_large("the model is not loaded"), None);
        assert_eq!(too_large(""), None);
    }

    /// Cut in proportion, with margin: tokens per word is not constant, so the
    /// exact ratio would land just over the limit as often as under it.
    #[test]
    fn a_long_note_is_cut_to_fit_with_room_to_spare() {
        let text = vec!["word"; 1000].join(" ");
        let kept = fit_words(&text, 1120, 512);
        let words = kept.split_whitespace().count();
        assert!(words < 1000 * 512 / 1120, "no margin left: kept {words}");
        assert!(
            words > 1000 * 512 / 1120 / 2,
            "cut far more than needed: kept {words}"
        );
        assert!(
            text.starts_with(&kept),
            "the front of the note, not a sample of it"
        );
    }

    /// A refusal that somehow names a limit above what it had must still make
    /// progress, or the retry loop spins on an input that never shrinks.
    #[test]
    fn a_cut_always_shrinks_the_input() {
        let text = vec!["word"; 50].join(" ");
        assert!(fit_words(&text, 40, 512).split_whitespace().count() < 50);
        assert!(!fit_words(&text, 600, 512).is_empty());
    }

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

        // `-ngl 0` keeps the layers off the card but still opens the Vulkan
        // device, and an open device cannot power down. Measured on the RTX
        // 3050: D0 with `-ngl 0` alone, D3 with `--device none`, same vector.
        let device = args.iter().position(|a| a == "--device").expect("--device");
        assert_eq!(args[device + 1], "none", "it must not hold the card awake");

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
