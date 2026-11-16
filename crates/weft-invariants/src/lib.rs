pub mod cover;
pub mod states;
pub mod table;
pub mod template;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use weft_parse::{Invariant, State};
use weft_samples::model::{Example, Given, Live};
use weft_samples::{Candidate, Result};

use cover::Set;
use states::{Seen, World, broken, read, written};
use table::{Row, Table};

const SPACE: usize = 1 << 20;
const GRAMMAR: &str = "A driver records the state of the system after each action. A measure is one condition on one state that is true after each action, in each order of actions. A condition is one line `<op> <term> <term>` or one block `<op> {` with one condition on each line and `}` on the last line. The ops of a block are and, or, not, implies and iff; implies has two lines, the condition and its result. The ops of a line are eq, neq and in. A term is a key of the state as \"<object>.<key>\", a string in quotes, or an integer. `in \"<object>.<key>\" <value> <value>` is true when the key has one of the values. A line ends at the end of its text line. Example:\nimplies {\n  and {\n    eq \"door.event\" \"open\"\n    eq \"door.error\" \"\"\n  }\n  eq \"door.state\" \"opened\"\n}";

#[derive(Default)]
pub struct Options {
    pub cases: Option<u64>,
    pub steps: Option<u64>,
    pub examples: Vec<PathBuf>,
}

pub struct Found {
    pub source: &'static str,
    pub candidate: Candidate,
    pub invariant: Invariant,
    pub forbids: Set,
}

pub struct Space {
    pub table: Table,
    pub start: State,
}

impl Space {
    pub fn of(states: &[&State]) -> Result<Self> {
        let table = Table::of(states);
        match table.size() > SPACE {
            true => Err(format!(
                "the space has {} states, more than {SPACE}",
                table.size()
            )),
            false => Ok(Self {
                table,
                start: states
                    .first()
                    .map(|state| (*state).clone())
                    .unwrap_or_default(),
            }),
        }
    }

    pub fn forbidden(&self, invariant: &Invariant) -> Set {
        let mut set = cover::empty(self.table.size());
        let rows = (0..self.table.size()).filter(|&at| {
            invariant
                .judge(&self.start, &self.table.state(&self.table.row(at)))
                .is_some()
        });
        for at in rows {
            cover::put(&mut set, at);
        }
        set
    }

    pub fn found(
        &self,
        world: &World,
        source: &'static str,
        candidates: Vec<Candidate>,
    ) -> Result<Vec<Found>> {
        Ok(world
            .invariants(&candidates)?
            .into_iter()
            .zip(candidates)
            .map(|(invariant, candidate)| Found {
                source,
                forbids: self.forbidden(&invariant),
                candidate,
                invariant,
            })
            .collect())
    }

    pub fn fewest(&self, world: &World, rows: &BTreeSet<Row>) -> Result<Vec<Found>> {
        let mut candidates: Vec<Candidate> = template::implications(&self.table, rows)
            .iter()
            .map(|implication| implication.candidate(&self.table))
            .collect();
        candidates.sort_by(|a, b| a.name.cmp(&b.name));
        candidates.dedup_by(|a, b| a.name == b.name);
        let mut found: Vec<Option<Found>> = self
            .found(world, "search", candidates)?
            .into_iter()
            .map(Some)
            .collect();
        let costed: Vec<(&Set, usize)> = found
            .iter()
            .flatten()
            .map(|found| (&found.forbids, found.candidate.measure.lines().count()))
            .collect();
        let chosen = cover::greedy(&costed);
        Ok(chosen
            .into_iter()
            .filter_map(|at| found[at].take())
            .collect())
    }
}

fn told(found: &Found, seen: &[Seen], others: &[Found]) {
    let held = match broken(&found.invariant, seen) {
        Some(_) => "false in the search",
        None => "true in the search",
    };
    let twin = others
        .iter()
        .find(|other| other.forbids == found.forbids && other.source != found.source)
        .map_or_else(String::new, |other| {
            format!("  = {} {:?}", other.source, other.invariant.source())
        });
    println!(
        "  {:>5}  {held}  {}  {:?}{twin}",
        cover::count(&found.forbids),
        found.source,
        found.invariant.source()
    );
}

