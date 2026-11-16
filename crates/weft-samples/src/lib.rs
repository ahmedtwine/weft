pub mod artifact;
pub mod machine;
pub mod medium;
pub mod model;

use std::collections::BTreeMap;
use std::ops::Range;

use serde_json::Value;

use crate::model::Model;

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub name: String,
    pub measure: String,
}

pub fn measures(candidates: &[Candidate]) -> BTreeMap<String, String> {
    candidates
        .iter()
        .map(|candidate| (candidate.name.clone(), candidate.measure.clone()))
        .collect()
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

    pub fn values(&self, model: &dyn Model) -> Result<Vec<Value>> {
        match self {
            Generator::Integer(min, max) => Ok((*min..=*max).map(Value::from).collect()),
            Generator::SampledFrom(options) => Ok(options.clone()),
            Generator::Model(name, request) => model.alternates(name, request),
        }
    }
}

pub fn lint(
    model: &dyn Model,
    invariants: &BTreeMap<String, String>,
    parts: &[Part],
) -> Result<Vec<Violation>> {
    let texts: Vec<&str> = parts.iter().map(|p| p.text.as_str()).collect();
    Ok(model
        .scores(invariants, &texts)?
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
