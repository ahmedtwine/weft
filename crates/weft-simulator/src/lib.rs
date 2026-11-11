mod draw;
mod engine;
mod hub;
mod schema;

use std::collections::BTreeSet;

use serde_json::{Value as Json, json};
use weft_parse::{Check, Diagram, Ran, Span, State, shown};

use engine::{Case, Engine, Halt, INTERESTING, INVALID, OVERRUN, VALID};

pub use hub::{Hub, Peer, Remote, Session};
pub use schema::Choice;

const STEPS: i64 = 20;
const TRIES: usize = 4;
const PATH: f64 = 0.25;
const ORIGIN: &str = "weft";
const START: &str = "start";

#[derive(Default, Clone)]
pub struct Hello {
    pub reads: Vec<String>,
    pub start: Vec<Choice>,
    pub actions: Vec<(String, Choice)>,
}

pub enum Turn<'a> {
    Start(&'a [Json]),
    Run(&'a str, &'a str, &'a Json),
    Read,
    Done,
}

pub enum Report {
    State(State),
    Thrown(String),
    Refused,
}

pub trait Member {
    fn hello(&self) -> &Hello;
    fn turn(&mut self, turn: &Turn) -> Result<Report, String>;
    fn finish(&mut self) -> Result<Ran, String>;
}

#[derive(Clone)]
pub struct Line {
    pub action: Option<(String, String)>,
    pub input: String,
    pub state: State,
    pub evidence: Vec<String>,
}

impl Line {
    pub fn step(&self) -> String {
        self.action
            .as_ref()
            .map_or(START.into(), |(verb, object)| format!("{verb} to={object}"))
    }
}

pub struct Tally {
    pub step: String,
    pub steps: u64,
    pub checks: u64,
}

pub struct Outcome {
    pub cases: u64,
    pub runs: u64,
    pub tally: Vec<Tally>,
    pub trace: Vec<Line>,
    pub failed: bool,
    pub caveat: Option<String>,
    pub ran: Ran,
}

struct Step {
    verb: String,
    object: String,
    owner: usize,
    input: Choice,
}

struct Run<'a> {
    d: &'a Diagram,
    c: &'a Check,
    steps: Vec<Step>,
    runs: u64,
    tally: Vec<Tally>,
    found: Option<Vec<Line>>,
    trace: Vec<Line>,
    longest: Vec<Line>,
    refused: BTreeSet<String>,
}

enum Stepped {
    Next,
    Refused,
    Failed(Vec<String>),
}

type Members<'m> = [Box<dyn Member + 'm>];

impl Turn<'_> {
    pub fn json(&self) -> String {
        match self {
            Turn::Start(inputs) => json!({ "start": inputs }),
            Turn::Run(verb, _, input) => json!({ "run": verb, "input": input }),
            Turn::Read => json!({ "read": true }),
            Turn::Done => json!({ "done": true }),
        }
        .to_string()
    }
}

pub fn view(s: &State) -> String {
    s.iter()
        .map(|(o, p)| format!("{o}: {}", shown(p)))
        .collect::<Vec<_>>()
        .join("; ")
}

fn written(input: &Json) -> String {
    match input {
        Json::Object(fields) => fields
            .iter()
            .map(|(key, value)| format!("{key}={value}"))
            .collect::<Vec<_>>()
            .join(" "),
        other => other.to_string(),
    }
}

fn merged(mut state: State, part: State) -> State {
    for (object, pairs) in part {
        state.entry(object).or_default().extend(pairs);
    }
    state
}

