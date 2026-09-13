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
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

const CHUNK: usize = 64 * 1024;

/// The share of the line a yielding download may take while one that gates
/// something is still running, as a divisor: 50 is one fiftieth.
///
/// A share rather than a byte rate because the denominator is unknown. 64KB/s
/// is a trickle on fibre and the entire line on a 2Mbps connection, so a fixed
/// rate would starve exactly the machines that can least afford it.
const BACKGROUND_SHARE: u32 = 50;

/// A window ends at whichever of these comes first. Bytes so that per-chunk
/// buffering averages out before anything is decided from the timing; time so
/// that a slow line does not spend minutes measuring one window, and so the
/// idle it implies stays short enough not to look like a stall.
const WINDOW_BYTES: u64 = 256 * 1024;
const WINDOW_TIME: Duration = Duration::from_millis(300);

/// Idle in slices, rechecking between them. The whole point is that finishing
/// the download which gates recording hands the line over immediately; one long
/// sleep would leave it idle for up to a minute after there was nothing left to
/// yield to.
const YIELD_SLICE: Duration = Duration::from_millis(200);

/// How many downloads that gate something are in flight.
///
/// Shared rather than passed, because the thing being expressed is one download
/// observing another: §9.4 makes transcription gate recording and leaves the
/// reasoning model free to arrive late, so they are not peers competing for the
/// same line.
#[derive(Debug, Default)]
pub struct Gate(AtomicUsize);

/// Held for as long as a gating download runs. A guard rather than a pair of
/// calls so that an early return or an error still releases the line.
pub struct Busy(Arc<Gate>);

impl Gate {
    pub fn new() -> Arc<Self> {
        Arc::new(Self(AtomicUsize::new(0)))
    }

    fn enter(self: &Arc<Self>) -> Busy {
        self.0.fetch_add(1, Ordering::SeqCst);
        Busy(self.clone())
    }

    fn busy(&self) -> bool {
        self.0.load(Ordering::SeqCst) > 0
    }
}

impl Drop for Busy {
    fn drop(&mut self) {
        self.0 .0.fetch_sub(1, Ordering::SeqCst);
    }
}

/// What a download is allowed to do to the line.
#[derive(Clone)]
pub enum Pace {
    /// Takes the line, and is what background downloads yield to.
    Now(Arc<Gate>),
    /// Yields while any `Now` download is in flight, then takes the whole line.
    /// The divisor is carried rather than read from the constant so a test can
    /// make the effect unmistakable without waiting for a real one.
    WhenIdle(Arc<Gate>, u32),
}

impl Pace {
    pub fn now(gate: Arc<Gate>) -> Self {
        Pace::Now(gate)
    }

    pub fn when_idle(gate: Arc<Gate>) -> Self {
        Pace::WhenIdle(gate, BACKGROUND_SHARE)
    }
}

/// How long a download must stay idle to hold `1/share` of the line, given a
/// window that took `took` to read.
///
/// Idling is what actually releases the bandwidth: not reading closes the
/// receive window and stalls the sender, where merely reading more slowly would
/// not. The first window after a pause is served partly from whatever was
/// already in flight, which over gigabytes is a rounding error.
fn idle_after(took: Duration, share: u32) -> Duration {
    took * share.saturating_sub(1)
}

pub fn part_path(dest: &Path) -> PathBuf {
    let mut name = dest.file_name().unwrap_or_default().to_os_string();
    name.push(".part");
    dest.with_file_name(name)
}

/// Downloads to `dest`, resuming any `.part` beside it. `on_progress` is called
/// with bytes received and the expected total.
pub fn fetch(url: &str, dest: &Path, on_progress: &mut dyn FnMut(u64, u64)) -> Result<()> {
    fetch_paced(url, dest, on_progress, Pace::now(Gate::new()))
}

