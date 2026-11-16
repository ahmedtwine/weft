use std::sync::Mutex;

use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize)]
pub struct Bank {
    balance: i64,
}

#[derive(Deserialize)]
pub struct Amount {
    amount: i64,
}

static BANK: Mutex<Bank> = Mutex::new(Bank { balance: 0 });

fn change(by: impl FnOnce(&mut Bank)) -> Result<(), String> {
    by(&mut *BANK.lock().map_err(|e| e.to_string())?);
    Ok(())
}

pub fn state() -> Result<Bank, String> {
    BANK.lock().map(|b| b.clone()).map_err(|e| e.to_string())
}

pub fn reset() -> Result<(), String> {
    change(|b| b.balance = 0)
}

pub fn deposit(given: Amount) -> Result<(), String> {
    change(|b| b.balance += given.amount)
}

pub fn withdraw(given: Amount) -> Result<(), String> {
    change(|b| {
        if given.amount <= b.balance {
            b.balance -= given.amount;
        }
    })
}
