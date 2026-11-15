pub mod artifact;
pub mod machine;
pub mod medium;

pub mod provider {
    pub mod gemini;
    pub mod jev;

    use std::hash::{DefaultHasher, Hash, Hasher};
    use std::path::PathBuf;

    use serde::Serialize;
    use serde::de::DeserializeOwned;

    use crate::Result;

    pub(crate) fn home() -> PathBuf {
        PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
    }

    pub(crate) fn secret(name: &str) -> Result<String> {
        let file = std::fs::read_to_string(home().join(".config/invariants/secrets.env"))
            .unwrap_or_default();
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
                "no {name} in the environment or in ~/.config/invariants/secrets.env"
            ))
    }

    pub(crate) async fn cached<T: Serialize + DeserializeOwned>(
        key: impl Hash,
        fetch: impl Future<Output = Result<T>>,
    ) -> Result<T> {
        let cache = home().join(".cache/invariants");
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
}

use std::collections::BTreeMap;
use std::ops::Range;

use serde_json::{Value, json};

use crate::provider::gemini::Gemini;
use crate::provider::jev::Jev;

pub const THRESHOLD: f64 = 0.82;

pub type Result<T> = std::result::Result<T, String>;

pub struct Part {
    pub span: Range<usize>,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Generator {
    Integer(i64, i64),
    SampledFrom(Vec<Value>),
    Model(String, Value),
}

pub struct Rule {
    pub verb: String,
    pub drawn: Generator,
}

pub trait Medium {
    fn parts(&self, artifact: &str) -> Vec<Part>;
    fn rules(&self, artifact: &str, part: &Part, marked: Range<usize>) -> Vec<Rule>;
    fn run(&self, artifact: &str, part: &Part, verb: &str, input: &str) -> Option<String>;
}

pub struct Invariant {
    pub name: String,
    pub measure: String,
    pub rejected: Vec<String>,
    pub accepted: Vec<String>,
}

pub struct Violation {
    pub part: usize,
    pub invariant: String,
    pub score: f64,
}

impl Generator {
    pub fn kind(&self) -> &'static str {
        match self {
            Generator::Integer(..) => "hegel.integer",
            Generator::SampledFrom(_) => "hegel.sampled_from",
            Generator::Model(..) => "model.alternate",
        }
    }

    pub fn values(&self, gemini: &Gemini) -> Result<Vec<Value>> {
        match self {
            Generator::Integer(min, max) => Ok((*min..=*max).map(Value::from).collect()),
            Generator::SampledFrom(options) => Ok(options.clone()),
            Generator::Model(model, context) => gemini.draw(model, context),
        }
    }
}

pub fn scores(
    jev: &Jev,
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
    Ok(jev
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

pub fn lint(
    jev: &Jev,
    invariants: &BTreeMap<String, String>,
    parts: &[Part],
) -> Result<Vec<Violation>> {
    let texts: Vec<&str> = parts.iter().map(|p| p.text.as_str()).collect();
    Ok(scores(jev, invariants, &texts)?
        .into_iter()
        .enumerate()
        .flat_map(|(part, scores)| {
            scores
                .into_iter()
                .filter(|(_, score)| *score >= THRESHOLD)
                .map(move |(invariant, score)| Violation {
                    part,
                    invariant,
                    score,
                })
        })
        .collect())
}
