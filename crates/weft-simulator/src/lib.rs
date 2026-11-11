mod hub;
mod schema;

use std::collections::BTreeSet;

use serde_json::{Value as Json, json};
use weft_hegel::{Case, Engine, Halt, INTERESTING, INVALID, OVERRUN, VALID};
use weft_parse::{Check, Diagram, Ran, Span, State, Value, shown};

pub use hub::{Hub, Peer, Remote, Session};
pub use weft_hegel::Generator;

const STEPS: i64 = 20;
const TRIES: usize = 4;
const PATH: f64 = 0.25;
const ORIGIN: &str = "weft";
const START: &str = "start";

pub struct Start {
    pub line: usize,
    pub input: Option<Generator>,
    pub pool: Option<String>,
}

pub struct Action {
    pub verb: String,
    pub line: usize,
    pub drawn: Generator,
    pub pool: Option<String>,
}

#[derive(Default)]
pub struct Hello {
    pub reads: Vec<String>,
    pub start: Vec<Start>,
    pub actions: Vec<Action>,
    pub sized: bool,
}

pub struct Call<'a> {
    pub line: usize,
    pub input: &'a Json,
    pub from: &'a Json,
    pub add: Option<u64>,
}

pub enum Turn<'a> {
    Start(&'a [Call<'a>]),
    Run(&'a str, &'a str, Call<'a>),
    Read,
    Done,
}

pub enum Report {
    State(State, Vec<u64>),
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
    pub output: Vec<String>,
    pub ran: Ran,
}

struct Step {
    verb: String,
    object: String,
    owner: usize,
    action: usize,
}

struct Run<'a> {
    d: &'a Diagram,
    c: &'a Check,
    steps: Vec<Step>,
    sized: bool,
    runs: u64,
    added: u64,
    first: State,
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

impl Call<'_> {
    fn json(&self) -> Json {
        json!({ "use": self.line, "input": self.input, "from": self.from, "add": self.add })
    }
}

