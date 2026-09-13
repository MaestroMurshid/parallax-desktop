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

/// Enough for compute buffers and the desktop, and little enough that a 4GB
/// card still takes the whole model.
const FIT_MARGIN_MIB: u32 = 256;

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
        device: Option<&str>,
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

        // -ngl is left unset on purpose: it defaults to `auto` and `--fit on`
        // then sizes the offload to the memory actually free, keeping a margin.
        // Forcing `99` overrode that and ran out mid-answer on a 4GB card.
        match backend {
            ComputeBackend::Cpu => {
                command.arg("-ngl").arg("0");
            }
            _ => {
                // Without a device, offload lands on the first one, which on a
                // laptop is the integrated GPU and its shared system RAM.
                if let Some(id) = device {
                    command.arg("--device").arg(id);
                }
                // --fit keeps 1GiB per device free by default, which is a
                // quarter of a 4GB card: 2.3GiB of weights plus a 576MiB KV
                // cache then does not fit, so it offloads part of the model and
                // runs the rest on CPU. Measured, that costs about 10x.
                command.arg("--fit-target").arg(FIT_MARGIN_MIB.to_string());
            }
        }

        let child = super::without_a_console(&mut command)
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
            // Qwen3 thinks by default, and measured it spent all 400 tokens
            // doing it: finish_reason was length, reasoning_content held 2kB,
            // and content -- the only thing the grammar applies to -- was empty.
            // Off, the same call answers in 94 tokens and 3.5s rather than 8.5s.
            // Templates without the flag ignore it.
            "chat_template_kwargs": { "enable_thinking": false },
        });

        if let Some(max) = ask.max_tokens {
            body["max_tokens"] = json!(max);
        }

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

        // Its own words when it refuses. "Returned no content" was all an
        // overflowing request ever said, which reads as the model misbehaving
        // rather than the prompt being too long for it.
        if let Some(message) = response["error"]["message"].as_str() {
            return Err(Error::Other(format!(
                "llama-server refused the request: {message}"
            )));
        }
        response["choices"][0]["message"]["content"]
            .as_str()
            .map(|s| s.trim().to_string())
            .ok_or_else(|| Error::Other("llama-server returned no content".into()))
    }
}

pub(crate) fn free_port() -> Result<u16> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    Ok(listener.local_addr()?.port())
}

/// A real child standing in for llama-server, for tests above this module that
/// need a server to stop without loading a model.
#[cfg(test)]
pub(crate) fn stand_in(child: Child) -> LlamaServer {
    LlamaServer {
        port: 0,
        model_name: "stand-in".into(),
        child: Mutex::new(Some(child)),
        client: reqwest::blocking::Client::new(),
    }
}

/// Whether a running process is a model server this app left behind.
///
/// Both conditions, because each alone kills the wrong thing: a live parent is
/// a second instance still using its servers, and a model outside our own
/// folder is a llama-server the user runs for reasons of their own.
fn is_orphan(cmd: &[String], models_dir: &std::path::Path, parent_alive: bool) -> bool {
    !parent_alive
        && cmd
            .iter()
            .any(|arg| std::path::Path::new(arg).starts_with(models_dir))
}

/// Stops model servers a previous run of this app left running.
///
/// `shutdown` covers every ordinary quit, but a crash or an End Task runs no
/// handler at all. Measured in the packaged app: killed hard, it left both
/// servers up, holding 2.6GB of RAM and most of a 4GB card, and the next launch
/// had to fit its model beside them -- which on this hardware means the CPU and
/// roughly a tenth of the speed. Swept at startup, before anything spawns.
pub fn reap_orphans(models_dir: &std::path::Path) {
    use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

    let mut system = System::new();
    system.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing().with_cmd(UpdateKind::Always),
    );
    for process in system.processes().values() {
        let name = process.name().to_string_lossy().to_lowercase();
        if !name.starts_with("llama-server") {
            continue;
        }
        let parent_alive = process
            .parent()
            .is_some_and(|pid| system.process(pid).is_some());
        let cmd: Vec<String> = process
            .cmd()
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        if is_orphan(&cmd, models_dir, parent_alive) && process.kill() {
            println!(
                "stopped a model server left by a previous run: {}",
                process.pid()
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cmd(model: &str) -> Vec<String> {
        ["llama-server.exe", "-m", model, "--port", "2131"]
            .iter()
            .map(|s| s.to_string())
            .collect()
    }

    /// Measured in the packaged app: killed hard, it left both servers running
    /// -- 2.6GB of RAM and most of a 4GB card -- and the next launch had to fit
    /// its model beside a server nobody owned.
    #[test]
    fn a_server_on_our_models_with_a_dead_parent_is_an_orphan() {
        let models = std::path::Path::new("E:/root/data/models");
        assert!(is_orphan(
            &cmd("E:/root/data/models/qwen3-4b-q4.gguf"),
            models,
            false
        ));
    }

    /// A second instance of the app is still using its servers.
    #[test]
    fn a_server_whose_parent_is_alive_is_left_running() {
        let models = std::path::Path::new("E:/root/data/models");
        assert!(!is_orphan(
            &cmd("E:/root/data/models/qwen3-4b-q4.gguf"),
            models,
            true
        ));
    }

    /// The one outcome that must never happen: killing a llama-server the user
    /// runs themselves, against their own models, for their own reasons.
    #[test]
    fn someone_elses_llama_server_is_never_touched() {
        let models = std::path::Path::new("E:/root/data/models");
        assert!(!is_orphan(&cmd("D:/my-models/mistral.gguf"), models, false));
        assert!(!is_orphan(&["llama-server.exe".to_string()], models, false));
    }

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
