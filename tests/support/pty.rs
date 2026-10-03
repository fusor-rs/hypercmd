#![allow(dead_code)]
use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};
use std::{
    io::{Read, Write},
    sync::mpsc::{Receiver, sync_channel},
    time::{Duration, Instant},
};

type TermiosCheck = dyn Fn(&dyn MasterPty) -> bool;

pub struct Terminal {
    pub child: Box<dyn Child + Send + Sync>,
    pub master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    receiver: Receiver<Vec<u8>>,
    pub transcript: Vec<u8>,
    termios: Box<TermiosCheck>,
}
impl Drop for Terminal {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
impl Terminal {
    pub fn spawn(mut command: CommandBuilder, rows: u16, cols: u16) -> Self {
        let pair = native_pty_system()
            .openpty(PtySize {
                rows,
                cols,
                ..Default::default()
            })
            .unwrap();
        let original = pair.master.get_termios().unwrap();
        command.env("TERM", "xterm-256color");
        let child = pair.slave.spawn_command(command).unwrap();
        drop(pair.slave);
        let writer = pair.master.take_writer().unwrap();
        let mut reader = pair.master.try_clone_reader().unwrap();
        let (sender, receiver) = sync_channel(8);
        std::thread::spawn(move || {
            let mut bytes = [0; 4096];
            while let Ok(count) = reader.read(&mut bytes) {
                if count == 0 || sender.send(bytes[..count].to_vec()).is_err() {
                    break;
                }
            }
        });
        Self {
            child,
            master: pair.master,
            writer,
            receiver,
            transcript: Vec::new(),
            termios: Box::new(move |master| original == master.get_termios().unwrap()),
        }
    }
    fn record(&mut self, bytes: Vec<u8>) {
        assert!(
            self.transcript.len() + bytes.len() <= 2 * 1024 * 1024,
            "PTY output did not settle"
        );
        self.transcript.extend(bytes);
    }
    pub fn drain(&mut self) {
        while let Ok(bytes) = self.receiver.try_recv() {
            self.record(bytes);
        }
    }
    pub fn send(&mut self, bytes: &[u8]) -> usize {
        self.drain();
        let start = self.transcript.len();
        self.writer.write_all(bytes).unwrap();
        self.writer.flush().unwrap();
        start
    }
    pub fn expect(&mut self, bytes: &[u8], text: &str) {
        let start = self.send(bytes);
        self.wait_for(start, text);
    }
    pub fn wait_for(&mut self, start: usize, text: &str) {
        let deadline = Instant::now() + Duration::from_secs(5);
        let needle: String = text.chars().filter(|ch| !ch.is_whitespace()).collect();
        while find(&self.transcript[start..], text.as_bytes()).is_none()
            && !printed_text(&self.transcript[start..]).contains(&needle)
        {
            let bytes = self
                .receiver
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .unwrap_or_else(|error| {
                    panic!(
                        "PTY did not emit {text:?}: {error}; output={:?}",
                        String::from_utf8_lossy(&self.transcript[start..])
                    )
                });
            self.record(bytes);
        }
    }
    pub fn wait_exit(&mut self) {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            self.drain();
            if let Some(status) = self.child.try_wait().unwrap() {
                while let Ok(bytes) = self.receiver.recv_timeout(Duration::from_millis(30)) {
                    self.record(bytes);
                }
                assert!(
                    status.success(),
                    "child failed: {status}; output={:?}",
                    String::from_utf8_lossy(&self.transcript)
                );
                return;
            }
            assert!(
                Instant::now() < deadline,
                "child did not exit; output={:?}",
                String::from_utf8_lossy(&self.transcript)
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    pub fn assert_termios(&self) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while !(self.termios)(&*self.master) {
            assert!(Instant::now() < deadline, "termios was not restored");
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    pub fn assert_restored(&mut self, paste: bool) {
        self.assert_termios();
        for reset in ["\x1b[?1049l", "\x1b[?25h"]
            .into_iter()
            .chain(paste.then_some("\x1b[?2004l"))
        {
            self.wait_for(0, reset);
        }
    }
    pub fn signal(&self, signal: &str) {
        assert!(
            std::process::Command::new("kill")
                .args([signal, &self.child.process_id().unwrap().to_string()])
                .status()
                .unwrap()
                .success()
        );
    }
    pub fn assert_quiet(&mut self) {
        std::thread::sleep(Duration::from_millis(80));
        self.drain();
        assert!(
            self.receiver
                .recv_timeout(Duration::from_millis(180))
                .is_err(),
            "settled app emitted idle output"
        );
    }
}
pub fn find(bytes: &[u8], needle: &[u8]) -> Option<usize> {
    bytes
        .windows(needle.len())
        .position(|bytes| bytes == needle)
}
// Ratatui may move the cursor across spaces instead of writing them.
fn printed_text(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    let mut chars = text.chars();
    let mut printed = String::new();
    while let Some(ch) = chars.next() {
        if ch == '\x1b' && chars.next() == Some('[') {
            for ch in chars.by_ref() {
                if ('@'..='~').contains(&ch) {
                    break;
                }
            }
        } else if !ch.is_whitespace() {
            printed.push(ch);
        }
    }
    printed
}