/// `fetch`, taking the share of the line this download is entitled to.
pub fn fetch_paced(
    url: &str,
    dest: &Path,
    on_progress: &mut dyn FnMut(u64, u64),
    pace: Pace,
) -> Result<()> {
    // Held for the whole fetch, including every early return below: a gating
    // download that failed is no longer gating anything.
    let _busy = match &pace {
        Pace::Now(gate) => Some(gate.enter()),
        Pace::WhenIdle(..) => None,
    };
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
    let mut window = Window::new();
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
        // Before the idle, not after: a bar that only moves once the wait is
        // over is the state this exists to remove.
        on_progress(received, total);
        window.paced(n as u64, &pace);
    }
    file.flush()?;
    drop(file);

    // Only now is it a model rather than a prefix of one.
    std::fs::rename(&part, dest)?;
    Ok(())
}

/// Measures a window of reading and then idles for the rest of its share.
struct Window {
    started: Instant,
    bytes: u64,
}

impl Window {
    fn new() -> Self {
        Self {
            started: Instant::now(),
            bytes: 0,
        }
    }

    fn paced(&mut self, read: u64, pace: &Pace) {
        let Pace::WhenIdle(gate, share) = pace else {
            return;
        };
        self.bytes += read;
        if self.bytes < WINDOW_BYTES && self.started.elapsed() < WINDOW_TIME {
            return;
        }
        // Checked here rather than on entry so an open gate still closes the
        // window: the next one then measures fresh timings instead of charging
        // this download for however long it spent unthrottled.
        if gate.busy() {
            wait_out(idle_after(self.started.elapsed(), *share), gate);
        }
        self.bytes = 0;
        self.started = Instant::now();
    }
}

