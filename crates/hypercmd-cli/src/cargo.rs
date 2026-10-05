//! Cargo builds with authored HTML locations alongside the original diagnostics.
use crate::Result;
use fusor_build::{SourceMap, backend::build::OutputManifest};
use serde::Deserialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    env,
    ffi::{OsStr, OsString},
    fs,
    io::{BufRead, BufReader, ErrorKind},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

const NO_RUST: &str = "Rust is not installed. Install it from https://rustup.rs:

  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

then open a new terminal and run this command again.";

#[derive(Deserialize)]
#[serde(tag = "reason")]
enum CargoMessage {
    #[serde(rename = "build-script-executed")]
    BuildScript { out_dir: PathBuf },
    #[serde(rename = "compiler-message")]
    Diagnostic { message: Diagnostic },
    #[serde(rename = "compiler-artifact")]
    Artifact {
        manifest_path: PathBuf,
        executable: Option<PathBuf>,
    },
    #[serde(other)]
    Other,
}

#[derive(Deserialize)]
struct Diagnostic {
    message: String,
    level: String,
    rendered: Option<String>,
    spans: Vec<Span>,
    children: Vec<Diagnostic>,
}

#[derive(Deserialize)]
struct Span {
    file_name: PathBuf,
    line_start: usize,
    expansion: Option<Expansion>,
}

#[derive(Deserialize)]
struct Expansion {
    span: Box<Span>,
}

type Maps = BTreeMap<PathBuf, (PathBuf, SourceMap)>;

/// A compiled program and the manifest of the package that owns it.
pub(crate) struct Program {
    pub executable: PathBuf,
    pub manifest: PathBuf,
}

/// Run a Cargo subcommand, reporting errors at their HTML locations too.
/// Returns the package's program, if the subcommand produced one.
pub(crate) fn compile(
    subcommand: &str,
    arguments: impl IntoIterator<Item = impl AsRef<OsStr>>,
) -> Result<Option<Program>> {
    let arguments: Vec<OsString> = arguments
        .into_iter()
        .map(|argument| argument.as_ref().to_owned())
        .collect();
    if arguments
        .iter()
        .any(|argument| argument.to_string_lossy().starts_with("--message-format"))
    {
        return Err("hypercmd controls Cargo's diagnostic format; omit --message-format".into());
    }
    let mut child = Command::new(env::var_os("CARGO").unwrap_or_else(|| "cargo".into()))
        .arg(subcommand)
        .arg("--message-format=json")
        .args(arguments)
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|error| match error.kind() {
            ErrorKind::NotFound => NO_RUST.into(),
            _ => Box::<dyn std::error::Error>::from(error),
        })?;
    let mut maps = Maps::new();
    let mut programs = Vec::new();
    let output = child.stdout.take().ok_or("Cargo stdout was not piped")?;
    for line in BufReader::new(output).lines() {
        let line = line?;
        match serde_json::from_str::<CargoMessage>(&line) {
            Ok(CargoMessage::BuildScript { out_dir }) => load_maps(&out_dir, &mut maps),
            Ok(CargoMessage::Diagnostic { message }) => {
                show_locations(&message, &maps);
                match &message.rendered {
                    Some(rendered) => eprint!("{rendered}"),
                    None => eprintln!("{}: {}", message.level, message.message),
                }
            }
            Ok(CargoMessage::Artifact {
                manifest_path,
                executable: Some(executable),
            }) => programs.push(Program {
                executable,
                manifest: manifest_path,
            }),
            Ok(_) => {}
            Err(_) => eprintln!("{line}"),
        }
    }
    let status = child.wait()?;
    if !status.success() {
        return Err(format!("the app has errors; see the messages above (Cargo {status})").into());
    }
    match programs.len() {
        0 | 1 => Ok(programs.pop()),
        _ => Err("this package has several programs; run Cargo directly to choose one".into()),
    }
}

fn load_maps(out_dir: &Path, maps: &mut Maps) {
    let manifest = out_dir.join("fusor_backends/hypercmd/manifest.json");
    if !manifest.exists() {
        return;
    }
    let result = (|| {
        OutputManifest::read(&manifest)?
            .sources
            .into_iter()
            .map(|source| {
                Ok((
                    source.rust,
                    (
                        source.source,
                        fs::read_to_string(source.source_map)?.parse()?,
                    ),
                ))
            })
            .collect::<Result<Vec<_>>>()
    })();
    match result {
        Ok(entries) => maps.extend(entries),
        Err(error) => eprintln!(
            "hypercmd: cannot map {}: {error}; original Rust locations follow",
            manifest.display()
        ),
    }
}

fn show_locations(diagnostic: &Diagnostic, maps: &Maps) {
    let mut locations = BTreeSet::new();
    for span in &diagnostic.spans {
        for span in std::iter::successors(Some(span), |span| {
            span.expansion
                .as_ref()
                .map(|expansion| expansion.span.as_ref())
        }) {
            if let Some((source, map)) = maps.get(&span.file_name) {
                if let Some(location) = map.lookup(span.line_start) {
                    locations.insert((source, location.line, location.column));
                }
            }
        }
    }
    let here = env::current_dir().unwrap_or_default();
    for (source, line, column) in locations {
        eprintln!(
            "{}:{line}:{column}: {}: {}",
            source.strip_prefix(&here).unwrap_or(source).display(),
            diagnostic.level,
            diagnostic.message
        );
    }
    for child in &diagnostic.children {
        show_locations(child, maps);
    }
}
