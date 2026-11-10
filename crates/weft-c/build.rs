use std::env;
use std::fs;
use std::path::PathBuf;

use weft_wire::generate::{client, header, parse};

fn main() -> Result<(), String> {
    let path = header()?;
    println!("cargo::rerun-if-changed={}", path.display());
    let text = fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let out = PathBuf::from(env::var("OUT_DIR").map_err(|e| e.to_string())?).join("hegel.rs");
    fs::write(&out, client(&parse(&text)?)?).map_err(|e| format!("{}: {e}", out.display()))
}
