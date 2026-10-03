#![cfg(unix)]

use portable_pty::CommandBuilder;
use std::fs;
#[path = "../../../tests/support/pty.rs"]
mod pty;
use pty::Terminal;

// The runtime PTY suite does not exercise this generated app's filesystem
// workers, navigation or retained filter. Drive those together in one session.
#[test]
fn browse_preview_and_return() {
    let directory = tempfile::tempdir().unwrap();
    fs::create_dir(directory.path().join("NESTED")).unwrap();
    fs::write(directory.path().join("readme.txt"), "XYZ_ROOT_PREVIEW_OK").unwrap();
    fs::write(
        directory.path().join("NESTED/INSIDE.txt"),
        "NESTED_PREVIEW_OK",
    )
    .unwrap();
    fs::write(directory.path().join("binary.bin"), [0, 255]).unwrap();

    let mut command = CommandBuilder::new(env!("CARGO_BIN_EXE_file-browser"));
    command.arg(directory.path());
    let mut terminal = Terminal::spawn(command, 18, 40);

    terminal.wait_for(0, "File browser");
    terminal.wait_for(0, "3 entries");
    terminal.wait_for(0, "Ctrl+C");
    terminal.wait_for(0, "Refresh");
    terminal.expect(b"readme\x1b[B\x1b[A\x1b[B\r", "XYZ_ROOT_PREVIEW_OK");
    terminal.expect(b"\r", "1 of 3 entries");

    terminal.expect(b"\x1b[F\x7f\x7f\x7f\x7f\x7f\x7fnested", "NESTED/");
    terminal.expect(b"\x1b[B\r", "INSIDE.txt");
    terminal.expect(b"\t\r", "NESTED_PREVIEW_OK");
    terminal.expect(b"\r", "INSIDE.txt");
    terminal.expect(b"\t\r", "binary.bin");

    terminal.expect(b"\x1b[Zbinary", "binary.bin");
    terminal.expect(b"\t\t\t\r", "UTF-8");
    fs::write(
        directory.path().join("binary.bin"),
        "XYZ_RECOVERED_PREVIEW_OK",
    )
    .unwrap();
    terminal.expect(b"\t\r", "XYZ_RECOVERED_PREVIEW_OK");

    terminal.send(&[3]);
    terminal.wait_exit();
    terminal.assert_restored(true);
}
