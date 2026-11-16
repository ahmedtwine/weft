use std::collections::BTreeMap;
use std::io::BufRead;
use std::path::Path;
use std::process::ExitCode;

use weft_samples::Result;
use weft_samples::artifact::{Artifact, example};
use weft_samples::machine::{Alternate, Machine};
use weft_samples::model::Live;

const USAGE: &str = "usage: weft-samples check <file> <invariants.json> | narrow <file>";

fn asked(_: &str, alternate: &Alternate) -> Option<bool> {
    println!(
        "  {}  {}  {:?}",
        alternate.kind, alternate.change, alternate.text
    );
    println!("  reject (r) or accept (a)?");
    std::io::stdin()
        .lock()
        .lines()
        .map_while(std::result::Result::ok)
        .find_map(|line| match line.trim() {
            "r" => Some(true),
            "a" => Some(false),
            _ => None,
        })
}

fn narrow(file: &Path) -> Result<()> {
    let artifact = Artifact::open(file, Box::new(Live::open()?))?;
    let machines = artifact.narrow(&example(&file.with_extension("json"))?, asked)?;
    let found: BTreeMap<String, String> = machines
        .iter()
        .filter_map(Machine::invariant)
        .map(|invariant| (invariant.name, invariant.measure))
        .collect();
    artifact.violations(&found).map(|_| ())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let done = match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        ["check", file, invariants] => Live::open()
            .and_then(|model| Artifact::open(Path::new(file), Box::new(model)))
            .and_then(|artifact| {
                artifact
                    .violations(&example(Path::new(invariants))?.invariants)
                    .map(|_| ())
            }),
        ["narrow", file] => narrow(Path::new(file)),
        _ => Err(USAGE.into()),
    };
    match done {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}
