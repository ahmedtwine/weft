use std::sync::Mutex;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use weft_sdk::{self as weft, Module};

#[derive(Clone, Serialize, JsonSchema)]
struct Bank {
    balance: i64,
}

#[derive(Deserialize, JsonSchema)]
struct Amount {
    #[schemars(range(min = 1, max = 100))]
    amount: i64,
}

static BANK: Mutex<Bank> = Mutex::new(Bank { balance: 0 });

fn now() -> Bank {
    BANK.lock().map_or(Bank { balance: 0 }, |b| b.clone())
}

fn change(by: impl FnOnce(&mut Bank)) -> Result<(), String> {
    by(&mut *BANK.lock().map_err(|e| e.to_string())?);
    Ok(())
}

fn reset() -> Result<(), String> {
    change(|b| b.balance = 0)
}

fn deposit(given: Amount) -> Result<(), String> {
    change(|b| b.balance += given.amount)
}

fn withdraw(given: Amount) -> Result<(), String> {
    change(|b| {
        if given.amount <= b.balance {
            b.balance -= given.amount;
        }
    })
}

pub fn module() -> Module {
    weft::state("bank", now);
    weft::module!()
        .function("reset", reset)
        .function("deposit", deposit)
        .function("withdraw", withdraw)
}
