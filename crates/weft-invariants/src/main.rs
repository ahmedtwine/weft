use std::path::Path;
use std::process::ExitCode;

use weft_invariants::{Options, mine};
use weft_samples::Result;

const USAGE: &str =
    "usage: weft-invariants <directory> <test> [cases=<count>] [steps=<count>] [examples=<file>]…";

fn given(mut options: Options, option: &str) -> Result<Options> {
    let count = |count: &str| count.parse().map_err(|e| format!("{count}: {e}"));
    match option.split_once('=') {
        Some(("cases", cases)) => options.cases = Some(count(cases)?),
        Some(("steps", steps)) => options.steps = Some(count(steps)?),
        Some(("examples", file)) => options.examples.push(file.into()),
        _ => return Err(USAGE.into()),
    }
    Ok(options)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args == [weft_orchestrator::REAP] {
        weft_orchestrator::reap();
        return ExitCode::SUCCESS;
    }
    let done = match args.as_slice() {
        [directory, test, options @ ..] => weft_orchestrator::guard()
            .and_then(|()| {
                options
                    .iter()
                    .map(String::as_str)
                    .try_fold(Options::default(), given)
            })
            .and_then(|options| mine(Path::new(directory), Path::new(test), &options)),
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
