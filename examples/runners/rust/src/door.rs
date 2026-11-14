use std::sync::Mutex;

use weft_sdk::{Json, Module, json};

static STATE: Mutex<&str> = Mutex::new("closed");

fn set(to: &'static str) -> Result<(), String> {
    *STATE.lock().map_err(|e| e.to_string())? = to;
    Ok(())
}

pub fn door() -> Result<Json, String> {
    Ok(json!({ "state": *STATE.lock().map_err(|e| e.to_string())? }))
}

pub fn reset() -> Result<(), String> {
    set("closed")
}

pub fn open() -> Result<(), String> {
    if *STATE.lock().map_err(|e| e.to_string())? == "locked" {
        return Err("the door is locked".into());
    }
    set("open")
}

pub fn close() -> Result<(), String> {
    set("closed")
}

pub fn lock() -> Result<(), String> {
    set("locked")
}

pub fn unlock() -> Result<(), String> {
    set("closed")
}

fn verb(verb: &str) -> Json {
    json!({ "type": "object", "properties": {}, "additionalProperties": false, "weft": { "verb": verb } })
}

pub fn module() -> Module {
    weft_sdk::module!()
        .schema(
            "doorSchema",
            json!({
                "type": "object",
                "properties": { "state": { "type": "string", "weft": { "read": "door.state" } } },
                "required": ["state"],
                "additionalProperties": false
            }),
        )
        .schema("errorSchema", json!({ "type": "string", "weft": { "read": "door.error" } }))
        .schema("openSchema", verb("open"))
        .schema("closeSchema", verb("close"))
        .schema("lockSchema", verb("lock"))
        .schema("unlockSchema", verb("unlock"))
        .function("door", door)
        .function("reset", reset)
        .function("open", open)
        .function("close", close)
        .function("lock", lock)
        .function("unlock", unlock)
}
