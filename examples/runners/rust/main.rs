#[path = "../../services/rust/addresses.rs"]
mod addresses;
#[path = "../../services/rust/bank.rs"]
mod bank;

use std::process::ExitCode;

use weft_sdk::{Function, functions};

fn listed() -> Vec<Function> {
    functions![
        bank::reset,
        bank::state,
        bank::deposit,
        bank::withdraw,
        addresses::url,
        addresses::reset,
        addresses::state,
        addresses::correct,
    ]
}

fn main() -> ExitCode {
    weft_sdk::run(&listed())
}

#[test]
fn parcel() {
    weft_sdk::test("tests/parcel.weft.kdl", &listed());
}
