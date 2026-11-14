mod door;

use std::process::ExitCode;

fn main() -> ExitCode {
    weft_sdk::run([door::module()])
}
