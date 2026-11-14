#[path = "parcel/addresses.rs"]
mod addresses;
#[path = "door/door.rs"]
mod door;

use std::process::ExitCode;

fn main() -> ExitCode {
    weft_sdk::run([door::module(), addresses::module()])
}
