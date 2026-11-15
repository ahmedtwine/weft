pub mod jev;
pub mod medium;
pub mod narrow;

use std::collections::BTreeMap;
use std::ops::Range;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::jev::Jev;

pub type Result<T> = std::result::Result<T, String>;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Part {
    pub span: Range<usize>,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Generator {
    Integer(i64, i64),
    SampledFrom(Vec<String>),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Action {
    pub verb: String,
    pub drawn: Generator,
}

pub trait Medium {
    fn parts(&self, artifact: &str) -> Vec<Part>;
    fn actions(&self, artifact: &str, part: &Part, marked: Range<usize>) -> Vec<Action>;
    fn run(&self, artifact: &str, part: &Part, verb: &str, input: &str) -> Option<String>;
    fn cut(&self, text: &str, cut: &[String]) -> bool;
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Measure {
    Pattern(String),
    Choice(Choice),
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Choice {
    pub cut: Vec<String>,
    pub question: String,
    pub flag: BTreeMap<String, String>,
    pub pass: BTreeMap<String, String>,
    pub split: BTreeMap<String, String>,
    pub then: Option<Box<Choice>>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Rule {
    pub measure: Measure,
    pub threshold: f64,
    pub rejected: Vec<String>,
    pub accepted: Vec<String>,
}

impl Generator {
    pub fn values(&self) -> Vec<String> {
        match self {
            Generator::Integer(lo, hi) => (*lo..=*hi).map(|i| i.to_string()).collect(),
            Generator::SampledFrom(options) => options.clone(),
        }
    }
}

pub fn scores(
    jev: &Jev,
    medium: &dyn Medium,
    measure: &Measure,
    texts: &[String],
    context: &str,
) -> Result<Vec<f64>> {
    let cut: &[String] = if let Measure::Choice(choice) = measure {
        &choice.cut
    } else {
        &[]
    };
    let kept: Vec<usize> = (0..texts.len())
        .filter(|&i| cut.is_empty() || medium.cut(&texts[i], cut))
        .collect();
    let picked: Vec<(String, String)> = kept
        .iter()
        .map(|&i| (texts[i].clone(), context.to_string()))
        .collect();
    let mut out = vec![0.0; texts.len()];
    kept.into_iter()
        .zip(jev.scores(measure, &picked)?)
        .for_each(|(i, score)| out[i] = score);
    Ok(out)
}

pub fn changed(medium: &dyn Medium, old: &str, new: &str) -> String {
    let lead = old
        .bytes()
        .zip(new.bytes())
        .take_while(|(a, b)| a == b)
        .count();
    let trail = old
        .bytes()
        .rev()
        .zip(new.bytes().rev())
        .take_while(|(a, b)| a == b)
        .count()
        .min(old.len().min(new.len()) - lead);
    let end = new.len() - trail;
    medium
        .parts(new)
        .into_iter()
        .filter(|p| p.span.start <= lead && end <= p.span.end)
        .min_by_key(|p| p.span.len())
        .map_or_else(|| new[lead..end].to_string(), |p| p.text)
}

pub fn flagged(
    jev: &Jev,
    medium: &dyn Medium,
    measure: &Measure,
    artifact: &str,
) -> Result<Vec<usize>> {
    let parts = medium.parts(artifact);
    let texts: Vec<String> = parts.iter().map(|p| p.text.clone()).collect();
    Ok(parts
        .iter()
        .zip(scores(jev, medium, measure, &texts, artifact)?)
        .filter(|(_, s)| *s >= 0.5)
        .map(|(p, _)| p.span.start)
        .collect())
}

pub fn sameness(jev: &Jev, artifact: &str, from: &str, to: &str) -> Result<f64> {
    let choice = Choice {
        question: "`text` renames a name in `context`. Which relation holds between the meaning of the old name and the meaning of the new name, for the object in `context`?".into(),
        flag: BTreeMap::from([("same".into(), "the same meaning, or a more precise meaning for the same object".into())]),
        pass: BTreeMap::from([("different".into(), "a meaning that fits a different object".into())]),
        ..Choice::default()
    };
    Ok(jev.scores(
        &Measure::Choice(choice),
        &[(format!("{from} to {to}"), artifact.to_string())],
    )?[0])
}

pub fn home() -> PathBuf {
    std::env::var_os("INVARIANTS_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".config/invariants")
        })
}

pub fn path() -> PathBuf {
    home().join("rules.json")
}

pub fn load() -> Vec<Rule> {
    std::fs::read(path())
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

pub fn save(rules: &[Rule]) -> Result<()> {
    std::fs::create_dir_all(home()).map_err(|e| e.to_string())?;
    let text = serde_json::to_string_pretty(rules).map_err(|e| e.to_string())?;
    std::fs::write(path(), text + "\n").map_err(|e| e.to_string())
}
