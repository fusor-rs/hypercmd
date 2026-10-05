//! The hypercmd command-line tool: create, check, run and build terminal apps.
mod cargo;
mod new;
mod upgrade;

use clap::{Parser, Subcommand};
use std::{ffi::OsString, fs, path::PathBuf, process::ExitCode};

type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;

#[derive(Parser)]
#[command(
    name = "hypercmd",
    version,
    about = "Build terminal apps from HTML and Rust",
    after_help = "Examples:\n  hypercmd new my-app\n  cd my-app\n  hypercmd run\n  hypercmd build"
)]
struct Cli {
    #[command(subcommand)]
    command: Action,
}

#[derive(Subcommand)]
enum Action {
    /// Create an app in a new directory and check that it builds
    New {
        path: PathBuf,
        /// Use this hypercmd checkout instead of crates.io, for unreleased versions
        #[arg(long)]
        hypercmd_path: Option<PathBuf>,
    },
    /// Build the app in this directory and run it
    Run {
        /// Arguments for the app
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<OsString>,
    },
    /// Find errors, reported at the HTML lines that caused them
    Check {
        /// Extra Cargo arguments
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<OsString>,
    },
    /// Build an optimized program into dist/
    Build {
        /// Build without optimizations, faster to compile
        #[arg(long)]
        debug: bool,
    },
    /// List the supported HTML elements and CSS properties
    Profile,
    /// Upgrade the installed CLI to the latest stable release
    Upgrade,
}

fn main() -> ExitCode {
    let action = Cli::parse().command;
    let offline = matches!(&action, Action::Check { args }
        if args.iter().any(|argument| argument == "--offline" || argument == "--frozen"));
    if !offline && !matches!(action, Action::Upgrade) {
        upgrade::notify();
    }
    match run(action) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("hypercmd: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run(action: Action) -> Result {
    match action {
        Action::Upgrade => upgrade::run(),
        Action::New {
            path,
            hypercmd_path,
        } => new::run(&path, hypercmd_path.as_deref()),
        Action::Check { args } => cargo::compile("check", args).map(drop),
        Action::Run { args } => {
            let program = program(&[])?;
            let mut command = std::process::Command::new(program.executable);
            command.args(args);
            // Replace this process so the app owns the terminal, signals and suspend.
            #[cfg(unix)]
            let error = std::os::unix::process::CommandExt::exec(&mut command);
            #[cfg(not(unix))]
            let error = match command.status()? {
                status if status.success() => return Ok(()),
                status => std::io::Error::other(format!("the app exited with {status}")),
            };
            Err(error.into())
        }
        Action::Build { debug } => {
            let program = program(if debug { &[] } else { &["--release"] })?;
            let dist = program.manifest.with_file_name("dist");
            fs::create_dir_all(&dist)?;
            let file_name = program
                .executable
                .file_name()
                .ok_or("the program has no name")?;
            fs::copy(&program.executable, dist.join(file_name))?;
            println!("Built dist/{}", file_name.to_string_lossy());
            Ok(())
        }
        Action::Profile => {
            println!(
                "{}\nElements: {}\n\n| Property | Accepted values |\n| --- | --- |",
                hypercmd_build::profile::NAME,
                hypercmd_build::profile::ELEMENTS.join(", ")
            );
            for (property, _, values) in hypercmd_build::profile::CSS_PROPERTIES {
                println!("| {property} | {} |", values.replace('|', "or"));
            }
            Ok(())
        }
    }
}

fn program(arguments: &[&str]) -> Result<cargo::Program> {
    cargo::compile("build", arguments)?
        .ok_or_else(|| "this directory has no app to run; create one with `hypercmd new`".into())
}
