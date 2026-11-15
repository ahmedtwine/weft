use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde::Deserialize;

use crate::machine::{Alternate, Machine};
use crate::provider::gemini::Gemini;
use crate::provider::jev::Jev;
use crate::{Medium, Part, Result, lint, medium};

#[derive(Deserialize)]
pub struct Example {
    #[serde(default)]
    pub annotations: Vec<(String, String)>,
    pub invariants: BTreeMap<String, String>,
    #[serde(default)]
    pub violations: BTreeMap<String, Vec<String>>,
}

pub struct Artifact<'a> {
    pub file: &'a Path,
    pub text: String,
    pub medium: Box<dyn Medium + Send + Sync>,
    pub parts: Vec<Part>,
    pub jev: Jev,
    pub gemini: Gemini,
}

pub fn read(path: &Path) -> Result<String> {
    std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))
}

pub fn example(path: &Path) -> Result<Example> {
    serde_json::from_str(&read(path)?).map_err(|e| format!("{}: {e}", path.display()))
}

impl<'a> Artifact<'a> {
    pub fn open(file: &'a Path) -> Result<Self> {
        let text = read(file)?;
        let medium = medium::of(&file.to_string_lossy());
        Ok(Self {
            file,
            parts: medium.parts(&text),
            text,
            medium,
            jev: Jev::open()?,
            gemini: Gemini::open()?,
        })
    }

    pub fn place(&self, part: usize) -> String {
        let at = self.parts[part].span.start;
        let begin = self.text[..at].rfind('\n').map_or(0, |i| i + 1);
        format!(
            "{}:{}:{}",
            self.file.display(),
            self.text[..at].matches('\n').count() + 1,
            at - begin + 1
        )
    }

    pub fn violations(
        &self,
        invariants: &BTreeMap<String, String>,
    ) -> Result<Vec<(usize, String)>> {
        let found = lint(&self.jev, invariants, &self.parts)?;
        for v in &found {
            println!(
                "{}  {}  {:.2}  {:?}",
                self.place(v.part),
                v.invariant,
                v.score,
                self.parts[v.part].text
            );
        }
        Ok(found.into_iter().map(|v| (v.part, v.invariant)).collect())
    }

    pub fn wrong(&self, example: &Example, invariants: &BTreeMap<String, String>) -> Result<usize> {
        let found = self.violations(invariants)?;
        let mut expected: Vec<(usize, String)> = Vec::new();
        let mut wrong = 0;
        for (invariant, texts) in &example.violations {
            for text in texts {
                let at = expected.len();
                expected.extend(
                    (0..self.parts.len())
                        .filter(|&i| self.parts[i].text.contains(text.as_str()))
                        .map(|i| (i, invariant.clone())),
                );
                if at == expected.len() {
                    println!("absent  {invariant}  {text:?}");
                    wrong += 1;
                }
            }
        }
        let differ = |tag: &str, from: &[(usize, String)], other: &[(usize, String)]| {
            from.iter()
                .filter(|v| !other.contains(v))
                .inspect(|(part, invariant)| {
                    println!(
                        "{tag}  {}  {invariant}  {:?}",
                        self.place(*part),
                        self.parts[*part].text
                    )
                })
                .count()
        };
        Ok(wrong + differ("missed", &expected, &found) + differ("false", &found, &expected))
    }

    pub fn narrow(
        &self,
        example: &Example,
        mut react: impl FnMut(&str, &Alternate) -> Option<bool>,
    ) -> Result<Vec<Machine>> {
        let mut machines = Vec::new();
        for (marked, note) in &example.annotations {
            println!("annotation  {marked:?}  {note:?}");
            let mut machine = Machine::new(
                (&self.jev, &self.gemini),
                self.medium.as_ref(),
                &self.text,
                (marked, note),
            )?;
            while let Some(round) = machine.round() {
                let alternate = &machine.alternates[round];
                let Some(reject) = react(marked, alternate) else {
                    break;
                };
                let tag = if reject { "reject" } else { "accept" };
                println!(
                    "  {tag}  {}  {}  {:?}",
                    alternate.kind, alternate.change, alternate.text
                );
                machine.react(round, reject);
            }
            match machine.invariant() {
                Some(invariant) => {
                    println!(
                        "  invariant  {}  {}  {} rejected, {} accepted",
                        invariant.name,
                        invariant.measure,
                        invariant.rejected.len(),
                        invariant.accepted.len()
                    );
                }
                None => println!("  no invariant agrees with each reaction"),
            }
            machines.push(machine);
        }
        Ok(machines)
    }

    pub fn expected(&self, texts: &[String]) -> BTreeSet<usize> {
        (0..self.parts.len())
            .filter(|&i| {
                texts
                    .iter()
                    .any(|t| self.parts[i].text.contains(t.as_str()))
            })
            .collect()
    }

    pub fn flagged(
        &self,
        invariants: &BTreeMap<String, String>,
    ) -> Result<BTreeMap<String, BTreeSet<usize>>> {
        let mut flagged: BTreeMap<String, BTreeSet<usize>> = BTreeMap::new();
        for v in lint(&self.jev, invariants, &self.parts)? {
            flagged.entry(v.invariant).or_default().insert(v.part);
        }
        Ok(flagged)
    }

    pub fn differ(&self, found: &BTreeSet<usize>, expected: &BTreeSet<usize>) {
        for (tag, from, other) in [("missed", expected, found), ("false", found, expected)] {
            for part in from.difference(other) {
                println!(
                    "    {tag}  {}  {:?}",
                    self.place(*part),
                    self.parts[*part].text
                );
            }
        }
    }
}
