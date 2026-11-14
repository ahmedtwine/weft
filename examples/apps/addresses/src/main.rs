mod addresses;

use std::process::ExitCode;

fn main() -> ExitCode {
    weft_sdk::run([addresses::module()])
}