impl Run<'_> {
    fn thrown(&mut self, mut line: Line, error: &str) -> Vec<String> {
        line.evidence = vec![format!("throws {error}")];
        self.trace.push(line.clone());
        line.evidence
    }

    fn started(
        &mut self,
        case: &Case,
        members: &mut Members,
    ) -> Result<Result<State, Vec<String>>, Halt> {
        let mut real = State::new();
        let mut line = Line {
            action: None,
            input: String::new(),
            state: State::new(),
            evidence: Vec::new(),
        };
        self.trace.clear();
        for m in members.iter_mut() {
            let inputs = m
                .hello()
                .start
                .iter()
                .map(|c| case.draw(c))
                .collect::<Result<Vec<Json>, Halt>>()?;
            let report = match inputs.is_empty() {
                true => m.turn(&Turn::Read)?,
                false => {
                    line.input = inputs.iter().map(written).collect::<Vec<_>>().join(" ");
                    m.turn(&Turn::Start(&inputs))?
                }
            };
            match report {
                Report::State(part) => real = merged(real, part),
                Report::Thrown(error) => return Ok(Err(self.thrown(line, &error))),
                Report::Refused => {
                    self.refused.insert(START.into());
                    return Err(Halt::Invalid);
                }
            }
        }
        let judged = self.c.judge(&real, &real, "");
        line.state = real.clone();
        line.evidence = judged.clone().unwrap_or_default();
        self.trace.push(line);
        Ok(judged.map_or(Ok(real), Err))
    }

    fn step(
        &mut self,
        case: &Case,
        members: &mut Members,
        rule: usize,
        real: &mut State,
        model: &mut State,
    ) -> Result<Stepped, Halt> {
        let Step {
            verb,
            object,
            owner,
            ..
        } = &self.steps[rule];
        let (verb, object, owner) = (verb.clone(), object.clone(), *owner);
        let to = self
            .d
            .enabled(model)
            .into_iter()
            .find(|(o, v, _)| o.name == object && *v == verb)
            .map(|(_, _, to)| to);
        let mut line = Line {
            action: Some((verb.clone(), object.clone())),
            input: String::new(),
            state: State::new(),
            evidence: Vec::new(),
        };
        let mut report = Report::Refused;
        for _ in 0..TRIES {
            if to.is_none() || !matches!(report, Report::Refused) {
                break;
            }
            let input = case.draw(&self.steps[rule].input)?;
            line.input = written(&input);
            report = members[owner].turn(&Turn::Run(&verb, &object, &input))?;
        }
        let mut next = match report {
            Report::State(part) => merged(real.clone(), part),
            Report::Refused => {
                if to.is_some() {
                    self.refused.insert(verb);
                }
                return Ok(Stepped::Refused);
            }
            Report::Thrown(error) => return Ok(Stepped::Failed(self.thrown(line, &error))),
        };
        for (_, m) in members.iter_mut().enumerate().filter(|(i, _)| *i != owner) {
            match m.turn(&Turn::Read)? {
                Report::State(part) => next = merged(next, part),
                _ => {
                    return Err(Halt::Fatal(
                        "the runner answers `read` with no state".into(),
                    ));
                }
            }
        }
        model.insert(object, to.unwrap_or_default());
        let applies = self.c.applies(real, &next, &verb);
        self.tally[rule].steps += 1;
        self.tally[rule].checks += u64::from(applies);
        let judged = self.c.judge(real, &next, &verb);
        line.state = next.clone();
        line.evidence = judged.clone().unwrap_or_default();
        self.trace.push(line);
        *real = next;
        Ok(judged.map_or(Stepped::Next, Stepped::Failed))
    }

    fn stepped(
        &mut self,
        case: &mut Case,
        members: &mut Members,
    ) -> Result<Option<Vec<String>>, Halt> {
        let mut real = match self.started(case, members)? {
            Ok(real) => real,
            Err(evidence) => return Ok(Some(evidence)),
        };
        let mut model = self.d.init.clone();
        if case.chance(PATH)? {
            let path: Vec<usize> = self
                .d
                .steps
                .iter()
                .filter_map(|s| {
                    self.steps
                        .iter()
                        .position(|step| step.verb == s.verb && step.object == s.to)
                })
                .collect();
            for rule in path {
                match self.step(case, members, rule, &mut real, &mut model)? {
                    Stepped::Next => {}
                    Stepped::Refused => return Err(Halt::Invalid),
                    Stepped::Failed(evidence) => return Ok(Some(evidence)),
                }
            }
            return Ok(None);
        }
        let rules: Vec<String> = self.tally.iter().map(|t| t.step.clone()).collect();
        case.machine(&rules, STEPS)?;
        while case.round()? {
            let mut rejected = false;
            while let Some(rule) = case.rule()? {
                match self.step(case, members, rule, &mut real, &mut model)? {
                    Stepped::Next => {}
                    Stepped::Refused => {
                        case.rejected()?;
                        rejected = true;
                    }
                    Stepped::Failed(evidence) => {
                        case.close(false)?;
                        return Ok(Some(evidence));
                    }
                }
            }
            case.close(rejected)?;
        }
        case.close(false)?;
        Ok(None)
    }

    fn case(&mut self, mut case: Case, members: &mut Members) -> Result<(), String> {
        self.runs += 1;
        let status = match self.stepped(&mut case, members) {
            Ok(None) => VALID,
            Ok(Some(_)) => INTERESTING,
            Err(Halt::Overrun) => OVERRUN,
            Err(Halt::Invalid) => INVALID,
            Err(Halt::Fatal(e)) => return Err(e),
        };
        match status == INTERESTING {
            true => self.found = Some(self.trace.clone()),
            false if self.trace.len() > self.longest.len() => self.longest = self.trace.clone(),
            false => {}
        }
        case.complete(status, (status == INTERESTING).then_some(ORIGIN))
    }
}

