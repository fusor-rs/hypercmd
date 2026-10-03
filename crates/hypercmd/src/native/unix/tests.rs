use super::*;
use crate::ErrorKind;
use crate::{
    Kind, StaticNode,
    layout::tests::{el, txt},
};
use fusor::Signal;
use portable_pty::{CommandBuilder, PtySize};
#[path = "../../../../../tests/support/pty.rs"]
mod pty;
use pty::{Terminal, find};
use rustix::process::{Pid, WaitOptions, waitpid};
use std::{
    cell::Cell,
    rc::Rc,
    time::{Duration, Instant},
};

const CHILD: &str = "native::unix::tests::pty_child";
const LOAD_EDITS: usize = 4096;

#[test]
#[ignore = "subprocess entry point for terminal_pty"]
fn pty_child() {
    let mode = std::env::var("HYPERCMD_PTY_CASE").unwrap();
    if mode == "partial" {
        partial_restore();
        return;
    }
    let (scope, value, edits) = child_scope(&mode);
    spawn_background(&scope, &mode, &value);
    let mut options = NativeOptions::default();
    if mode == "limit" {
        options.max_paste_bytes = 8;
    }
    let outcome = panic::catch_unwind(AssertUnwindSafe(|| super::super::run_with(scope, &options)));
    match (mode.as_str(), outcome) {
        ("panic", Err(_)) => println!("PANIC_RESTORED"),
        ("limit", Ok(Err(error))) => {
            assert_eq!(error.kind, ErrorKind::Limit);
            println!("LIMIT_RESTORED");
        }
        (_, Ok(Ok(()))) => println!("FINISHED edits={} value={:?}", edits.get(), value.get()),
        (_, outcome) => panic!("unexpected runner result: {outcome:?}"),
    }
}

fn partial_restore() {
    struct FailsAfterAlternate {
        written: usize,
    }
    impl Write for FailsAfterAlternate {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            if self.written >= 8 {
                return Err(io::Error::other("injected setup failure"));
            }
            let count = io::stdout().write(bytes)?;
            self.written += count;
            Ok(count)
        }
        fn flush(&mut self) -> io::Result<()> {
            io::stdout().flush()
        }
    }
    let mut session = Session::default();
    assert!(
        session
            .enter(&mut FailsAfterAlternate { written: 0 })
            .is_err()
    );
    drop(session);
    println!("PARTIAL_RESTORED");
}

fn child_scope(mode: &str) -> (Scope, Signal<String>, Rc<Cell<usize>>) {
    let mut scope = Scope::new(
        None,
        &[
            el(None, "main"),
            el(Some(0), "p"),
            txt(1, "PTY_READY"),
            StaticNode {
                parent: Some(0),
                kind: Kind::Element("input", &[("type", "text"), ("id", "editor")], Some(0)),
            },
            StaticNode {
                parent: Some(0),
                kind: Kind::Element("button", &[], Some(1)),
            },
            txt(4, "Trigger"),
        ],
    )
    .unwrap();
    let value = fusor::signal(String::new());
    scope.bind_text(0, value.clone()).unwrap();
    let edits = Rc::new(Cell::new(0));
    let count = edits.clone();
    scope
        .on(0, "input", move |_| count.set(count.get() + 1))
        .unwrap();
    let scene = scope.root.0.scene.clone();
    let mode = mode.to_owned();
    let mut error_reported = false;
    scope
        .on(1, "click", move |_| match mode.as_str() {
            "flags" => {
                let flags = rustix::fs::fcntl_getfl(io::stdin()).unwrap();
                assert!(
                    !flags.contains(OFlags::NONBLOCK),
                    "native runner changed inherited stdin flags"
                );
                Ok(())
            }
            "panic" => panic!("injected application panic \x1b]52;c;bad\x07"),
            "error" if error_reported => {
                Err(Error::new(ErrorKind::Template, "newest reactive error"))
            }
            "error" => {
                for _ in 0..63 {
                    scene.report(Error::new(ErrorKind::Template, "injected reactive error"));
                }
                error_reported = true;
                Err(Error::new(ErrorKind::Template, "injected reactive error"))
            }
            _ => Ok(()),
        })
        .unwrap();
    (scope, value, edits)
}

fn spawn_background(scope: &Scope, mode: &str, value: &Signal<String>) {
    let services = Services::from_owner(&scope.owner()).unwrap();
    if mode == "load" {
        let wakes_itself = std::future::poll_fn(|context| {
            context.waker().wake_by_ref();
            std::task::Poll::<()>::Pending
        });
        services.spawn(&scope.owner(), wakes_itself).unwrap();
        return;
    }
    if !matches!(mode, "async" | "worker_panic") {
        return;
    }
    let service = services.clone();
    let result = value.clone();
    let scene = scope.root.0.scene.clone();
    let worker_panic = mode == "worker_panic";
    let task = async move {
        let sleep = service.sleep(Duration::from_millis(250)).unwrap();
        sleep.await.unwrap();
        let token = fusor_async::CancellationToken::default();
        let worker = service
            .worker(&token, move |_| {
                std::thread::sleep(Duration::from_millis(100));
                assert!(!worker_panic, "worker exploded \x1b]52;c;bad\x07");
                "TASK_AWAKE".to_owned()
            })
            .unwrap();
        match worker.await {
            Ok(text) => result.set(text),
            Err(error) => scene.report(error),
        }
    };
    services.spawn(&scope.owner(), task).unwrap();
}

