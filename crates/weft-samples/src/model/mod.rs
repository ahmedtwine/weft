mod gemini;
mod jev;

use std::collections::BTreeMap;
use std::fs;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use crate::{Candidate, Result};

pub use gemini::MODEL;

const NAME: &str = env!("CARGO_PKG_NAME");

fn home(kind: &str) -> PathBuf {
    PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
        .join(kind)
        .join(NAME)
}

fn secret(name: &str) -> Result<String> {
    let file = fs::read_to_string(home(".config").join("secrets.env")).unwrap_or_default();
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

enum Store {
    Cache(PathBuf),
    Recorded(BTreeMap<String, Value>),
}

impl Store {
    async fn cached<T: Serialize + DeserializeOwned>(
        &self,
        key: impl Hash,
        fetch: impl Future<Output = Result<T>>,
    ) -> Result<T> {
        let mut hasher = DefaultHasher::new();
        key.hash(&mut hasher);
        let name = format!("{:016x}", hasher.finish());
        match self {
            Store::Recorded(answers) => answers
                .get(&name)
                .ok_or(format!("no recorded answer {name}"))
                .and_then(|answer| {
                    serde_json::from_value(answer.clone()).map_err(|e| format!("{name}: {e}"))
                }),
            Store::Cache(dir) => {
                let file = dir.join(&name);
                if let Some(found) = fs::read(&file)
                    .ok()
                    .and_then(|b| serde_json::from_slice(&b).ok())
                {
                    return Ok(found);
                }
                let fetched = fetch.await?;
                fs::write(&file, serde_json::to_vec(&fetched).unwrap_or_default()).ok();
                Ok(fetched)
            }
        }
    }
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
    runtime: tokio::runtime::Runtime,
    store: Store,
}

impl Model {
    pub fn open() -> Result<Self> {
        let cache = home(".cache");
        fs::create_dir_all(&cache).map_err(|e| e.to_string())?;
        Self::with(Store::Cache(cache))
    }

    pub fn recorded(file: &Path) -> Result<Self> {
        let failed = |e: &dyn std::fmt::Display| format!("{}: {e}", file.display());
        let text = fs::read_to_string(file).map_err(|e| failed(&e))?;
        Self::with(Store::Recorded(
            serde_json::from_str(&text).map_err(|e| failed(&e))?,
        ))
    }

    fn with(store: Store) -> Result<Self> {
        Ok(Self {
            runtime: tokio::runtime::Runtime::new().map_err(|e| e.to_string())?,
            store,
        })
    }

    pub fn alternates(&self, model: &str, request: &Value) -> Result<Vec<Value>> {
        self.runtime
            .block_on(gemini::alternates(&self.store, model, request))
    }

    pub fn candidates(&self, source: &dyn Source) -> Result<Vec<Candidate>> {
        let examples: Vec<Value> = source
            .examples()
            .iter()
            .map(|example| json!({"text": example.text, "note": example.note}))
            .collect();
        self.runtime.block_on(gemini::candidates(
            &self.store,
            MODEL,
            &json!({"examples": examples, "context": source.context()}),
        ))
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
            .runtime
            .block_on(jev::nouls(&self.store, &states, &instructions))?
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
