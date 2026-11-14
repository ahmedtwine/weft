use std::sync::Mutex;

use schemars::JsonSchema;
use serde::Serialize;
use weft_sdk::{self as weft, Module};

#[derive(Clone, Serialize, JsonSchema)]
struct Door {
    state: &'static str,
    error: &'static str,
}

static DOOR: Mutex<Door> = Mutex::new(Door {
    state: "closed",
    error: "",
});

fn set(state: &'static str, error: &'static str) -> Result<(), String> {
    *DOOR.lock().map_err(|e| e.to_string())? = Door { state, error };
    Ok(())
}

fn now() -> Door {
    DOOR.lock().map_or(Door { state: "", error: "" }, |d| d.clone())
}

pub fn reset() -> Result<(), String> {
    set("closed", "")
}

pub fn open() -> Result<(), String> {
    match now().state {
        "locked" => set("locked", "the door is locked"),
        _ => set("open", ""),
    }
}

pub fn close() -> Result<(), String> {
    set("closed", "")
}

pub fn lock() -> Result<(), String> {
    set("locked", "")
}

pub fn unlock() -> Result<(), String> {
    set("closed", "")
}

pub fn module() -> Module {
    weft::state("door", now);
    weft::module!()
        .function("reset", reset)
        .function("open", open)
        .function("close", close)
        .function("lock", lock)
        .function("unlock", unlock)
}
