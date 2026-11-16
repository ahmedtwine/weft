use std::collections::BTreeSet;
use std::path::PathBuf;
use std::process::Command;

use serde_json::Value as Json;

const FAMILIES: [&str; 3] = ["hegel_generate_", "hegel_string_generator_", "hegel_new_"];
const NO_SCHEMA_FORM: [&str; 2] = ["hegel_generate_integer_big", "hegel_new_repeat"];
const SOURCES: [&str; 2] = [
    include_str!("../src/generator.rs"),
    include_str!("../src/engine.rs"),
];

fn header() -> String {
    let metadata = Command::new(env!("CARGO"))
        .args(["metadata", "--format-version", "1", "--locked"])
        .output()
        .expect("cargo metadata");
    let metadata: Json = serde_json::from_slice(&metadata.stdout).expect("metadata");
    let manifest = metadata["packages"]
        .as_array()
        .expect("packages")
        .iter()
        .find(|package| package["name"] == "hegeltest-c")
        .and_then(|package| package["manifest_path"].as_str())
        .expect("hegeltest-c");
    std::fs::read_to_string(
        PathBuf::from(manifest)
            .with_file_name("include")
            .join("hegel.h"),
    )
    .expect("hegel.h")
}

#[test]
fn each_native_generator_of_the_pinned_hegel_has_a_call_in_weft() {
    let header = header();
    let native: BTreeSet<&str> = header
        .lines()
        .filter_map(|line| line.strip_prefix("hegel_result_t "))
        .filter_map(|line| line.split('(').next())
        .filter(|name| FAMILIES.iter().any(|family| name.starts_with(family)))
        .collect();
    assert!(
        native.len() > 20,
        "hegel.h names {} generators",
        native.len()
    );
    let missing: Vec<&str> = native
        .into_iter()
        .filter(|name| !NO_SCHEMA_FORM.contains(name))
        .filter(|name| {
            !SOURCES
                .iter()
                .any(|source| source.contains(&format!("{name}(")))
        })
        .collect();
    assert!(missing.is_empty(), "weft calls no {missing:?}");
}