fn modeled(
    space: &Space,
    world: &World,
    seen: &[Seen],
    examples: &[PathBuf],
) -> Result<Vec<Found>> {
    let examples = examples
        .iter()
        .map(|path| {
            Ok(Example {
                text: read(path)?,
                note: path.display().to_string(),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let keys: Vec<Value> = (0..space.table.keys.len())
        .map(|key| json!({"key": space.table.name(key), "values": space.table.values[key]}))
        .collect();
    let actions: BTreeSet<&String> = seen.iter().filter_map(|seen| seen.verb.as_ref()).collect();
    let given = Given {
        examples,
        context: json!({"keys": keys, "actions": actions, "grammar": GRAMMAR}),
    };
    let mut found = Vec::new();
    for candidate in Live::open()?.candidates(&given)? {
        match space.found(world, "model", vec![candidate.clone()]) {
            Ok(one) => found.extend(one),
            Err(e) => println!(
                "      -  not weft  model  {:?}  {}",
                candidate.name,
                e.lines().next().unwrap_or_default()
            ),
        }
    }
    Ok(found)
}

pub fn mine(dir: &Path, test: &Path, options: &Options) -> Result<()> {
    let examples: Vec<PathBuf> = options
        .examples
        .iter()
        .map(std::fs::canonicalize)
        .collect::<std::io::Result<_>>()
        .map_err(|e| e.to_string())?;
    std::env::set_current_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let mut world = World::open(test)?;
    world.diagram.run.cases = options.cases.unwrap_or(world.diagram.run.cases);
    world.diagram.run.steps = options.steps.unwrap_or(world.diagram.run.steps);
    let seen = world.run()?;
    let states: Vec<&State> = seen.iter().map(|seen| &seen.state).collect();
    let space = Space::of(&states)?;
    let rows = space.table.rows(&states);
    println!(
        "states  {} runs  {} states  {} distinct  space {}",
        seen.iter().map(|seen| seen.run).max().unwrap_or(0),
        states.len(),
        rows.len(),
        space.table.size()
    );
    let golden: Vec<Found> = world
        .golden
        .iter()
        .map(|invariant| Found {
            source: "golden",
            candidate: Candidate {
                name: invariant.source(),
                measure: String::new(),
            },
            forbids: space.forbidden(invariant),
            invariant: invariant.clone(),
        })
        .collect();
    let mut found = space.fewest(&world, &rows)?;
    let mut searched = cover::empty(space.table.size());
    for one in &found {
        cover::join(&mut searched, &one.forbids);
    }
    println!(
        "search  {} candidates forbid {} of {} states that are not reached",
        found.len(),
        cover::count(&searched),
        space.table.size() - rows.len()
    );
    if !examples.is_empty() {
        found.extend(modeled(&space, &world, &seen, &examples)?);
    }
    for one in &found {
        told(one, &seen, &golden);
    }
    println!("golden  {}", golden.len());
    for one in &golden {
        told(one, &seen, &found);
        if !cover::within(&one.forbids, &searched) {
            println!("         not inside the search set");
        }
    }
    println!("invariants");
    for one in found
        .iter()
        .filter(|one| broken(&one.invariant, &seen).is_none())
    {
        println!("{}", written(&one.candidate));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONFIG: &str = "include \"*.weft.kdl\"\nrun cases=100 steps=50 check=\"sampled\"\nstyle \"api\" icon=\"server\"\n";
    const TEST: &str =
        "setup {\n  api \"door\"\n}\nimpl \"bun\" {\n  module \"door\" \"../door.ts\"\n}\n";

    fn door(event: &str, previous: &str, state: &str) -> State {
        State::from([(
            "door".to_string(),
            [("event", event), ("previous", previous), ("state", state)]
                .into_iter()
                .map(|(key, value)| (key.to_string(), json!(value)))
                .collect(),
        )])
    }

    #[test]
    fn the_fewest_candidates_forbid_each_state_that_is_not_reached() {
        let world = World::parse(CONFIG, TEST, Path::new("door.weft.kdl")).unwrap();
        let reached = [
            door("", "closed", "closed"),
            door("open", "closed", "opened"),
            door("open", "opened", "opened"),
            door("close", "opened", "closed"),
            door("close", "closed", "closed"),
            door("knock", "closed", "closed"),
            door("knock", "opened", "opened"),
        ];
        let states: Vec<&State> = reached.iter().collect();
        let space = Space::of(&states).unwrap();
        let rows = space.table.rows(&states);
        let found = space.fewest(&world, &rows).unwrap();
        let names: Vec<&str> = found
            .iter()
            .map(|one| one.candidate.name.as_str())
            .collect();
        let mut forbidden = cover::empty(space.table.size());
        for one in &found {
            cover::join(&mut forbidden, &one.forbids);
        }
        assert_eq!(cover::count(&forbidden), space.table.size() - rows.len());
        assert_eq!(
            names,
            [
                "event  or knock gives previous equal to state",
                "event  or close gives state closed",
                "event open gives state opened"
            ]
        );
        assert!(reached.iter().all(|state| {
            found
                .iter()
                .all(|one| one.invariant.judge(&space.start, state).is_none())
        }));
    }
}
