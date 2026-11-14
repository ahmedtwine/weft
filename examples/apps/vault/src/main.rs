mod vault;

use std::process::ExitCode;

fn main() -> ExitCode {
    weft_sdk::run([vault::module()])
}
