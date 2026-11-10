use std::env;
use std::fs;
use std::path::PathBuf;

use weft_wire::generate::{header, parse, server};

const OWNED: [&str; 7] = [
    "hegel_run_start",
    "hegel_run_start_blob",
    "hegel_test_case_from_blob",
    "hegel_next_test_case",
    "hegel_mark_complete",
    "hegel_run_result",
    "hegel_failure_origin",
];

fn main() -> Result<(), String> {
    let path = header()?;
    println!("cargo::rerun-if-changed={}", path.display());
    let text = fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let out = PathBuf::from(env::var("OUT_DIR").map_err(|e| e.to_string())?).join("hegel.rs");
    fs::write(&out, server(&parse(&text)?, &OWNED)?).map_err(|e| format!("{}: {e}", out.display()))
}