pub fn listed(ran: Ran, tally: &[Tally]) -> Result<Ran, String> {
    let verb = |t: &Tally| t.step.split(' ').next().unwrap_or_default().to_string();
    let spans = ran
        .actions
        .iter()
        .flat_map(|(_, spans)| spans)
        .chain(&ran.start);
    if let Some(s) = spans.clone().find(|s| s.from == 0 || s.to < s.from) {
        return Err(format!(
            "`ran` has the lines {}-{} of `{}`, and lines go up from 1",
            s.from, s.to, s.file
        ));
    }
    if ran.start.is_empty() {
        return Err("the runner sends no `ran` for `start`".into());
    }
    if let Some((v, _)) = ran
        .actions
        .iter()
        .find(|(v, _)| !tally.iter().any(|t| verb(t) == *v))
    {
        return Err(format!("`ran` names `{v}`, the verb of no `on` line"));
    }
    if let Some(v) = tally.iter().filter(|t| t.steps > 0).map(verb).find(|v| {
        !ran.actions
            .iter()
            .any(|(a, spans)| a == v && !spans.is_empty())
    }) {
        return Err(format!("the runner sends no `ran` for the action `{v}`"));
    }
    let mut actions: Vec<(String, Vec<Span>)> = Vec::new();
    for v in tally.iter().map(verb) {
        if let Some(found) = ran.actions.iter().find(|(a, _)| *a == v)
            && !actions.contains(found)
        {
            actions.push(found.clone());
        }
    }
    Ok(Ran {
        start: ran.start,
        actions,
    })
}

fn owners(d: &Diagram, c: &Check, members: &Members) -> Result<Vec<Step>, String> {
    if let Some((o, k)) = c.reads().into_iter().find(|(o, k)| {
        let tag = format!("{o}.{k}");
        !members.iter().any(|m| m.hello().reads.contains(&tag))
    }) {
        return Err(format!(
            "the check reads `{o}.{k}`, and no field of a `read` schema has the tag `{o}.{k}`"
        ));
    }
    if members.iter().all(|m| m.hello().start.is_empty()) {
        return Err("no runner has a `start`".into());
    }
    let mut steps: Vec<Step> = Vec::new();
    for o in &d.objects {
        for m in &o.moves {
            if steps.iter().any(|s| s.verb == m.verb && s.object == o.name) {
                continue;
            }
            let found: Vec<(usize, &Choice)> = members
                .iter()
                .enumerate()
                .flat_map(|(i, member)| {
                    member
                        .hello()
                        .actions
                        .iter()
                        .filter(|(tag, _)| *tag == m.verb)
                        .map(move |(_, input)| (i, input))
                })
                .collect();
            match found.as_slice() {
                [(owner, input)] => steps.push(Step {
                    verb: m.verb.clone(),
                    object: o.name.clone(),
                    owner: *owner,
                    input: (*input).clone(),
                }),
                [] => {
                    return Err(format!(
                        "the verb `{0}` has no `run` input with the tag `{0}`",
                        m.verb
                    ));
                }
                _ => {
                    return Err(format!(
                        "the verb `{}` has {} `run` inputs",
                        m.verb,
                        found.len()
                    ));
                }
            }
        }
    }
    Ok(steps)
}

pub fn simulate(
    d: &Diagram,
    members: &mut Members,
    cases: u64,
    key: Option<&str>,
) -> Result<Outcome, String> {
    let c = d.check.as_ref().ok_or("needs `check`")?;
    let steps = owners(d, c, members)?;
    let mut run = Run {
        d,
        c,
        tally: steps
            .iter()
            .map(|s| Tally {
                step: format!("{} to={}", s.verb, s.object),
                steps: 0,
                checks: 0,
            })
            .collect(),
        steps,
        runs: 0,
        found: None,
        trace: Vec::new(),
        longest: Vec::new(),
        refused: BTreeSet::new(),
    };
    let mut engine = Engine::start(cases, key)?;
    while let Some(case) = engine.next()? {
        run.case(case, members)?;
    }
    let ended = engine.end()?;
    let rans = members
        .iter_mut()
        .map(|m| m.finish())
        .collect::<Result<Vec<Ran>, String>>()?;
    let verb = |t: &Tally| t.step.split(' ').next().unwrap_or_default().to_string();
    if let Some(refused) = [START.to_string()]
        .into_iter()
        .filter(|_| run.longest.is_empty())
        .chain(run.tally.iter().filter(|t| t.steps == 0).map(verb))
        .filter(|_| !ended.failed)
        .find(|v| run.refused.contains(v))
    {
        return Err(format!(
            "each input of `{refused}` fails the schema of its function"
        ));
    }
    let ran = Ran {
        start: rans.iter().flat_map(|r| r.start.clone()).collect(),
        actions: rans.into_iter().flat_map(|r| r.actions).collect(),
    };
    let ran = listed(ran, &run.tally)?;
    let trace = match (ended.failed, run.found) {
        (true, Some(found)) => found,
        (true, None) => return Err("Hegel fails the run with no failing case".into()),
        (false, _) => run.longest,
    };
    Ok(Outcome {
        cases,
        runs: run.runs,
        failed: ended.failed,
        trace,
        tally: run.tally,
        caveat: ended.caveat,
        ran,
    })
}
