use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use weft_samples::artifact::{Artifact, example};
use weft_samples::model::Model;

const LITERAL: &str = "Each part is free of the text ";

struct Golden {
    invariants: BTreeMap<String, String>,
    violations: BTreeMap<String, Vec<String>>,
}

impl Golden {
    fn texts(&self, measure: &str) -> Vec<String> {
        match measure
            .strip_prefix(LITERAL)
            .and_then(|quoted| quoted.strip_suffix('.'))
        {
            Some(quoted) => serde_json::from_str(quoted).into_iter().collect(),
            None => self
                .invariants
                .iter()
                .filter(|(_, known)| *known == measure)
                .flat_map(|(name, _)| self.violations.get(name).cloned().unwrap_or_default())
                .collect(),
        }
    }
}

impl Model for Golden {
    fn alternates(&self, _: &str, request: &Value) -> Result<Vec<Value>> {
        let part = request["part"].as_str().unwrap_or_default();
        Ok(self
            .invariants
            .iter()
            .map(|(name, measure)| {
                let text = self
                    .texts(measure)
                    .iter()
                    .fold(part.to_string(), |text, cut| text.replace(cut.as_str(), ""));
                json!({"text": text, "name": name, "measure": measure})
            })
            .collect())
    }

    fn scores(
        &self,
        invariants: &BTreeMap<String, String>,
        texts: &[&str],
    ) -> Result<Vec<BTreeMap<String, f64>>> {
        Ok(texts
            .iter()
            .map(|text| {
                invariants
                    .iter()
                    .map(|(name, measure)| {
                        let violated = self
                            .texts(measure)
                            .iter()
                            .any(|cut| text.contains(cut.as_str()));
                        (name.clone(), f64::from(u8::from(violated)))
                    })
                    .collect()
            })
            .collect())
    }
}
use weft_samples::{Candidate, Result, THRESHOLD, measures};

fn flow(file: &Path) -> Result<()> {
    let example = example(&file.with_extension("json"))?;
    let golden = Golden {
        invariants: example.invariants.clone(),
        violations: example.violations.clone(),
    };
    let artifact = Artifact::open(file, Box::new(golden))?;
    println!("golden invariants");
    let golden = artifact.wrong(&example, &example.invariants)?;
    let expected: BTreeMap<&String, BTreeSet<usize>> = example
        .violations
        .iter()
        .map(|(name, texts)| (name, artifact.expected(texts)))
        .collect();
    let owner = |marked: &str| {
        expected
            .iter()
            .find(|(_, parts)| {
                parts
                    .iter()
                    .any(|&p| artifact.parts[p].text.contains(marked))
            })
            .map(|(name, _)| name.to_string())
    };
    let machines = artifact.narrow(&example, |marked, alternate| {
        let name = owner(marked)?;
        let measure = example.invariants.get(&name)?.clone();
        let scored = artifact
            .model
            .scores(
                &BTreeMap::from([(name.clone(), measure)]),
                &[alternate.text.as_str()],
            )
            .ok()?;
        Some(scored.first()?.get(&name)? >= &THRESHOLD)
    })?;
    println!("flow");
    let mut failed = 0;
    for ((marked, _), machine) in example.annotations.iter().zip(&machines) {
        let Some(name) = owner(marked) else {
            println!("  golden  {marked:?} is in no golden violation");
            failed += 1;
            continue;
        };
        let target = &expected[&name];
        let flagged = artifact.flagged(&measures(&machine.invariants))?;
        let none = BTreeSet::new();
        let parts = |candidate: &str| flagged.get(candidate).unwrap_or(&none);
        let wrong = |candidate: &str| parts(candidate).symmetric_difference(target).count();
        let Some(Candidate {
            name: best,
            measure,
        }) = machine.invariants.iter().min_by_key(|c| wrong(&c.name))
        else {
            continue;
        };
        let chosen = machine.invariant().map(|i| i.name);
        match chosen.as_deref() {
            _ if wrong(best) > 0 => {
                println!(
                    "  draw  {marked:?}  no drawn invariant gives {name}; the nearest is {best}: {measure}, {} wrong",
                    wrong(best)
                );
                artifact.differ(parts(best), target);
                failed += 1;
            }
            Some(chosen) if wrong(chosen) == 0 => {
                println!("  pass  {marked:?}  {chosen} gives {name}")
            }
            chosen => {
                println!(
                    "  reactions  {marked:?}  the reactions chose {}, but {best} gives {name}",
                    chosen.unwrap_or("no invariant")
                );
                if let Some(chosen) = chosen {
                    artifact.differ(parts(chosen), target);
                }
                failed += 1;
            }
        }
    }
    match (golden, failed) {
        (0, 0) => Ok(()),
        _ => Err(format!(
            "{}: {golden} wrong violations with the golden invariants, {failed} of {} annotations fail in the flow",
            file.display(),
            example.annotations.len()
        )),
    }
}

fn path(name: &str) -> PathBuf {
    Path::new("examples").join(name)
}

#[test]
fn writing() {
    flow(&path("writing.md")).unwrap();
}

#[test]
fn code() {
    flow(&path("code.rs")).unwrap();
}
