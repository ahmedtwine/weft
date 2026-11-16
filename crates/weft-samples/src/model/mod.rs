mod gemini;
mod jev;

use std::collections::BTreeMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::PathBuf;

use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use crate::{Candidate, Result};
use gemini::Gemini;
use jev::Jev;

pub use gemini::MODEL;

const NAME: &str = env!("CARGO_PKG_NAME");

fn home(kind: &str) -> PathBuf {
    PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
        .join(kind)
        .join(NAME)
}

fn secret(name: &str) -> Result<String> {
    let file = std::fs::read_to_string(home(".config").join("secrets.env")).unwrap_or_default();
    std::env::var(name)
        .ok()
        .or_else(|| {
            file.lines().find_map(|l| {
                l.strip_prefix(&format!("{name}="))
                    .map(|v| v.trim().trim_matches('"').to_string())
            })
        })
        .filter(|v| !v.is_empty())
        .ok_or(format!(
            "no {name} in the environment or in ~/.config/{NAME}/secrets.env"
        ))
}

async fn cached<T: Serialize + DeserializeOwned>(
    key: impl Hash,
    fetch: impl Future<Output = Result<T>>,
) -> Result<T> {
    let cache = home(".cache");
    std::fs::create_dir_all(&cache).map_err(|e| e.to_string())?;
    let mut hasher = DefaultHasher::new();
    key.hash(&mut hasher);
    let file = cache.join(format!("{:016x}", hasher.finish()));
    if let Some(found) = std::fs::read(&file)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
    {
        return Ok(found);
    }
    let fetched = fetch.await?;
    std::fs::write(&file, serde_json::to_vec(&fetched).unwrap_or_default()).ok();
    Ok(fetched)
}

#[derive(Clone)]
pub struct Example {
    pub text: String,
    pub note: String,
}

pub trait Source {
    fn examples(&self) -> Vec<Example>;
    fn context(&self) -> Value;
}

pub struct Given {
    pub examples: Vec<Example>,
    pub context: Value,
}

impl Source for Given {
    fn examples(&self) -> Vec<Example> {
        self.examples.clone()
    }

    fn context(&self) -> Value {
        self.context.clone()
    }
}

pub fn alternate(source: &dyn Source, size: usize) -> Value {
    let mut request = source.context();
    for example in source.examples() {
        request["marked"] = json!(example.text);
        request["note"] = json!(example.note);
    }
    request["size"] = json!(size);
    request
}

pub struct Model {
    jev: Jev,
    gemini: Gemini,
}

impl Model {
    pub fn open() -> Result<Self> {
        Ok(Self {
            jev: Jev::open()?,
            gemini: Gemini::open()?,
        })
    }

    pub fn alternates(&self, model: &str, request: &Value) -> Result<Vec<Value>> {
        self.gemini.alternates(model, request)
    }

    pub fn candidates(&self, source: &dyn Source) -> Result<Vec<Candidate>> {
        let examples: Vec<Value> = source
            .examples()
            .iter()
            .map(|example| json!({"text": example.text, "note": example.note}))
            .collect();
        self.gemini.candidates(
            MODEL,
            &json!({"examples": examples, "context": source.context()}),
        )
    }

    pub fn scores(
        &self,
        invariants: &BTreeMap<String, String>,
        texts: &[&str],
    ) -> Result<Vec<BTreeMap<String, f64>>> {
        if invariants.is_empty() {
            return Ok(vec![BTreeMap::new(); texts.len()]);
        }
        let states: Vec<Value> = texts
            .iter()
            .flat_map(|part| {
                invariants
                    .values()
                    .map(move |invariant| json!({"part": part, "invariant": invariant}))
            })
            .collect();
        let instructions = BTreeMap::from([(
            "violation".to_string(),
            json!("`part` violates `invariant`."),
        )]);
        Ok(self
            .jev
            .nouls(&states, &instructions)?
            .chunks(invariants.len())
            .map(|nouls| {
                invariants
                    .keys()
                    .cloned()
                    .zip(nouls.iter().flat_map(|noul| noul.values().copied()))
                    .collect()
            })
            .collect())
    }
}