/// Idles, but stops the moment there is nothing left to yield to.
fn wait_out(mut remaining: Duration, gate: &Gate) {
    while remaining > Duration::ZERO && gate.busy() {
        let slice = remaining.min(YIELD_SLICE);
        std::thread::sleep(slice);
        remaining -= slice;
    }
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

    // -- pacing -------------------------------------------------------------

    /// The arithmetic the whole scheme rests on. Holding a fiftieth of the line
    /// means being idle for forty-nine times as long as reading took.
    #[test]
    fn idling_is_the_share_of_whatever_the_line_delivered() {
        assert_eq!(
            idle_after(Duration::from_millis(10), 50),
            Duration::from_millis(490)
        );
        assert_eq!(
            idle_after(Duration::from_millis(10), 2),
            Duration::from_millis(10)
        );
        // A share of one is the whole line, so there is nothing to wait out.
        assert_eq!(idle_after(Duration::from_millis(10), 1), Duration::ZERO);
        assert_eq!(idle_after(Duration::from_millis(10), 0), Duration::ZERO);
    }

    /// Scales with the line rather than against a fixed rate: a window that took
    /// ten times as long to read buys ten times the idle, so the share is the
    /// same on fibre and on a 2Mbps link.
    #[test]
    fn the_share_is_of_the_line_not_a_fixed_rate() {
        let fast = idle_after(Duration::from_millis(10), BACKGROUND_SHARE);
        let slow = idle_after(Duration::from_millis(100), BACKGROUND_SHARE);
        assert_eq!(slow, fast * 10);
    }

    #[test]
    fn the_gate_is_busy_only_while_something_holds_it() {
        let gate = Gate::new();
        assert!(!gate.busy());
        let held = gate.enter();
        assert!(gate.busy());
        drop(held);
        assert!(
            !gate.busy(),
            "the gate stayed shut after the download finished"
        );
    }

    /// Speech and the embedder both gate, and they overlap at the handover.
    /// Counting rather than flagging is what stops the first one to finish from
    /// opening the line while the second is still running.
    #[test]
    fn the_gate_counts_rather_than_flags() {
        let gate = Gate::new();
        let speech = gate.enter();
        let embedder = gate.enter();
        drop(speech);
        assert!(
            gate.busy(),
            "the line opened while the embedder was still running"
        );
        drop(embedder);
        assert!(!gate.busy());
    }

    /// A foreground download must never wait on the gate it is itself holding.
    #[test]
    fn a_gating_download_does_not_yield_to_itself() {
        let want = body(600_000);
        let server = Server::start(want.clone(), false);
        let dest = temp_dest("pace-foreground");
        let gate = Gate::new();

        let started = Instant::now();
        fetch_paced(
            &server.url(),
            &dest,
            &mut |_, _| {},
            Pace::now(gate.clone()),
        )
        .unwrap();

        assert_eq!(std::fs::read(&dest).unwrap(), want);
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "a foreground download paced itself: {:?}",
            started.elapsed()
        );
        assert!(
            !gate.busy(),
            "the gate was not released when the fetch returned"
        );
    }

    /// Nothing to yield to, so a background download takes the whole line.
    #[test]
    fn a_background_download_is_unpaced_when_nothing_gates() {
        let want = body(600_000);
        let server = Server::start(want.clone(), false);
        let dest = temp_dest("pace-idle");
        let gate = Gate::new();

        let started = Instant::now();
        fetch_paced(
            &server.url(),
            &dest,
            &mut |_, _| {},
            Pace::WhenIdle(gate, 5_000),
        )
        .unwrap();

        assert_eq!(std::fs::read(&dest).unwrap(), want);
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "it paced itself against an open gate: {:?}",
            started.elapsed()
        );
    }

    /// The behaviour asked for: the 5GB download starts immediately and gets out
    /// of the way of the one that gates recording.
    #[test]
    fn a_background_download_yields_while_something_gates() {
        let want = body(600_000);
        let server = Server::start(want.clone(), false);
        let dest = temp_dest("pace-yield");
        let gate = Gate::new();
        let _held = gate.enter();

        let started = Instant::now();
        fetch_paced(
            &server.url(),
            &dest,
            &mut |_, _| {},
            // Extreme share so the wait is unmistakable rather than a margin.
            Pace::WhenIdle(gate.clone(), 5_000),
        )
        .unwrap();

        assert_eq!(
            std::fs::read(&dest).unwrap(),
            want,
            "throttling corrupted the file"
        );
        assert!(
            started.elapsed() > Duration::from_millis(400),
            "it did not yield at all: {:?}",
            started.elapsed()
        );
    }

    /// "If speech finishes, we go full bandwidth" — and without waiting out the
    /// idle it had already committed to.
    #[test]
    fn clearing_the_gate_hands_the_line_over_immediately() {
        let want = body(600_000);
        let server = Server::start(want.clone(), false);
        let dest = temp_dest("pace-release");
        let gate = Gate::new();
        let held = gate.enter();

        // Long enough that a download which waited out its full idle could not
        // possibly finish inside the assertion below.
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(300));
            drop(held);
        });

        let started = Instant::now();
        fetch_paced(
            &server.url(),
            &dest,
            &mut |_, _| {},
            Pace::WhenIdle(gate, 100_000),
        )
        .unwrap();
        let took = started.elapsed();

        assert_eq!(std::fs::read(&dest).unwrap(), want);
        assert!(
            took < Duration::from_secs(3),
            "it kept yielding after the gate cleared: {:?}",
            took
        );
    }

    /// Progress is what the onboarding bar reads, and a throttled download still
    /// has to report it — a bar that only moves at the end is the state this
    /// whole change exists to remove.
    #[test]
    fn a_throttled_download_still_reports_progress_as_it_goes() {
        let want = body(600_000);
        let server = Server::start(want.clone(), false);
        let dest = temp_dest("pace-progress");
        let gate = Gate::new();
        let _held = gate.enter();

        let mut seen: Vec<u64> = Vec::new();
        fetch_paced(
            &server.url(),
            &dest,
            &mut |got, _| seen.push(got),
            Pace::WhenIdle(gate, 2_000),
        )
        .unwrap();

        let partial = seen
            .iter()
            .filter(|&&g| g > 0 && g < want.len() as u64)
            .count();
        assert!(
            partial > 0,
            "no progress was reported before the end: {seen:?}"
        );
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
