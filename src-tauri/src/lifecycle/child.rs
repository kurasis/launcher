//! Supervision of one owned child process (docs/spec/05 "Process supervision").
//!
//! stdout and stderr are read concurrently on their own threads so the child can
//! never block on a full pipe. Captured output is bounded. After the child exits,
//! remaining pipe data is drained for at most [`DRAIN_LIMIT`]; a grandchild that
//! inherited the pipes cannot delay the result indefinitely. The child handle is
//! owned by the supervisor thread, never by a window, so closing the UI cannot drop
//! or kill it.

use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Read};
use std::process::{Child, ChildStderr, ChildStdout, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

/// Maximum time to wait for pipe EOF after the child has exited.
pub const DRAIN_LIMIT: Duration = Duration::from_secs(2);
/// Maximum bytes kept from a single line; longer lines are truncated with a marker.
pub const MAX_LINE_BYTES: usize = 64 * 1024;
/// Lines kept in the in-memory tail.
pub const TAIL_LINES: usize = 2_000;
/// Bytes kept in the in-memory tail.
pub const TAIL_BYTES: usize = 2 * 1024 * 1024;

pub const TRUNCATION_MARKER: &str = " …[line truncated]";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stream {
    Stdout,
    Stderr,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputLine {
    pub stream: Stream,
    pub text: String,
}

/// A bounded tail of captured lines. Oldest lines are dropped first.
#[derive(Debug, Default)]
pub struct OutputTail {
    lines: VecDeque<OutputLine>,
    bytes: usize,
    dropped: u64,
}

impl OutputTail {
    pub fn push(&mut self, line: OutputLine) {
        self.bytes += line.text.len();
        self.lines.push_back(line);
        while self.lines.len() > TAIL_LINES || self.bytes > TAIL_BYTES {
            match self.lines.pop_front() {
                Some(old) => {
                    self.bytes -= old.text.len();
                    self.dropped += 1;
                }
                None => break,
            }
        }
    }

    pub fn lines(&self) -> impl Iterator<Item = &OutputLine> {
        self.lines.iter()
    }

    pub fn len(&self) -> usize {
        self.lines.len()
    }

    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    /// Number of lines dropped from the front because of the bounds.
    pub fn dropped(&self) -> u64 {
        self.dropped
    }
}

/// Receives each captured line; used for redaction and log sinks (P09).
pub type LineSink = Box<dyn FnMut(OutputLine) + Send>;

/// Final result of a supervised child.
#[derive(Debug)]
pub struct ChildOutcome {
    pub status: Result<ExitStatus, String>,
    pub duration: Duration,
    /// True when pipes were still open after [`DRAIN_LIMIT`] and capture was closed.
    pub drain_timed_out: bool,
}

/// Splits a byte stream into lines with a hard per-line bound. Invalid UTF-8 is
/// replaced, never rejected.
fn read_lines(reader: impl Read, stream: Stream, mut emit: impl FnMut(OutputLine)) {
    let mut reader = BufReader::with_capacity(16 * 1024, reader);
    let mut line = Vec::with_capacity(256);
    let mut truncated = false;
    loop {
        let buffer = match reader.fill_buf() {
            Ok([]) => break,
            Ok(buffer) => buffer,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => break,
        };
        let (consumed, newline) = match buffer.iter().position(|b| *b == b'\n') {
            Some(index) => (index + 1, true),
            None => (buffer.len(), false),
        };
        let content = &buffer[..if newline { consumed - 1 } else { consumed }];
        let room = MAX_LINE_BYTES.saturating_sub(line.len());
        if content.len() > room {
            line.extend_from_slice(&content[..room]);
            truncated = true;
        } else {
            line.extend_from_slice(content);
        }
        reader.consume(consumed);
        if newline {
            emit(finish_line(&mut line, &mut truncated, stream));
        }
    }
    if !line.is_empty() || truncated {
        emit(finish_line(&mut line, &mut truncated, stream));
    }
}

fn finish_line(line: &mut Vec<u8>, truncated: &mut bool, stream: Stream) -> OutputLine {
    if line.last() == Some(&b'\r') {
        line.pop();
    }
    let mut text = String::from_utf8_lossy(line).into_owned();
    if *truncated {
        text.push_str(TRUNCATION_MARKER);
    }
    line.clear();
    *truncated = false;
    OutputLine { stream, text }
}

/// A running supervised child. Dropping this value does not kill the child.
pub struct Supervised {
    pid: u32,
    started: Instant,
    kill_tx: mpsc::Sender<()>,
    exited: Arc<AtomicBool>,
}

impl Supervised {
    pub fn pid(&self) -> u32 {
        self.pid
    }

    pub fn started(&self) -> Instant {
        self.started
    }

    /// Requests termination of this exact owned child. Never targets other
    /// processes. Returns false once the child has exited.
    pub fn kill(&self) -> bool {
        !self.exited.load(Ordering::SeqCst) && self.kill_tx.send(()).is_ok()
    }
}

/// Spawns `command` with piped output and supervises it on background threads.
/// `on_process_exit` runs as soon as the process has exited, before the output
/// drain; `on_exit` runs exactly once afterwards with the final outcome. Both run
/// on the supervisor thread.
pub fn spawn(
    mut command: Command,
    sink: LineSink,
    on_process_exit: impl FnOnce() + Send + 'static,
    on_exit: impl FnOnce(ChildOutcome) + Send + 'static,
) -> std::io::Result<Supervised> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let started = Instant::now();
    let mut child = command.spawn()?;
    let pid = child.id();
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let (kill_tx, kill_rx) = mpsc::channel::<()>();
    let exited = Arc::new(AtomicBool::new(false));
    let exited_flag = Arc::clone(&exited);

    let sink = Arc::new(Mutex::new(sink));
    let (done_tx, done_rx) = mpsc::channel::<()>();
    let readers = start_readers(stdout, stderr, &sink, &done_tx);
    drop(done_tx);

    thread::Builder::new()
        .name(format!("child-{pid}"))
        .spawn(move || {
            let status = wait_or_kill(&mut child, &kill_rx);
            exited_flag.store(true, Ordering::SeqCst);
            on_process_exit();
            let deadline = Instant::now() + DRAIN_LIMIT;
            let mut finished = 0;
            let mut drain_timed_out = false;
            while finished < readers {
                let remaining = deadline.saturating_duration_since(Instant::now());
                match done_rx.recv_timeout(remaining) {
                    Ok(()) => finished += 1,
                    Err(mpsc::RecvTimeoutError::Timeout) => {
                        drain_timed_out = true;
                        break;
                    }
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                }
            }
            on_exit(ChildOutcome {
                status,
                duration: started.elapsed(),
                drain_timed_out,
            });
        })?;

    Ok(Supervised {
        pid,
        started,
        kill_tx,
        exited,
    })
}

fn start_readers(
    stdout: Option<ChildStdout>,
    stderr: Option<ChildStderr>,
    sink: &Arc<Mutex<LineSink>>,
    done: &mpsc::Sender<()>,
) -> usize {
    let mut count = 0;
    let mut start = |reader: Box<dyn Read + Send>, stream: Stream| {
        let sink = Arc::clone(sink);
        let done = done.clone();
        let spawned = thread::Builder::new()
            .name(format!("child-{stream:?}").to_lowercase())
            .spawn(move || {
                read_lines(reader, stream, |line| {
                    let mut sink = sink.lock().unwrap_or_else(PoisonError::into_inner);
                    (sink)(line);
                });
                let _ = done.send(());
            });
        if spawned.is_ok() {
            count += 1;
        }
    };
    if let Some(stdout) = stdout {
        start(Box::new(stdout), Stream::Stdout);
    }
    if let Some(stderr) = stderr {
        start(Box::new(stderr), Stream::Stderr);
    }
    count
}

/// Waits for exit, polling only to notice an explicit kill request. The poll
/// interval is short enough for a responsive Stop and cheap enough to be idle.
fn wait_or_kill(child: &mut Child, kill_rx: &mpsc::Receiver<()>) -> Result<ExitStatus, String> {
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status),
            Ok(None) => {}
            Err(error) => return Err(format!("{:?}", error.kind())),
        }
        match kill_rx.recv_timeout(Duration::from_millis(250)) {
            Ok(()) => {
                let _ = child.kill();
                return child.wait().map_err(|e| format!("{:?}", e.kind()));
            }
            Err(mpsc::RecvTimeoutError::Timeout | mpsc::RecvTimeoutError::Disconnected) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_are_split_bounded_and_lossy() {
        let mut long = vec![b'x'; MAX_LINE_BYTES + 10];
        long.extend_from_slice(b"\nnext\r\n\xff\xfe tail");
        let mut lines = Vec::new();
        read_lines(&long[..], Stream::Stdout, |l| lines.push(l.text));
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0].len(), MAX_LINE_BYTES + TRUNCATION_MARKER.len());
        assert!(lines[0].ends_with(TRUNCATION_MARKER));
        assert_eq!(lines[1], "next");
        assert!(lines[2].ends_with(" tail"));
    }

    #[test]
    fn tail_is_bounded_by_lines() {
        let mut tail = OutputTail::default();
        for i in 0..(TAIL_LINES + 5) {
            tail.push(OutputLine {
                stream: Stream::Stdout,
                text: format!("{i}"),
            });
        }
        assert_eq!(tail.len(), TAIL_LINES);
        assert_eq!(tail.dropped(), 5);
        assert_eq!(tail.lines().next().unwrap().text, "5");
    }

    #[test]
    fn tail_is_bounded_by_bytes() {
        let mut tail = OutputTail::default();
        let big = "y".repeat(MAX_LINE_BYTES);
        for _ in 0..40 {
            tail.push(OutputLine {
                stream: Stream::Stderr,
                text: big.clone(),
            });
        }
        assert!(tail.lines().map(|l| l.text.len()).sum::<usize>() <= TAIL_BYTES);
        assert!(tail.dropped() > 0);
    }
}
