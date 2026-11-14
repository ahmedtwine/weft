#[path = "parcel/addresses.rs"]
mod addresses;
#[path = "bank/bank.rs"]
mod bank;

use std::process::ExitCode;

fn main() -> ExitCode {
    weft_sdk::run([bank::module(), addresses::module()])
}
