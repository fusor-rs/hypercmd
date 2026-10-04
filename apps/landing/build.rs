use docs_base_build::Highlighter;
use quote::{format_ident, quote};
use std::{env, fs, path::PathBuf};

const EXAMPLES: [(&str, &str, &str, u16); 5] = [
    ("Counter", "Counter", "Try +1, then reset.", 13_u16),
    (
        "Search",
        "Filesystem",
        "Search this app's source files. Try .rs or .html.",
        17,
    ),
    (
        "Tables",
        "Tables",
        "Real file sizes. Try sorting the rows.",
        18,
    ),
    ("Boxes", "Boxes", "Switch between columns and rows.", 23),
    (
        "Menu",
        "Menu",
        "Choose a command with a click or Tab + Enter.",
        21,
    ),
];

fn main() -> Result<(), Box<dyn std::error::Error>> {
    fs::create_dir_all("public/brand")?;
    for name in [
        "hypercmd.svg",
        "hypercmd-horizontal.svg",
        "hypercmd-social.png",
    ] {
        let source = format!("../../assets/brand/{name}");
        let destination = format!("public/brand/{name}");
        println!("cargo:rerun-if-changed={source}");
        write_asset(&destination, &fs::read(source)?)?;
    }
    println!("cargo:rerun-if-changed=../../install.sh");
    write_asset("public/install.sh", &fs::read("../../install.sh")?)?;
    highlighted_examples()?;
    project_files()?;
    hypercmd_build::compile_app()?;
    fusor_build::compile_app()?;
    Ok(())
}

fn highlighted_examples() -> Result<(), Box<dyn std::error::Error>> {
    let highlighter = Highlighter::default();
    let mut examples = Vec::new();
    for (name, label, hint, rows) in EXAMPLES {
        let kind = format_ident!("{name}");
        let mut files = Vec::new();
        for (directory, extension, language) in
            [("ui", "html", "HTML"), ("src/examples", "rs", "Rust")]
        {
            let filename = format!("{}.{extension}", name.to_lowercase());
            let source = fs::read_to_string(format!("{directory}/{filename}"))?;
            let mut lines = Vec::new();
            for (index, line) in source.lines().enumerate() {
                let number = index + 1;
                let tokens = highlighter.tokens(line, language)?;
                lines.push(quote! { CodeLine { number: #number, tokens: #tokens } });
            }
            files.push(quote! {
                SourceFile { filename: #filename, language: #language, lines: &[#(#lines),*] }
            });
            write_asset(&format!("public/{filename}"), source.as_bytes())?;
        }
        examples.push(quote! {
            Example {
                kind: ExampleKind::#kind, label: #label, hint: #hint,
                rows: #rows, files: [#(#files),*],
            }
        });
    }
    fs::write(
        PathBuf::from(env::var("OUT_DIR")?).join("source.rs"),
        quote! { const EXAMPLES: &[Example] = &[#(#examples),*]; }.to_string(),
    )?;
    Ok(())
}

fn project_files() -> Result<(), Box<dyn std::error::Error>> {
    let mut paths = Vec::new();
    for directory in ["src", "ui"] {
        collect_files(directory, &mut paths)?;
    }
    paths.sort();
    let mut files = Vec::new();
    for path in paths {
        println!("cargo:rerun-if-changed={path}");
        let bytes = fs::metadata(&path)?.len();
        files.push(quote! { ProjectFile { path: #path, bytes: #bytes } });
    }
    fs::write(
        PathBuf::from(env::var("OUT_DIR")?).join("files.rs"),
        quote! { pub(super) const FILES: &[ProjectFile] = &[#(#files),*]; }.to_string(),
    )?;
    Ok(())
}

fn collect_files(directory: &str, paths: &mut Vec<String>) -> std::io::Result<()> {
    println!("cargo:rerun-if-changed={directory}");
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let path = entry.path().to_string_lossy().into_owned();
        if entry.file_type()?.is_dir() {
            collect_files(&path, paths)?;
        } else if entry.file_type()?.is_file() {
            paths.push(path);
        }
    }
    Ok(())
}

fn write_asset(path: &str, contents: &[u8]) -> std::io::Result<()> {
    println!("cargo:rerun-if-changed={path}");
    let changed = match fs::read(path) {
        Ok(current) => current != contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => true,
        Err(error) => return Err(error),
    };
    if changed {
        fs::write(path, contents)?;
    }
    Ok(())
}
