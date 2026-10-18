//! Child supervision scenarios from docs/spec/05 and P03: immediate exit, crash
//! exit codes, large simultaneous stdout/stderr, a grandchild holding inherited
//! pipes open, and stopping exactly the owned child. The children are this test
//! binary re-invoked with an environment variable selecting the behaviour.

#![allow(clippy::print_stdout, clippy::print_stderr)] // the children write output
#![allow(clippy::unwrap_used)] // test helpers outside #[test] functions

use std::io::Write;
use std::process::Command;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use launcher_lib::lifecycle::child::{self, ChildOutcome, OutputLine, OutputTail, Stream};

const MODE_ENV: &str = "LAUNCHER_CHILD_PROBE";

/// Runs only inside a child process.
#[test]
fn probe_child() {
    let Ok(mode) = std::env::var(MODE_ENV) else {
        return;
    };
    let mut out = std::io::stdout();
    let mut err = std::io::stderr();
    match mode.as_str() {
        "exit7" => {
            println!("about to exit");
            std::process::exit(7);
        }
        "noisy" => {
            let line = "z".repeat(4095);
            for _ in 0..4096 {
                let _ = writeln!(out, "{line}");
                let _ = writeln!(err, "{line}");
            }
            let _ = out.flush();
            std::process::exit(0);
        }
        "hold-pipe" => {
            let _ = Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "probe_child", "--nocapture"])
                .env(MODE_ENV, "sleep")
                .spawn();
            println!("parent exiting");
            std::process::exit(0);
        }
        "sleep" => {
            std::thread::sleep(Duration::from_secs(8));
            std::process::exit(0);
        }
        "forever" => loop {
            std::thread::sleep(Duration::from_secs(1));
        },
        _ => std::process::exit(99),
    }
}

/// Tests run in parallel. Where the platform creates a pipe and marks it
/// close-on-exec in two steps (macOS), a child spawned by another test in
/// between inherits that pipe; a long-lived one (the `hold-pipe` grandchild, the
/// `forever` bystander) then keeps it open and the drain times out. Spawning one
/// child at a time closes that window.
static SPAWN: Mutex<()> = Mutex::new(());

fn spawning<T>(spawn: impl FnOnce() -> T) -> T {
    let _one_at_a_time = SPAWN
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    spawn()
}

fn probe(mode: &str) -> Command {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", "probe_child", "--nocapture", "--test-threads=1"])
        .env(MODE_ENV, mode);
    command
}

fn run(mode: &str) -> (ChildOutcome, Arc<Mutex<OutputTail>>, child::Supervised) {
    let tail = Arc::new(Mutex::new(OutputTail::default()));
    let sink = Arc::clone(&tail);
    let (tx, rx) = mpsc::channel();
    let (exited_tx, exited_rx) = mpsc::channel();
    let supervised = spawning(|| {
        child::spawn(
            probe(mode),
            Box::new(move |line: OutputLine| sink.lock().unwrap().push(line)),
            move || exited_tx.send(Instant::now()).unwrap(),
            move |outcome| tx.send((outcome, Instant::now())).unwrap(),
        )
    })
    .expect("spawn");
    let (outcome, finished) = rx.recv_timeout(Duration::from_secs(30)).expect("outcome");
    let exited = exited_rx.try_recv().expect("process exit reported first");
    assert!(exited <= finished);
    (outcome, tail, supervised)
}

#[test]
fn immediate_exit_code_is_reported() {
    let (outcome, tail, supervised) = run("exit7");
    assert_eq!(outcome.status.unwrap().code(), Some(7));
    assert!(!outcome.drain_timed_out);
    assert!(
        tail.lock()
            .unwrap()
            .lines()
            .any(|l| l.stream == Stream::Stdout && l.text.contains("about to exit"))
    );
    assert!(supervised.pid() > 0);
    assert!(!supervised.kill(), "supervisor already finished");
}

#[test]
fn large_simultaneous_output_never_blocks_and_stays_bounded() {
    let started = Instant::now();
    let (outcome, tail, _) = run("noisy");
    assert!(outcome.status.unwrap().success());
    let tail = tail.lock().unwrap();
    assert!(tail.len() <= child::TAIL_LINES);
    assert!(tail.dropped() > 0);
    assert!(tail.lines().any(|l| l.stream == Stream::Stderr));
    assert!(started.elapsed() < Duration::from_secs(20));
}

#[test]
fn inherited_pipes_do_not_delay_the_result_beyond_the_drain_limit() {
    let started = Instant::now();
    let (outcome, _, _) = run("hold-pipe");
    assert!(outcome.status.unwrap().success());
    assert!(outcome.drain_timed_out, "grandchild keeps the pipes open");
    // Exit is almost immediate; the result must arrive after about DRAIN_LIMIT,
    // long before the grandchild's 8 seconds.
    assert!(started.elapsed() < child::DRAIN_LIMIT + Duration::from_secs(3));
}

#[test]
fn kill_stops_only_the_owned_child() {
    let tail = Arc::new(Mutex::new(OutputTail::default()));
    let sink = Arc::clone(&tail);
    let (tx, rx) = mpsc::channel();
    let bystander = spawning(|| probe("forever").spawn()).expect("bystander");
    let supervised = spawning(|| {
        child::spawn(
            probe("forever"),
            Box::new(move |line: OutputLine| sink.lock().unwrap().push(line)),
            || {},
            move |outcome| tx.send(outcome).unwrap(),
        )
    })
    .expect("spawn");
    std::thread::sleep(Duration::from_millis(300));
    assert!(supervised.kill());
    let outcome = rx.recv_timeout(Duration::from_secs(10)).expect("outcome");
    assert!(!outcome.status.unwrap().success());

    let mut bystander = bystander;
    assert!(
        bystander.try_wait().unwrap().is_none(),
        "an unrelated process with the same executable must keep running"
    );
    bystander.kill().unwrap();
    bystander.wait().unwrap();
}
