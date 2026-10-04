use docs_base_build::{Config, Highlighter};
use std::{
    env, fs,
    path::{Path, PathBuf},
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let directory = PathBuf::from(env::var("CARGO_MANIFEST_DIR")?);
    let root = directory
        .parent()
        .and_then(Path::parent)
        .ok_or("the docs app must live in apps/docs inside the Hypercmd repository")?;
    let guides = docs_base_build::compile(
        &Config {
            root,
            content: Path::new("docs"),
            navigation: Path::new("apps/docs/navigation.json"),
            references: None,
            base_path: "/docs/",
            repository: Some("https://github.com/fusor-rs/hypercmd/blob/main/"),
        },
        &Highlighter::default(),
    )?;
    guides.write_assets(Path::new("public"))?;
    fs::write(
        PathBuf::from(env::var("OUT_DIR")?).join("guides.rs"),
        guides.source.to_string(),
    )?;
    println!("cargo:rerun-if-changed=../../assets/brand/hypercmd.svg");
    fs::copy(root.join("assets/brand/hypercmd.svg"), "public/favicon.svg")?;
    fusor_build::compile_app()
}
