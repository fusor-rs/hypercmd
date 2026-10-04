pub mod pty;

use portable_pty::{CommandBuilder, PtySize};
use std::{env, io, io::Write};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut arguments = env::args_os().skip(1);
    let executable = arguments.next().ok_or("expected a command to run")?;
    let mut command = CommandBuilder::new(executable);
    command.args(arguments);
    let size = PtySize::default();
    let mut terminal = pty::Terminal::spawn(command, size.rows, size.cols);
    terminal.wait_exit();
    io::stdout().write_all(&terminal.transcript)?;
    Ok(())
}