// Existing scene consumers cannot establish OS termios or real PTY byte
// ordering. This one child protocol covers the required restoration paths.
#[test]
fn terminal_pty() {
    for mode in [
        "normal",
        "error",
        "panic",
        "partial",
        "limit",
        "suspend",
        "load",
        "flags",
        "terminate",
        "async",
        "worker_panic",
    ] {
        exercise(mode);
    }
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--ignored", "--exact", CHILD, "--nocapture"])
        .env("HYPERCMD_PTY_CASE", "redirect")
        .output()
        .unwrap();
    assert!(!output.stdout.windows(2).any(|bytes| bytes == b"\x1b["));
    assert!(
        !output.status.success(),
        "redirected runner must reject before screen controls"
    );
}

fn exercise(mode: &str) {
    let mut command = CommandBuilder::new(std::env::current_exe().unwrap());
    command.args(["--ignored", "--exact", CHILD, "--nocapture"]);
    command.env("HYPERCMD_PTY_CASE", mode);
    let mut terminal = Terminal::spawn(command, 12, 60);
    if mode != "partial" {
        terminal.wait_for(0, "PTY_READY");
        std::thread::sleep(Duration::from_millis(80));
        drive(&mut terminal, mode);
        if !matches!(mode, "panic" | "limit" | "terminate") {
            terminal.send(&[3]);
        }
    }
    terminal.wait_exit();
    terminal.assert_restored(mode != "partial");
    check_transcript(&terminal, mode);
}

fn drive(terminal: &mut Terminal, mode: &str) {
    match mode {
        "normal" => {
            assert_quiet(terminal);
            terminal.send(b"\x1b[200~a\x03\r\n\x1b]52;c;bad\x07\x1b[201~");
            std::thread::sleep(Duration::from_millis(80));
            assert!(
                terminal.child.try_wait().unwrap().is_none(),
                "paste Ctrl+C ran shutdown"
            );
            let smaller = PtySize {
                rows: 9,
                cols: 40,
                ..Default::default()
            };
            terminal.master.resize(smaller).unwrap();
            std::thread::sleep(Duration::from_millis(80));
        }
        "panic" | "flags" => {
            terminal.send(b"\t\r");
        }
        "error" => {
            terminal.expect(b"\t\r", "injected reactive error");
            terminal.expect(b"\r", "newest reactive");
            assert!(
                terminal.child.try_wait().unwrap().is_none(),
                "recoverable error stopped interaction"
            );
        }
        "limit" => {
            terminal.send(b"\x1b[200~xxxxxxxxxxxxxxxxxxxxxxxx");
        }
        "suspend" => {
            terminal.send(&[26]);
            terminal.wait_for(0, "\x1b[?1049l");
            wait_stopped(terminal);
            terminal.assert_termios();
            let resumed_at = terminal.transcript.len();
            signal(terminal, "-CONT");
            terminal.wait_for(resumed_at, "\x1b[?2004h");
        }
        "async" => {
            terminal.wait_for(0, "TASK_AWAKE");
            assert_quiet(terminal);
        }
        "worker_panic" => terminal.wait_for(0, "Service:"),
        "terminate" => signal(terminal, "-TERM"),
        // Cross many input turns while a task continuously wakes itself. Keep the
        // draft bounded so grapheme scans do not turn this into a growing-text
        // throughput benchmark against the exit deadline.
        "load" => {
            terminal.send(&b"x\x7f".repeat(LOAD_EDITS / 2));
        }
        _ => panic!("unknown PTY mode {mode}"),
    }
}

fn check_transcript(terminal: &Terminal, mode: &str) {
    let output = &terminal.transcript;
    let text = String::from_utf8_lossy(output);
    assert!(
        find(output, b"\x1b]52;c;bad\x07").is_none(),
        "untrusted OSC reached output"
    );
    match mode {
        "worker_panic" => assert!(text.contains("worker exploded")),
        "normal" => assert!(text.contains("FINISHED edits=1")),
        "load" => assert!(text.contains(&format!("FINISHED edits={LOAD_EDITS} value=\"\""))),
        "error" => {
            assert!(text.contains("Limit: 1 earlier reactive errors omitted"));
            assert!(text.contains("Template: newest reactive error"));
        }
        "panic" => {
            assert!(text.contains("\\u{1b}]52;c;bad\\u{7}"));
            let restored = find(output, b"\x1b[?1049l").unwrap();
            let report = find(output, b"injected application panic").unwrap();
            assert!(restored < report, "panic was reported before restoration");
        }
        _ => {}
    }
}

fn assert_quiet(terminal: &mut Terminal) {
    std::thread::sleep(Duration::from_millis(80));
    terminal.drain();
    let settled = terminal.transcript.len();
    std::thread::sleep(Duration::from_millis(180));
    terminal.drain();
    assert_eq!(
        terminal.transcript.len(),
        settled,
        "settled app emitted idle output"
    );
}

fn signal(terminal: &Terminal, signal: &str) {
    let pid = terminal.child.process_id().unwrap().to_string();
    let status = std::process::Command::new("kill")
        .args([signal, &pid])
        .status()
        .unwrap();
    assert!(status.success());
}

fn wait_stopped(terminal: &Terminal) {
    // Screen restoration precedes SIGSTOP; sending SIGCONT before the stop
    // takes effect would leave the child suspended indefinitely.
    let pid = Pid::from_raw(terminal.child.process_id().unwrap().try_into().unwrap()).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match waitpid(Some(pid), WaitOptions::UNTRACED | WaitOptions::NOHANG) {
            Ok(Some((_, status))) => {
                assert!(status.stopped(), "child did not stop: {status:?}");
                return;
            }
            Ok(None) | Err(rustix::io::Errno::INTR) => {}
            Err(error) => panic!("could not wait for child to stop: {error}"),
        }
        assert!(Instant::now() < deadline, "child did not stop");
        std::thread::sleep(Duration::from_millis(10));
    }
}