impl Turn<'_> {
    pub fn json(&self) -> String {
        match self {
            Turn::Start(calls) => {
                json!({ "step": START, "calls": calls.iter().map(Call::json).collect::<Vec<_>>() })
            }
            Turn::Run(verb, _, call) => json!({ "step": verb, "calls": [call.json()] }),
            Turn::Read => json!({ "calls": [] }),
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
        Json::Null => String::new(),
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

    fn sized(&self, case: &Case, mut state: State) -> State {
        if self.sized {
            for o in &self.d.objects {
                for pool in o.pools.iter().filter(|pool| o.initial.contains_key(*pool)) {
                    state
                        .entry(o.name.clone())
                        .or_default()
                        .insert(pool.clone(), Value::Int(case.size(pool) as i128));
                }
            }
        }
        state
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
        self.added = 0;
        for pool in self.d.objects.iter().flat_map(|o| &o.pools) {
            case.pool(pool)?;
        }
        for m in members.iter_mut() {
            let inputs = m
                .hello()
                .start
                .iter()
                .map(|s| s.input.as_ref().map_or(Ok(Json::Null), |g| case.draw(g)))
                .collect::<Result<Vec<Json>, Halt>>();
            let inputs = match inputs {
                Err(Halt::Empty) => {
                    self.refused.insert(START.into());
                    return Err(Halt::Invalid);
                }
                inputs => inputs?,
            };
            let pools: Vec<(String, u64)> = m
                .hello()
                .start
                .iter()
                .filter_map(|s| s.pool.clone())
                .zip(self.added..)
                .collect();
            let report = match inputs.is_empty() {
                true => m.turn(&Turn::Read)?,
                false => {
                    line.input = inputs.iter().map(written).collect::<Vec<_>>().join(" ");
                    let mut adds = pools.iter().map(|(_, add)| *add);
                    let calls: Vec<Call> = m
                        .hello()
                        .start
                        .iter()
                        .zip(&inputs)
                        .map(|(s, input)| Call {
                            line: s.line,
                            input,
                            from: &Json::Null,
                            add: s.pool.as_ref().and_then(|_| adds.next()),
                        })
                        .collect();
                    m.turn(&Turn::Start(&calls))?
                }
            };
            let added = match report {
                Report::State(part, added) => {
                    real = merged(real, part);
                    added
                }
                Report::Thrown(error) => return Ok(Err(self.thrown(line, &error))),
                Report::Refused => {
                    self.refused.insert(START.into());
                    return Err(Halt::Invalid);
                }
            };
            for (pool, add) in pools.iter().filter(|(_, add)| added.contains(add)) {
                case.add(pool, Json::from(*add))?;
            }
            self.added += pools.len() as u64;
        }
        let real = self.sized(case, real);
        let judged = self.c.judge(&real, &real, &real, "");
        line.state = real.clone();
        line.evidence = judged.clone().unwrap_or_default();
        self.trace.push(line);
        self.first = real.clone();
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
            action,
        } = &self.steps[rule];
        let (verb, object, owner, action) = (verb.clone(), object.clone(), *owner, *action);
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
        let add = members[owner].hello().actions[action]
            .pool
            .as_ref()
            .map(|_| self.added);
        let mut report = Report::Refused;
        let mut empty = false;
        for _ in 0..TRIES {
            if to.is_none() || !matches!(report, Report::Refused) {
                break;
            }
            let drawn = match case.draw(&members[owner].hello().actions[action].drawn) {
                Err(Halt::Empty) => {
                    empty = true;
                    break;
                }
                drawn => drawn?,
            };
            let (from, input) = (&drawn[0], &drawn[1]);
            line.input = [written(from), written(input)]
                .into_iter()
                .filter(|part| !part.is_empty())
                .collect::<Vec<_>>()
                .join(" ");
            let call = Call {
                line: members[owner].hello().actions[action].line,
                input,
                from,
                add,
            };
            report = members[owner].turn(&Turn::Run(&verb, &object, call))?;
            if matches!(report, Report::Refused) {
                case.undo()?;
            }
        }
        let (mut next, added) = match report {
            Report::State(part, added) => (merged(real.clone(), part), added),
            Report::Refused => {
                if to.is_some() && !empty {
                    self.refused.insert(verb);
                }
                return Ok(Stepped::Refused);
            }
            Report::Thrown(error) => return Ok(Stepped::Failed(self.thrown(line, &error))),
        };
        if let (Some(pool), Some(add)) = (&members[owner].hello().actions[action].pool, add)
            && added.contains(&add)
        {
            case.add(pool, Json::from(add))?;
            self.added += 1;
        }
        for (_, m) in members.iter_mut().enumerate().filter(|(i, _)| *i != owner) {
            match m.turn(&Turn::Read)? {
                Report::State(part, _) => next = merged(next, part),
                _ => {
                    return Err(Halt::Fatal(
                        "the runner answers `read` with no state".into(),
                    ));
                }
            }
        }
        let next = self.sized(case, next);
        model.insert(object, to.unwrap_or_default());
        let applies = self.c.applies(&self.first, real, &next, &verb);
        self.tally[rule].steps += 1;
        self.tally[rule].checks += u64::from(applies);
        let judged = self.c.judge(&self.first, real, &next, &verb);
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
            Err(Halt::Invalid | Halt::Empty | Halt::Retry) => INVALID,
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
        let counted = d
            .objects
            .iter()
            .any(|x| x.name == **o && x.pools.contains(k));
        !counted && !members.iter().any(|m| m.hello().reads.contains(&tag))
    }) {
        return Err(format!(
            "the check reads `{o}.{k}`, and no schema has the annotation `read: {o}.{k}`"
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
            let found: Vec<(usize, usize)> = members
                .iter()
                .enumerate()
                .flat_map(|(owner, member)| {
                    member
                        .hello()
                        .actions
                        .iter()
                        .enumerate()
                        .filter(|(_, action)| action.verb == m.verb)
                        .map(move |(action, _)| (owner, action))
                })
                .collect();
            match found.as_slice() {
                [(owner, action)] => steps.push(Step {
                    verb: m.verb.clone(),
                    object: o.name.clone(),
                    owner: *owner,
                    action: *action,
                }),
                [] => {
                    return Err(format!(
                        "the verb `{0}` has no `run` line of the verb `{0}`",
                        m.verb
                    ));
                }
                _ => {
                    return Err(format!(
                        "the verb `{}` has {} `run` lines",
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
        sized: members.iter().any(|m| m.hello().sized),
        runs: 0,
        added: 0,
        first: State::new(),
        found: None,
        trace: Vec::new(),
        longest: Vec::new(),
        refused: BTreeSet::new(),
    };
    let mut engine = Engine::start(cases, key)?;
    while let Some(case) = engine.case()? {
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
        output: ended.output,
        ran,
    })
}
