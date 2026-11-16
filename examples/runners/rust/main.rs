#[path = "parcel/addresses.rs"]
mod addresses;
#[path = "bank/bank.rs"]
mod bank;

use std::process::ExitCode;

fn main() -> ExitCode {
    weft_sdk::run([bank::module(), addresses::module()])
}

#[test]
fn parcel() {
    weft_sdk::test("tests/parcel.weft.kdl", [bank::module(), addresses::module()]);
}
