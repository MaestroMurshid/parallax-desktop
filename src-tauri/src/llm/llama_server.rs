//! llama-server: spawn it, wait for it, talk to it, and take it down.
//!
//! Measured on the target machine, Qwen3-4B Q4_K_M: 1.8s per enrichment call
//! with the model on the GPU, 9.0s on CPU. §4 budgets about two seconds for
//! the post-recording question, so offload is not an optimisation here -- it
//! is the difference between the synchronous question existing and not.

use super::{Ask, LlmProvider};
use crate::error::{Error, Result};
use crate::model::ComputeBackend;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub struct LlamaServer {
    port: u16,
    model_name: String,
    child: Mutex<Option<Child>>,
    client: reqwest::blocking::Client,
}

impl LlamaServer {
    /// Spawns the server and waits for it to answer `/health`.
    ///
    /// A port is chosen by binding one and letting it go, rather than picking a
    /// number and hoping: a fixed port collides with whatever else the machine
    /// is running, and the failure looks like the model being broken.
    pub fn spawn(
        binary: &PathBuf,
        model: &PathBuf,
        backend: ComputeBackend,
        context: u32,
    ) -> Result<Self> {
        let port = free_port()?;

        let mut command = Command::new(binary);
        command
            .arg("-m")
            .arg(model)
            .arg("--port")
            .arg(port.to_string())
            .arg("-c")
            .arg(context.to_string())
            .arg("--no-webui")
            .stdout(Stdio::null())
            .stderr(Stdio::null());

        // Offload everything or nothing. A 4B Q4 sits at about 3.1GB of a 4GB
        // card, so a partial split buys latency at the cost of the headroom the
        // KV cache needs, and running out mid-answer is worse than being slow.
        match backend {
            ComputeBackend::Cpu => {
                command.arg("-ngl").arg("0");
            }
            _ => {
                command.arg("-ngl").arg("99");
            }
        }

        let child = command
            .spawn()
            .map_err(|e| Error::Other(format!("could not start llama-server: {e}")))?;

        let server = Self {
            port,
            model_name: model
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| "llama-server".to_string()),
            child: Mutex::new(Some(child)),
            client: reqwest::blocking::Client::builder()
                .timeout(Duration::from_secs(180))
                .build()
                .map_err(|e| Error::Other(e.to_string()))?,
        };

        server.wait_until_ready(Duration::from_secs(120))?;
        Ok(server)
    }

    fn wait_until_ready(&self, budget: Duration) -> Result<()> {
        let deadline = Instant::now() + budget;
        while Instant::now() < deadline {
            if self.ready() {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(200));
        }
        Err(Error::Other(
            "llama-server did not become ready in time".into(),
        ))
    }

    pub fn stop(&self) {
        if let Some(mut child) = self.child.lock().unwrap_or_else(|p| p.into_inner()).take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// A backstop, not the mechanism. Tauri exits through `std::process::exit`,
/// which runs no destructors, so anything held in managed state is never
/// dropped on quit -- the tray's Quit item included. The child is killed
/// explicitly from the exit handler in `lib.rs`; this only covers a
/// `LlamaServer` that goes out of scope some other way, such as the early
/// return when it never became ready.
impl Drop for LlamaServer {
    fn drop(&mut self) {
        self.stop();
    }
}

impl LlmProvider for LlamaServer {
    fn name(&self) -> String {
        self.model_name.clone()
    }

    fn ready(&self) -> bool {
        self.client
            .get(format!("http://127.0.0.1:{}/health", self.port))
            .timeout(Duration::from_millis(500))
            .send()
            .map(|r| r.status().is_success())
            .unwrap_or(false)
    }

    fn ask(&self, ask: Ask) -> Result<String> {
        let mut body = json!({
            "messages": [
                { "role": "system", "content": ask.system },
                { "role": "user", "content": ask.user },
            ],
            "temperature": ask.temperature,
            "max_tokens": ask.max_tokens,
        });

        if let Some(schema) = ask.schema {
            body["response_format"] = json!({
                "type": "json_schema",
                "json_schema": { "name": "reply", "strict": true, "schema": schema },
            });
        }

        let response: Value = self
            .client
            .post(format!(
                "http://127.0.0.1:{}/v1/chat/completions",
                self.port
            ))
            .json(&body)
            .send()
            .map_err(|e| Error::Other(format!("llama-server did not answer: {e}")))?
            .json()
            .map_err(|e| Error::Other(format!("llama-server sent something unreadable: {e}")))?;

        response["choices"][0]["message"]["content"]
            .as_str()
            .map(|s| s.trim().to_string())
            .ok_or_else(|| Error::Other("llama-server returned no content".into()))
    }
}

fn free_port() -> Result<u16> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    Ok(listener.local_addr()?.port())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_free_port_is_actually_free() {
        let port = free_port().unwrap();
        assert!(port > 0);
        // Binding it again proves it was released rather than held.
        std::net::TcpListener::bind(("127.0.0.1", port)).unwrap();
    }

    #[test]
    fn asking_an_unreachable_server_fails_rather_than_hanging() {
        let server = LlamaServer {
            port: free_port().unwrap(),
            model_name: "test".into(),
            child: Mutex::new(None),
            client: reqwest::blocking::Client::builder()
                .timeout(Duration::from_millis(300))
                .build()
                .unwrap(),
        };
        assert!(!server.ready());
        assert!(server.ask(Ask::new("s", "u")).is_err());
    }

    /// Both other tests use `child: None`, so nothing proved a live process is
    /// actually killed -- which is the whole purpose of the file. A real child
    /// stands in for llama-server here.
    #[test]
    fn stopping_kills_a_real_child() {
        // Something that would otherwise outlive the test.
        let child = Command::new("cmd")
            .args(["/C", "ping -n 60 127.0.0.1 > nul"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("could not spawn a stand-in child");
        let pid = child.id();

        let server = LlamaServer {
            port: free_port().unwrap(),
            model_name: "test".into(),
            child: Mutex::new(Some(child)),
            client: reqwest::blocking::Client::new(),
        };
        server.stop();

        assert!(
            server.child.lock().unwrap().is_none(),
            "the handle is released so a second stop is a no-op"
        );
        // Reaped, not merely killed: a zombie still holds a process slot.
        let still_running = std::process::Command::new("tasklist")
            .args(["/FI", &format!("PID eq {pid}")])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).contains(&pid.to_string()))
            .unwrap_or(false);
        assert!(!still_running, "process {pid} outlived stop()");
    }

    #[test]
    fn stopping_twice_is_harmless() {
        let server = LlamaServer {
            port: free_port().unwrap(),
            model_name: "test".into(),
            child: Mutex::new(None),
            client: reqwest::blocking::Client::new(),
        };
        server.stop();
        server.stop();
    }
}
