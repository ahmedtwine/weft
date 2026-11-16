use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use serde_json::{Value as Json, json};
use weft_hegel::{Case, Engine, Generator, Halt, INTERESTING, INVALID, OVERRUN, VALID};
use weft_parse::{Diagram, Ran, Span, State, shown};

use crate::hub::Remote;
use crate::shape::{Field, field, item};

#[derive(Clone, Debug)]
pub struct Hop {
    pub from: String,
    pub to: String,
    pub method: String,
    pub path: String,
    pub body: String,
    pub status: u16,
    pub reply: String,
    pub sent: usize,
    pub answered: usize,
}
const TRIES: usize = 4;
const START: &str = "start";

pub(crate) struct Start {
    pub line: usize,
    pub input: Option<Generator>,
}

pub(crate) struct Action {
    pub object: String,
    pub verb: String,
    pub line: usize,
    pub drawn: Generator,
}

#[derive(Default)]
pub(crate) struct Hello {
    pub schemas: Vec<(String, Json)>,
    pub start: Vec<Start>,
    pub actions: Vec<Action>,
}

pub(crate) struct Call<'a> {
    pub line: usize,
    pub input: &'a Json,
}

pub(crate) enum Turn<'a> {
    Start(&'a [Call<'a>]),
    Run(&'a str, Call<'a>),
    Read,
    Done,
}

pub(crate) enum Report {
    State(State),
    Thrown(String),
    Refused(String),
}

pub(crate) enum Ask {
    Generate(Arc<Generator>),
    PoolAdd(String, Json),
    PoolGenerate(String, bool),
    Target(f64, String),
}

pub(crate) type Answer<'a> = dyn FnMut(Ask) -> Result<Option<Json>, Halt> + 'a;

