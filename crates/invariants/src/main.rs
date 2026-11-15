use std::collections::BTreeMap;
use std::io::BufRead;
use std::path::Path;
use std::process::ExitCode;

use invariants::machine::{Alternate, Machine};
use invariants::provider::gemini::Gemini;
use invariants::provider::jev::Jev;
use invariants::{Medium, Part, Result, lint, medium};
use serde::Deserialize;

const USAGE: &str =
    "usage: invariants check <file> <invariants.json> | narrow <file> | test <file>";

#[derive(Deserialize)]
struct Example {
    #[serde(default)]
    annotations: Vec<(String, String)>,
    invariants: BTreeMap<String, String>,
    #[serde(default)]
    violations: BTreeMap<String, Vec<String>>,
}

struct Artifact<'a> {
    file: &'a Path,
    text: String,
    medium: Box<dyn Medium + Send + Sync>,
    parts: Vec<Part>,
    jev: Jev,
    gemini: Gemini,
}

fn read(path: &Path) -> Result<String> {
    std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))
}

fn example(path: &Path) -> Result<Example> {
    serde_json::from_str(&read(path)?).map_err(|e| format!("{}: {e}", path.display()))
}

impl<'a> Artifact<'a> {
    fn open(file: &'a Path) -> Result<Self> {
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

    fn place(&self, part: usize) -> String {
        let at = self.parts[part].span.start;
        let begin = self.text[..at].rfind('\n').map_or(0, |i| i + 1);
        format!(
            "{}:{}:{}",
            self.file.display(),
            self.text[..at].matches('\n').count() + 1,
            at - begin + 1
        )
    }

    fn violations(&self, invariants: &BTreeMap<String, String>) -> Result<Vec<(usize, String)>> {
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

    fn wrong(&self, example: &Example, invariants: &BTreeMap<String, String>) -> Result<usize> {
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

    fn narrow(
        &self,
        example: &Example,
        mut react: impl FnMut(&Alternate) -> Option<bool>,
    ) -> Result<BTreeMap<String, String>> {
        let mut found = BTreeMap::new();
        for (marked, note) in &example.annotations {
            println!("annotation  {marked:?}  {note:?}");
            let mut machine = Machine::new(
                (&self.jev, &self.gemini),
                self.medium.as_ref(),
                &self.text,
                (marked, note),
                &example.invariants,
            )?;
            while let Some(round) = machine.round() {
                let alternate = &machine.alternates[round];
                let Some(reject) = react(alternate) else {
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
                    found.insert(invariant.name, invariant.measure);
                }
                None => println!("  no invariant agrees with each reaction"),
            }
        }
        Ok(found)
    }
}

fn asked(alternate: &Alternate) -> Option<bool> {
    println!(
        "  {}  {}  {:?}",
        alternate.kind, alternate.change, alternate.text
    );
    println!("  reject (r) or accept (a)?");
    std::io::stdin()
        .lock()
        .lines()
        .map_while(|line| line.ok())
        .find_map(|line| match line.trim() {
            "r" => Some(true),
            "a" => Some(false),
            _ => None,
        })
}

fn narrow(file: &Path) -> Result<()> {
    let artifact = Artifact::open(file)?;
    let found = artifact.narrow(&example(&file.with_extension("json"))?, asked)?;
    artifact.violations(&found).map(|_| ())
}

fn test(file: &Path) -> Result<()> {
    let artifact = Artifact::open(file)?;
    let example = example(&file.with_extension("json"))?;
    println!("golden invariants");
    let golden = artifact.wrong(&example, &example.invariants)?;
    let found = artifact.narrow(&example, |alternate| {
        Some(
            alternate
                .invariants
                .iter()
                .any(|name| example.invariants.contains_key(name)),
        )
    })?;
    println!("narrowed invariants");
    let narrowed = artifact.wrong(&example, &found)?;
    match (golden, narrowed) {
        (0, 0) => Ok(()),
        _ => Err(format!(
            "{}: {golden} wrong violations with the golden invariants, {narrowed} wrong violations with the narrowed invariants",
            file.display()
        )),
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let done = match args.iter().map(String::as_str).collect::<Vec<_>>()[..] {
        ["check", file, invariants] => Artifact::open(Path::new(file)).and_then(|artifact| {
            artifact
                .violations(&example(Path::new(invariants))?.invariants)
                .map(|_| ())
        }),
        ["narrow", file] => narrow(Path::new(file)),
        ["test", file] => test(Path::new(file)),
        _ => Err(USAGE.into()),
    };
    match done {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}
