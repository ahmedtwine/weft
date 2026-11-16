use std::path::{Path, PathBuf};

use weft_orchestrator::Event;
use weft_parse::{Config, Diagram, Invariant, State};
use weft_samples::{Candidate, Result};

pub struct Seen {
    pub run: u64,
    pub verb: Option<String>,
    pub state: State,
}

pub struct World {
    pub test: PathBuf,
    pub config: Config,
    pub diagram: Diagram,
    pub golden: Vec<Invariant>,
}

pub fn read(path: &Path) -> Result<String> {
    std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))
}

pub fn written(candidate: &Candidate) -> String {
    let lines: Vec<String> = candidate
        .measure
        .lines()
        .map(|line| format!("  {line}"))
        .collect();
    format!(
        "invariant {:?} {{\n{}\n}}",
        candidate.name,
        lines.join("\n")
    )
}

impl World {
    pub fn parse(config: &str, test: &str, path: &Path) -> Result<Self> {
        let config = weft_parse::config(config).map_err(|e| e.to_string())?;
        let mut diagram = weft_parse::parse(test, &config).map_err(|e| e.to_string())?;
        let golden = diagram.invariants().into_iter().cloned().collect();
        for object in &mut diagram.objects {
            object.invariants.clear();
        }
        Ok(Self {
            test: path.to_path_buf(),
            config,
            diagram,
            golden,
        })
    }

    pub fn open(test: &Path) -> Result<Self> {
        Self::parse(&read(Path::new("weft.kdl"))?, &read(test)?, test)
    }

    pub fn run(&self) -> Result<Vec<Seen>> {
        let mut seen = Vec::new();
        let mut run = 0;
        weft_orchestrator::run(
            &self.config,
            &self.diagram,
            &self.test,
            None,
            None,
            &mut |event| match event {
                Event::Run(next) => run = next,
                Event::Step(line) => seen.push(Seen {
                    run,
                    verb: line.action.as_ref().map(|(verb, _)| verb.clone()),
                    state: line.state.clone(),
                }),
                _ => {}
            },
        )?;
        Ok(seen)
    }

    pub fn invariants(&self, candidates: &[Candidate]) -> Result<Vec<Invariant>> {
        let lines: Vec<String> = candidates.iter().map(written).collect();
        let objects =
            self.diagram
                .objects
                .iter()
                .enumerate()
                .fold(String::new(), |out, (at, object)| {
                    let body = if at == 0 {
                        lines.join("\n")
                    } else {
                        String::new()
                    };
                    format!("{out}{} {:?} {{\n{body}\n}}\n", object.kind, object.name)
                });
        weft_parse::parse(&format!("setup {{\n{objects}}}\n"), &self.config)
            .map(|diagram| diagram.invariants().into_iter().cloned().collect())
            .map_err(|e| e.to_string())
    }
}

pub fn broken<'a>(invariant: &Invariant, seen: &'a [Seen]) -> Option<&'a Seen> {
    let mut start = None;
    seen.iter().find(|now| {
        if start.is_none_or(|first: &Seen| first.run != now.run) {
            start = Some(*now);
        }
        start.is_some_and(|first| invariant.judge(&first.state, &now.state).is_some())
    })
}
