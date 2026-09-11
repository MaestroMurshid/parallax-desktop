//! Fetching a model file.
//!
//! Resumable because 2.5GB over a home connection gets interrupted, and written
//! to `.part` until complete because `list_models` reads size off disk and a
//! truncated gguf loads as the app being broken rather than the file being half
//! there.

use crate::error::{Error, Result};
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

const CHUNK: usize = 64 * 1024;

pub fn part_path(dest: &Path) -> PathBuf {
    let mut name = dest.file_name().unwrap_or_default().to_os_string();
    name.push(".part");
    dest.with_file_name(name)
}

/// Downloads to `dest`, resuming any `.part` beside it. `on_progress` is called
/// with bytes received and the expected total.
pub fn fetch(url: &str, dest: &Path, on_progress: &mut dyn FnMut(u64, u64)) -> Result<()> {
    if let Some(dir) = dest.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let part = part_path(dest);
    let have = std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0);

    // No total timeout: 2.5GB on a slow line is not a hung request. Only the
    // connect is bounded, so an unreachable host still fails quickly.
    let client = reqwest::blocking::Client::builder()
        .timeout(None)
        .connect_timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| Error::Other(format!("could not start the download: {e}")))?;

    let mut request = client.get(url);
    if have > 0 {
        request = request.header(reqwest::header::RANGE, format!("bytes={have}-"));
    }
    let mut response = request
        .send()
        .map_err(|e| Error::Other(format!("could not reach {url}: {e}")))?;
    if !response.status().is_success() {
        return Err(Error::Other(format!(
            "{url} answered {}",
            response.status()
        )));
    }

    // 206 means the range was honoured. 200 means it was not, and appending
    // would write the whole file onto the end of a partial one.
    let resumed = response.status() == reqwest::StatusCode::PARTIAL_CONTENT;
    let already = if resumed { have } else { 0 };
    let total = response.content_length().unwrap_or(0) + already;

    let mut file = if resumed {
        let mut f = OpenOptions::new().write(true).open(&part)?;
        f.seek(SeekFrom::End(0))?;
        f
    } else {
        File::create(&part)?
    };

    let mut received = already;
    let mut buf = vec![0u8; CHUNK];
    on_progress(received, total);
    loop {
        let n = response
            .read(&mut buf)
            .map_err(|e| Error::Other(format!("the download stopped: {e}")))?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n])?;
        received += n as u64;
        on_progress(received, total);
    }
    file.flush()?;
    drop(file);

    // Only now is it a model rather than a prefix of one.
    std::fs::rename(&part, dest)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader};
    use std::net::{TcpListener, TcpStream};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    /// Enough HTTP to answer one request, so the resume logic is tested against
    /// a socket rather than a mock of one.
    struct Server {
        port: u16,
        hits: Arc<std::sync::Mutex<Vec<String>>>,
        stop: Arc<AtomicBool>,
    }

    impl Server {
        /// `ignore_range` reproduces a server that answers 200 with the whole
        /// body even when asked for a range.
        fn start(body: Vec<u8>, ignore_range: bool) -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let port = listener.local_addr().unwrap().port();
            let hits = Arc::new(std::sync::Mutex::new(Vec::new()));
            let stop = Arc::new(AtomicBool::new(false));
            let (h, s) = (hits.clone(), stop.clone());

            std::thread::spawn(move || {
                for conn in listener.incoming() {
                    if s.load(Ordering::Relaxed) {
                        break;
                    }
                    let Ok(mut conn) = conn else { break };
                    let range = read_request(&mut conn, &h);
                    let from = if ignore_range { 0 } else { range.unwrap_or(0) };
                    let slice = &body[(from as usize).min(body.len())..];

                    let head = if from > 0 && !ignore_range {
                        format!(
                            "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {}-{}/{}\r\n\r\n",
                            slice.len(), from, body.len() - 1, body.len()
                        )
                    } else {
                        format!(
                            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nAccept-Ranges: bytes\r\n\r\n",
                            slice.len()
                        )
                    };
                    let _ = conn.write_all(head.as_bytes());
                    let _ = conn.write_all(slice);
                    let _ = conn.flush();
                }
            });

            Self { port, hits, stop }
        }

        fn url(&self) -> String {
            format!("http://127.0.0.1:{}/model.gguf", self.port)
        }

        fn range_headers(&self) -> Vec<String> {
            self.hits.lock().unwrap().clone()
        }
    }

    impl Drop for Server {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Relaxed);
            let _ = TcpStream::connect(("127.0.0.1", self.port));
        }
    }

    fn read_request(conn: &mut TcpStream, hits: &std::sync::Mutex<Vec<String>>) -> Option<u64> {
        let peek = conn.try_clone().unwrap();
        let mut reader = BufReader::new(peek);
        let mut range = None;
        loop {
            let mut line = String::new();
            if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                break;
            }
            if let Some(value) = line.to_lowercase().strip_prefix("range:") {
                hits.lock().unwrap().push(line.trim().to_string());
                range = value
                    .trim()
                    .strip_prefix("bytes=")
                    .and_then(|r| r.split('-').next())
                    .and_then(|n| n.trim().parse().ok());
            }
        }
        range
    }

    fn temp_dest(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("parallax-dl-{tag}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("whisper-tiny.gguf")
    }

    fn body(len: usize) -> Vec<u8> {
        (0..len).map(|i| (i % 251) as u8).collect()
    }

    #[test]
    fn a_download_writes_the_whole_file() {
        let want = body(200_000);
        let server = Server::start(want.clone(), false);
        let dest = temp_dest("whole");

        fetch(&server.url(), &dest, &mut |_, _| {}).unwrap();

        assert_eq!(std::fs::read(&dest).unwrap(), want);
        assert!(!part_path(&dest).exists(), "the .part should be gone");
    }

    #[test]
    fn progress_reports_received_and_total() {
        let want = body(200_000);
        let server = Server::start(want.clone(), false);
        let dest = temp_dest("progress");

        let mut seen: Vec<(u64, u64)> = Vec::new();
        fetch(&server.url(), &dest, &mut |got, total| {
            seen.push((got, total))
        })
        .unwrap();

        assert!(!seen.is_empty(), "progress was never reported");
        assert!(seen.iter().all(|(_, total)| *total == want.len() as u64));
        assert_eq!(seen.last().unwrap().0, want.len() as u64);
        assert!(
            seen.windows(2).all(|w| w[0].0 <= w[1].0),
            "progress went backwards"
        );
    }

    /// The case that matters for 2.5GB: half a file already on disk.
    #[test]
    fn a_partial_download_resumes() {
        let want = body(200_000);
        let server = Server::start(want.clone(), false);
        let dest = temp_dest("resume");
        std::fs::write(part_path(&dest), &want[..80_000]).unwrap();

        let mut first_total = 0;
        fetch(&server.url(), &dest, &mut |got, total| {
            if first_total == 0 {
                first_total = total;
                assert!(got >= 80_000, "resume should start from what is on disk");
            }
        })
        .unwrap();

        assert_eq!(std::fs::read(&dest).unwrap(), want);
        assert_eq!(first_total, want.len() as u64, "total is the whole file");
        assert!(
            server.range_headers().iter().any(|h| h.contains("80000")),
            "no range header was sent: {:?}",
            server.range_headers()
        );
    }

    /// A server free to ignore `Range`. Appending to the part file would corrupt
    /// it, so the download starts over.
    #[test]
    fn a_server_that_ignores_range_restarts() {
        let want = body(120_000);
        let server = Server::start(want.clone(), true);
        let dest = temp_dest("ignored");
        std::fs::write(part_path(&dest), &want[..50_000]).unwrap();

        fetch(&server.url(), &dest, &mut |_, _| {}).unwrap();

        assert_eq!(
            std::fs::read(&dest).unwrap(),
            want,
            "the file was appended to instead of restarted"
        );
    }

    #[test]
    fn an_already_complete_part_file_still_finishes() {
        let want = body(60_000);
        let server = Server::start(want.clone(), false);
        let dest = temp_dest("complete-part");
        std::fs::write(part_path(&dest), &want).unwrap();

        fetch(&server.url(), &dest, &mut |_, _| {}).unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), want);
    }

    #[test]
    fn an_unreachable_host_is_an_error_not_a_file() {
        let dest = temp_dest("unreachable");
        // Port 1 on loopback: nothing listens there.
        let failed = fetch("http://127.0.0.1:1/model.gguf", &dest, &mut |_, _| {});
        assert!(failed.is_err());
        assert!(
            !dest.exists(),
            "a failure must not leave a destination file"
        );
    }
}