#[derive(Clone, Copy)]
pub enum Event<'a> {
    Hegel(&'a str),
    Actions(&'a [(String, String)]),
    Run(u64),
    Step(&'a Line),
    Refused(&'a Line, &'a str),
    Check(&'a str, Option<&'a [String]>),
    End(&'static str, Option<&'a str>),
}

pub type Stream<'a> = dyn FnMut(Event) + 'a;

#[derive(Clone)]
pub struct Line {
    pub action: Option<(String, String)>,
    pub input: Vec<Json>,
    pub state: State,
    pub evidence: Vec<String>,
    pub hops: Vec<Hop>,
}

impl Line {
    pub fn step(&self) -> String {
        self.action
            .as_ref()
            .map_or(START.into(), |(verb, object)| format!("{verb} to={object}"))
    }

    pub fn input(&self) -> String {
        self.input
            .iter()
            .map(written)
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join(" ")
    }
}

pub struct Tally {
    pub step: String,
    pub runs: u64,
    pub steps: u64,
    pub refused: BTreeMap<String, u64>,
    pub then: Vec<u64>,
    pub checks: u64,
}

#[derive(Default)]
pub struct Pooled {
    pub add: u64,
    pub generate: u64,
    pub consume: u64,
    pub empty: u64,
}

pub struct Checked {
    pub name: String,
    pub checks: u64,
    pub items: Option<u64>,
}

#[derive(Default)]
pub struct Counts {
    pub statuses: BTreeMap<&'static str, u64>,
    pub calls: BTreeMap<(String, u16), u64>,
    pub pools: BTreeMap<String, Pooled>,
    pub invariants: Vec<Checked>,
}

pub struct Outcome {
    pub cases: u64,
    pub runs: u64,
    pub counts: Counts,
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
    steps: Vec<Step>,
    limit: i64,
    machine: bool,
    runs: u64,
    first: State,
    tally: Vec<Tally>,
    found: Option<Vec<Line>>,
    trace: Vec<Line>,
    story: Vec<Line>,
    refused: BTreeMap<String, BTreeSet<String>>,
    counts: Counts,
    order: Vec<usize>,
    stream: &'a mut Stream<'a>,
}

enum Stepped {
    Next,
    Refused,
    Failed(String),
}

type Members = [Remote];

impl Call<'_> {
    fn json(&self) -> Json {
        json!({ "use": self.line, "input": self.input })
    }
}

impl Turn<'_> {
    pub(crate) fn json(&self) -> String {
        match self {
            Turn::Start(calls) => {
                json!({ "step": START, "calls": calls.iter().map(Call::json).collect::<Vec<_>>() })
            }
            Turn::Run(verb, call) => json!({ "step": verb, "calls": [call.json()] }),
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

fn covered(trace: &[Line]) -> usize {
    trace
        .iter()
        .filter_map(|l| l.action.as_ref().map(|(verb, _)| verb))
        .collect::<BTreeSet<_>>()
        .len()
}

fn telling(trace: &[Line]) -> Vec<Line> {
    let all = covered(trace);
    let end = (1..=trace.len())
        .find(|n| covered(&trace[..*n]) == all)
        .unwrap_or(trace.len());
    trace[..end].to_vec()
}

fn origin(error: &str) -> String {
    error
        .rsplit_once(" at ")
        .map_or(error, |(_, place)| place)
        .to_string()
}

#[derive(Default)]
struct Asked {
    input: Vec<Json>,
    empty: Option<String>,
}

fn answered(
    case: &Case,
    pools: &mut BTreeMap<String, Pooled>,
    asked: &mut Asked,
    ask: Ask,
) -> Result<Option<Json>, Halt> {
    match ask {
        Ask::Generate(generator) => {
            let value = case.draw(&generator)?;
            asked.input.push(value.clone());
            Ok(Some(value))
        }
        Ask::PoolGenerate(pool, consume) => {
            let value = case.generate(&pool, consume)?;
            let pooled = pools.entry(pool.clone()).or_default();
            match (&value, consume) {
                (None, _) => {
                    pooled.empty += 1;
                    asked.empty = Some(format!("pool {pool}"));
                }
                (Some(_), true) => pooled.consume += 1,
                (Some(_), false) => pooled.generate += 1,
            }
            asked
                .input
                .extend(value.iter().map(|value| json!({ pool.as_str(): value })));
            Ok(value)
        }
        Ask::PoolAdd(pool, value) => {
            case.add(&pool, value)?;
            pools.entry(pool).or_default().add += 1;
            Ok(Some(Json::Null))
        }
        Ask::Target(value, label) => {
            case.target(value, &label);
            Ok(Some(Json::Null))
        }
    }
}

fn merged(mut state: State, part: State) -> State {
    for (object, pairs) in part {
        state.entry(object).or_default().extend(pairs);
    }
    state
}

impl Run<'_> {
    fn heard(&mut self, case: &Case, members: &mut Members) -> Result<Vec<Hop>, Halt> {
        let hops: Vec<Hop> = members.iter_mut().flat_map(Remote::hops).collect();
        for hop in &hops {
            let route = hop.path.split(['/', '?']).nth(1).unwrap_or_default();
            let call = format!("{} → {} {} /{route}", hop.from, hop.to, hop.method);
            case.event(&format!("call {call} {}", hop.status))?;
            *self.counts.calls.entry((call, hop.status)).or_default() += 1;
        }
        Ok(hops)
    }

    fn told(&mut self) {
        if let Some(line) = self.trace.last() {
            (self.stream)(Event::Step(line));
        }
    }

    fn read(
        &mut self,
        case: &Case,
        members: &mut Members,
        but: Option<usize>,
    ) -> Result<State, Halt> {
        let mut real = State::new();
        for (_, m) in members
            .iter_mut()
            .enumerate()
            .filter(|(i, _)| Some(*i) != but)
        {
            match m.turn(&Turn::Read, &mut |ask| {
                answered(case, &mut self.counts.pools, &mut Asked::default(), ask)
            })? {
                Report::State(part) => real = merged(real, part),
                _ => {
                    return Err(Halt::Fatal(
                        "the runner answers `read` with no state".into(),
                    ));
                }
            }
        }
        Ok(real)
    }

    fn thrown(&mut self, mut line: Line, error: &str) -> String {
        line.evidence = vec![format!("throws {error}")];
        self.trace.push(line);
        self.told();
        origin(error)
    }

    fn started(
        &mut self,
        case: &Case,
        members: &mut Members,
    ) -> Result<Result<State, String>, Halt> {
        let mut real = State::new();
        let mut line = Line {
            action: None,
            input: Vec::new(),
            state: State::new(),
            evidence: Vec::new(),
            hops: Vec::new(),
        };
        self.trace.clear();
        self.order.clear();
        for at in 0..members.len() {
            let m = &mut members[at];
            let inputs = m
                .hello()
                .start
                .iter()
                .map(|s| s.input.as_ref().map_or(Ok(Json::Null), |g| case.draw(g)))
                .collect::<Result<Vec<Json>, Halt>>()?;
            let mut asked = Asked {
                input: inputs.clone(),
                empty: None,
            };
            let calls: Vec<Call> = m
                .hello()
                .start
                .iter()
                .zip(&inputs)
                .map(|(s, input)| Call {
                    line: s.line,
                    input,
                })
                .collect();
            let turn = match calls.is_empty() {
                true => Turn::Read,
                false => Turn::Start(&calls),
            };
            let report = m.turn(&turn, &mut |ask| {
                answered(case, &mut self.counts.pools, &mut asked, ask)
            })?;
            line.input = asked.input;
            match report {
                Report::State(part) => real = merged(real, part),
                Report::Thrown(error) => {
                    line.hops = self.heard(case, members)?;
                    return Ok(Err(self.thrown(line, &error)));
                }
                Report::Refused(cause) => {
                    line.hops = self.heard(case, members)?;
                    let cause = asked.empty.unwrap_or(cause);
                    (self.stream)(Event::Refused(&line, &cause));
                    self.refused.entry(START.into()).or_default().insert(cause);
                    return Err(Halt::Invalid);
                }
            };
        }
        if members.len() > 1 {
            real = merged(real, self.read(case, members, None)?);
        }
        line.hops = self.heard(case, members)?;
        let started = self.d.started(&real);
        line.state = real.clone();
        line.evidence = started.clone().unwrap_or_default();
        self.trace.push(line);
        self.told();
        self.first = real.clone();
        let judged = match started {
            Some(_) => Some(START.to_string()),
            None => self.judged(case, None, &real)?,
        };
        Ok(judged.map_or(Ok(real), Err))
    }

    fn judged(
        &mut self,
        case: &Case,
        rule: Option<usize>,
        real: &State,
    ) -> Result<Option<String>, Halt> {
        let d = self.d;
        for (index, invariant) in d.invariants().into_iter().enumerate() {
            if self.machine && !case.invariant(index)? {
                continue;
            }
            if let Some(rule) = rule {
                self.tally[rule].checks += 1;
            }
            let checked = &mut self.counts.invariants[index];
            checked.checks += 1;
            checked.items = invariant
                .items(real)
                .map(|items| checked.items.unwrap_or(0) + items as u64);
            let evidence = invariant.judge(&self.first, real);
            (self.stream)(Event::Check(&invariant.name, evidence.as_deref()));
            if let Some(evidence) = evidence {
                if let Some(line) = self.trace.last_mut() {
                    line.evidence = evidence;
                }
                return Ok(Some(invariant.name.clone()));
            }
        }
        Ok(None)
    }

    fn step(
        &mut self,
        case: &Case,
        members: &mut Members,
        rule: usize,
        real: &mut State,
    ) -> Result<Stepped, Halt> {
        let Step {
            verb,
            object,
            owner,
            action,
        } = &self.steps[rule];
        let (verb, object, owner, action) = (verb.clone(), object.clone(), *owner, *action);
        let mut line = Line {
            action: Some((verb.clone(), object.clone())),
            input: Vec::new(),
            state: State::new(),
            evidence: Vec::new(),
            hops: Vec::new(),
        };
        let mut report = Report::Refused(String::new());
        for _ in 0..TRIES {
            if !matches!(report, Report::Refused(_)) {
                break;
            }
            let drawn = case.draw(&members[owner].hello().actions[action].drawn)?;
            let mut asked = Asked {
                input: vec![drawn.clone()],
                empty: None,
            };
            let call = Call {
                line: members[owner].hello().actions[action].line,
                input: &drawn,
            };
            report = members[owner].turn(&Turn::Run(&verb, call), &mut |ask| {
                answered(case, &mut self.counts.pools, &mut asked, ask)
            })?;
            line.input = asked.input;
            if let Report::Refused(cause) = &report {
                let cause = asked.empty.unwrap_or_else(|| cause.clone());
                line.hops = self.heard(case, members)?;
                case.event(&format!("refused {verb}: {cause}"))?;
                (self.stream)(Event::Refused(&line, &cause));
                *self.tally[rule].refused.entry(cause.clone()).or_default() += 1;
                self.refused.entry(verb.clone()).or_default().insert(cause);
            }
        }
        let next = match report {
            Report::State(part) => merged(real.clone(), part),
            Report::Refused(_) => return Ok(Stepped::Refused),
            Report::Thrown(error) => {
                line.hops = self.heard(case, members)?;
                return Ok(Stepped::Failed(self.thrown(line, &error)));
            }
        };
        let next = merged(next, self.read(case, members, Some(owner))?);
        line.hops = self.heard(case, members)?;
        case.event(&format!("step {verb}"))?;
        self.order.push(rule);
        self.tally[rule].steps += 1;
        self.tally[rule].checks += 1;
        line.state = next.clone();
        self.trace.push(line);
        self.told();
        *real = next;
        Ok(Stepped::Next)
    }

    fn stepped(&mut self, case: &mut Case, members: &mut Members) -> Result<Option<String>, Halt> {
        self.machine = false;
        let mut real = match self.started(case, members)? {
            Ok(real) => real,
            Err(origin) => return Ok(Some(origin)),
        };
        let rules: Vec<String> = self.tally.iter().map(|t| t.step.clone()).collect();
        let invariants: Vec<String> = self
            .d
            .invariants()
            .into_iter()
            .map(|i| i.name.clone())
            .collect();
        case.machine(&rules, &invariants, self.limit)?;
        self.machine = true;
        while case.round()? {
            let mut rejected = false;
            let mut last = None;
            while let Some(rule) = case.rule()? {
                match self.step(case, members, rule, &mut real)? {
                    Stepped::Next => last = Some(rule),
                    Stepped::Refused => {
                        case.rejected()?;
                        rejected = true;
                    }
                    Stepped::Failed(origin) => {
                        case.close(false)?;
                        return Ok(Some(origin));
                    }
                }
            }
            case.close(rejected)?;
            if let Some(origin) = self.judged(case, last, &real)? {
                return Ok(Some(origin));
            }
        }
        case.close(false)?;
        Ok(None)
    }

    fn case(&mut self, mut case: Case, members: &mut Members) -> Result<(), String> {
        self.runs += 1;
        (self.stream)(Event::Run(self.runs));
        let mut origin = None;
        let (status, named) = match self.stepped(&mut case, members) {
            Ok(None) => (VALID, "valid"),
            Ok(found) => {
                origin = found;
                (INTERESTING, "interesting")
            }
            Err(Halt::Overrun) => (OVERRUN, "overrun"),
            Err(Halt::Invalid | Halt::Retry) => (INVALID, "invalid"),
            Err(Halt::Fatal(e)) => return Err(e),
        };
        *self.counts.statuses.entry(named).or_default() += 1;
        (self.stream)(Event::End(named, origin.as_deref()));
        if matches!(status, VALID | INTERESTING) {
            let _ = case.event_value("steps", self.order.len() as f64);
        }
        for (at, rule) in self.order.iter().enumerate() {
            if !self.order[..at].contains(rule) {
                self.tally[*rule].runs += 1;
                let after: BTreeSet<usize> = self.order[at + 1..].iter().copied().collect();
                for then in after {
                    self.tally[*rule].then[then] += 1;
                }
            }
        }
        match status == INTERESTING {
            true => self.found = Some(self.trace.clone()),
            false => {
                let found = telling(&self.trace);
                if covered(&found) > covered(&self.story)
                    || (covered(&found) == covered(&self.story) && found.len() < self.story.len())
                    || self.story.is_empty()
                {
                    self.story = found;
                }
            }
        }
        case.complete(status, origin.as_deref())
    }
}

pub(crate) fn listed(ran: Ran, tally: &[Tally]) -> Result<Ran, String> {
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
        return Err(format!(
            "`ran` names `{v}`, and no module exports the action `{v}`"
        ));
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

fn registered<'a>(members: &'a Members, o: &str, k: &str) -> Result<Option<&'a Json>, String> {
    let tag = format!("{o}.{k}");
    let all = || members.iter().flat_map(|m| &m.hello().schemas);
    if let Some((_, s)) = all().find(|(key, _)| *key == tag) {
        return Ok(Some(s));
    }
    match all().find(|(key, _)| key == o) {
        Some((_, s)) => match field(s, k) {
            Field::Found(s) => Ok(Some(s)),
            Field::Open => Ok(None),
            Field::Missing(fields) => Err(format!(
                "the diagram names `{tag}`, and `weft.state(\"{o}\", …)` has the fields {}",
                fields.join(", ")
            )),
        },
        None => Err(format!(
            "the diagram names `{tag}`, and no runner calls `weft.state(\"{tag}\", …)`"
        )),
    }
}

fn shaped(d: &Diagram, members: &Members) -> Result<(), String> {
    for (o, k) in d.keys() {
        registered(members, o, k)?;
    }
    for (o, k, path) in d.fields() {
        let Some(mut at) = registered(members, o, k)?.and_then(item) else {
            continue;
        };
        for (n, part) in path.iter().enumerate() {
            match field(at, part) {
                Field::Found(s) => at = s,
                Field::Open => break,
                Field::Missing(fields) => {
                    return Err(format!(
                        "the invariant reads `{}` of each item of `{o}.{k}`, and an item has the fields {}",
                        path[..=n].join("."),
                        fields.join(", ")
                    ));
                }
            }
        }
    }
    Ok(())
}

fn owners(d: &Diagram, members: &Members) -> Result<Vec<Step>, String> {
    shaped(d, members)?;
    if members.iter().all(|m| m.hello().start.is_empty()) {
        return Err("no module exports a `reset`".into());
    }
    let steps: Vec<Step> = members
        .iter()
        .enumerate()
        .flat_map(|(owner, member)| {
            member
                .hello()
                .actions
                .iter()
                .enumerate()
                .map(move |(action, a)| Step {
                    verb: a.verb.clone(),
                    object: a.object.clone(),
                    owner,
                    action,
                })
        })
        .collect();
    if let Some(twice) = steps
        .iter()
        .enumerate()
        .find(|(at, s)| steps[..*at].iter().any(|t| t.verb == s.verb))
        .map(|(_, s)| &s.verb)
    {
        return Err(format!("two modules export the action `{twice}`"));
    }
    match steps.is_empty() {
        true => Err("no module exports an action".into()),
        false => Ok(steps),
    }
}

pub(crate) fn simulate<'a>(
    d: &'a Diagram,
    members: &mut Members,
    cases: u64,
    limit: u64,
    key: Option<&str>,
    stream: &'a mut Stream<'a>,
) -> Result<Outcome, String> {
    let steps = owners(d, members)?;
    let mut run = Run {
        d,
        tally: steps
            .iter()
            .map(|s| Tally {
                step: format!("{} to={}", s.verb, s.object),
                runs: 0,
                steps: 0,
                refused: BTreeMap::new(),
                then: vec![0; steps.len()],
                checks: 0,
            })
            .collect(),
        steps,
        limit: i64::try_from(limit).map_err(|e| e.to_string())?,
        machine: false,
        runs: 0,
        first: State::new(),
        found: None,
        trace: Vec::new(),
        story: Vec::new(),
        refused: BTreeMap::new(),
        counts: Counts {
            invariants: d
                .invariants()
                .into_iter()
                .map(|i| Checked {
                    name: i.name.clone(),
                    checks: 0,
                    items: None,
                })
                .collect(),
            ..Counts::default()
        },
        order: Vec::new(),
        stream,
    };
    let actions: Vec<(String, String)> = run
        .steps
        .iter()
        .map(|s| (s.verb.clone(), s.object.clone()))
        .collect();
    (run.stream)(Event::Actions(&actions));
    let mut engine = Engine::start(cases, key)?;
    loop {
        let case = engine.case()?;
        for line in engine.heard() {
            (run.stream)(Event::Hegel(&line));
        }
        let Some(case) = case else {
            break;
        };
        run.case(case, members)?;
    }
    let ended = engine.end()?;
    for line in engine.heard() {
        (run.stream)(Event::Hegel(&line));
    }
    let rans = members
        .iter_mut()
        .map(Remote::finish)
        .collect::<Result<Vec<Ran>, String>>()?;
    let verb = |t: &Tally| t.step.split(' ').next().unwrap_or_default().to_string();
    if let Some((refused, causes)) = [START.to_string()]
        .into_iter()
        .filter(|_| run.story.is_empty())
        .chain(run.tally.iter().filter(|t| t.steps == 0).map(verb))
        .filter(|_| !ended.failed)
        .find_map(|v| run.refused.get(&v).map(|causes| (v, causes)))
    {
        return Err(format!(
            "each try of `{refused}` is refused by {}",
            causes.iter().cloned().collect::<Vec<_>>().join(", ")
        ));
    }
    if let Some(unrun) = run
        .tally
        .iter()
        .filter(|t| t.steps == 0 && !ended.failed)
        .map(verb)
        .next()
    {
        return Err(format!(
            "each action runs in one case or more, and `{unrun}` runs in none"
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
        (false, _) => run.story,
    };
    Ok(Outcome {
        cases,
        runs: run.runs,
        counts: run.counts,
        failed: ended.failed,
        trace,
        tally: run.tally,
        caveat: ended.caveat,
        output: ended.output,
        ran,
    })
}
