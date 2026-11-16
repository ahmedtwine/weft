use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value as Json;
use weft_hegel::{Case, Engine, Generator, Halt, INTERESTING, INVALID, OVERRUN, VALID};
use weft_parse::{Diagram, Draw, Ran, Source, Span, State, Take, shown};

use crate::hub::Remote;
use crate::invariant::{Invariants, Moment};
use crate::protocol::{Call, Report, START, Turn};
use crate::proxy::Hop;
use crate::schema::{live, narrowed, whole};

const TRIES: usize = 4;

#[derive(Clone, Copy)]
pub enum Event<'a> {
    Hegel(&'a str),
    Joined(&'a str),
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
    pub origin: Option<String>,
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
    function: String,
    drawn: Generator,
}

struct Machine<'a> {
    d: &'a Diagram,
    steps: Vec<Step>,
    limit: i64,
    invariants: Invariants<'a>,
    runs: u64,
    first: State,
    tally: Vec<Tally>,
    found: Option<(Vec<Line>, String)>,
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

pub(crate) type Members = [Remote];

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

fn merged(mut state: State, part: State) -> State {
    for (object, pairs) in part {
        state.entry(object).or_default().extend(pairs);
    }
    state
}

fn read(members: &mut Members, but: Option<usize>) -> Result<State, Halt> {
    let mut real = State::new();
    for (_, m) in members
        .iter_mut()
        .enumerate()
        .filter(|(i, _)| Some(*i) != but)
    {
        match m.turn(&Turn::Read)? {
            Report::State(part, _) => real = merged(real, part),
            _ => {
                return Err(Halt::Fatal(
                    "the runner answers `read` with no state".into(),
                ));
            }
        }
    }
    Ok(real)
}

impl Machine<'_> {
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
        let null = Json::Null;
        for at in 0..members.len() {
            let resets: Vec<&str> = self
                .d
                .impls
                .iter()
                .filter(|i| i.adapter == members[at].adapter())
                .flat_map(|i| &i.modules)
                .filter_map(|m| m.reset.as_deref())
                .collect();
            let calls: Vec<Call> = resets
                .iter()
                .map(|function| Call {
                    function,
                    input: &null,
                })
                .collect();
            let turn = match calls.is_empty() {
                true => Turn::Read,
                false => Turn::Start(&calls),
            };
            match members[at].turn(&turn)? {
                Report::State(part, _) => real = merged(real, part),
                Report::Thrown(error) => {
                    line.hops = self.heard(case, members)?;
                    return Ok(Err(self.thrown(line, &error)));
                }
                Report::Refused(cause) => {
                    line.hops = self.heard(case, members)?;
                    (self.stream)(Event::Refused(&line, &cause));
                    self.refused.entry(START.into()).or_default().insert(cause);
                    return Err(Halt::Invalid);
                }
            };
        }
        if members.len() > 1 {
            real = merged(real, read(members, None)?);
        }
        line.hops = self.heard(case, members)?;
        if let Err(e) = self.d.conforms(&real) {
            line.state = real;
            return Ok(Err(self.thrown(line, &e)));
        }
        let started = self.d.started(&real);
        line.state = real.clone();
        line.evidence = started.clone().unwrap_or_default();
        self.trace.push(line);
        self.told();
        self.first = real.clone();
        if started.is_some() {
            return Ok(Err(START.to_string()));
        }
        let broken = self.judged(case, Moment::Initial, None, &real)?;
        Ok(broken.map_or(Ok(real), Err))
    }

    fn judged(
        &mut self,
        case: &Case,
        moment: Moment,
        rule: Option<usize>,
        real: &State,
    ) -> Result<Option<String>, Halt> {
        let judged = self
            .invariants
            .judged(case, moment, &self.first, real, self.stream)?;
        if let Some(rule) = rule {
            self.tally[rule].checks += judged.checks;
        }
        Ok(judged.broken.map(|broken| {
            if let Some(line) = self.trace.last_mut() {
                line.evidence = broken.evidence;
            }
            broken.origin
        }))
    }

    fn input(&mut self, case: &Case, rule: usize) -> Result<(Json, Option<String>), Halt> {
        let a = &self.d.actions[self.steps[rule].action];
        let mut input = case.draw(&self.steps[rule].drawn)?;
        for i in &a.inputs {
            let Source::Pool(pool, take) = &i.source else {
                continue;
            };
            let pooled = self.counts.pools.entry(pool.clone()).or_default();
            let value = match take {
                Take::Maybe => match case.draw(&Generator::Boolean)?.as_bool() {
                    Some(true) => case
                        .generate(pool, false)?
                        .inspect(|_| pooled.generate += 1),
                    _ => Some(Json::Null),
                }
                .or(Some(Json::Null)),
                Take::Reuse | Take::Consume => {
                    let consume = *take == Take::Consume;
                    let found = case.generate(pool, consume)?;
                    match (&found, consume) {
                        (None, _) => pooled.empty += 1,
                        (Some(_), true) => pooled.consume += 1,
                        (Some(_), false) => pooled.generate += 1,
                    }
                    found
                }
            };
            let Some(value) = value else {
                return Ok((input, Some(format!("pool {pool}"))));
            };
            if let Json::Object(all) = &mut input {
                all.insert(i.name.clone(), value);
            }
        }
        let refused = (!self.d.admits(a, &input)).then(|| format!("where of {}", a.verb));
        Ok((input, refused))
    }

    fn after(
        &mut self,
        case: &Case,
        rule: usize,
        input: &Json,
        result: &Json,
    ) -> Result<Option<String>, Halt> {
        let a = &self.d.actions[self.steps[rule].action];
        if let Some((name, t)) = &a.returns
            && let Err(e) = self.d.fit(t, result, &format!("{} returns {name}", a.verb))
        {
            return Ok(Some(e));
        }
        for (pool, from) in &a.adds {
            let value = match &a.returns {
                Some((name, _)) if name == from => result.clone(),
                _ => input.get(from).cloned().unwrap_or(Json::Null),
            };
            case.add(pool, value)?;
            self.counts.pools.entry(pool.clone()).or_default().add += 1;
        }
        Ok(None)
    }

    fn aimed(&self, case: &Case, real: &State) {
        for (o, k) in &self.d.targets {
            let score = match real.get(o).and_then(|p| p.get(k)) {
                Some(Json::Number(n)) => n.as_f64(),
                Some(Json::Array(all)) => Some(all.len() as f64),
                Some(Json::Object(all)) => Some(all.len() as f64),
                _ => None,
            };
            if let Some(score) = score {
                case.target(score, &format!("{o}.{k}"));
            }
        }
    }

    fn step(
        &mut self,
        case: &Case,
        members: &mut Members,
        rule: usize,
        real: &mut State,
    ) -> Result<Stepped, Halt> {
        let (verb, object, owner, function) = {
            let s = &self.steps[rule];
            (
                s.verb.clone(),
                s.object.clone(),
                s.owner,
                s.function.clone(),
            )
        };
        let mut line = Line {
            action: Some((verb.clone(), object.clone())),
            input: Vec::new(),
            state: State::new(),
            evidence: Vec::new(),
            hops: Vec::new(),
        };
        let mut done = None;
        for _ in 0..TRIES {
            let (input, refused) = self.input(case, rule)?;
            line.input = vec![input.clone()];
            let report = match refused {
                Some(cause) => Report::Refused(cause),
                None => members[owner].turn(&Turn::Run(
                    &verb,
                    Call {
                        function: &function,
                        input: &input,
                    },
                ))?,
            };
            if let Report::Refused(cause) = &report {
                line.hops = self.heard(case, members)?;
                case.event(&format!("refused {verb}: {cause}"))?;
                (self.stream)(Event::Refused(&line, cause));
                *self.tally[rule].refused.entry(cause.clone()).or_default() += 1;
                self.refused
                    .entry(verb.clone())
                    .or_default()
                    .insert(cause.clone());
                if cause.starts_with("pool ") {
                    break;
                }
                continue;
            }
            done = Some((input, report));
            break;
        }
        let Some((input, report)) = done else {
            return Ok(Stepped::Refused);
        };
        let next = match report {
            Report::State(part, result) => {
                if let Some(e) = self.after(case, rule, &input, &result)? {
                    line.hops = self.heard(case, members)?;
                    return Ok(Stepped::Failed(self.thrown(line, &e)));
                }
                merged(real.clone(), part)
            }
            Report::Refused(_) => return Ok(Stepped::Refused),
            Report::Thrown(error) => {
                line.hops = self.heard(case, members)?;
                return Ok(Stepped::Failed(self.thrown(line, &error)));
            }
        };
        let next = merged(next, read(members, Some(owner))?);
        line.hops = self.heard(case, members)?;
        if let Err(e) = self.d.conforms(&next) {
            line.state = next;
            return Ok(Stepped::Failed(self.thrown(line, &e)));
        }
        self.aimed(case, &next);
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
        let mut real = match self.started(case, members)? {
            Ok(real) => real,
            Err(origin) => return Ok(Some(origin)),
        };
        let rules: Vec<String> = self.tally.iter().map(|t| t.step.clone()).collect();
        case.machine(&rules, &self.invariants.declared(), self.limit)?;
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
            if let Some(origin) = self.judged(case, Moment::Round, last, &real)? {
                return Ok(Some(origin));
            }
        }
        case.close(false)?;
        self.judged(case, Moment::Final, None, &real)
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
            true => self.found = origin.clone().map(|o| (self.trace.clone(), o)),
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

fn owners(d: &Diagram, members: &Members) -> Result<Vec<Step>, String> {
    let draws: Vec<Draw> = d.objects.iter().flat_map(|o| o.draws.clone()).collect();
    let mut found = vec![0; draws.len()];
    let mut steps = Vec::new();
    for (at, a) in d.actions.iter().enumerate() {
        let (i, m) = d
            .module(&a.object)
            .ok_or_else(|| format!("`{}` has no module", a.object))?;
        let function = m
            .actions
            .iter()
            .find(|(v, _)| *v == a.verb)
            .map(|(_, f)| f.clone())
            .ok_or_else(|| format!("`module \"{}\"` maps no `{}`", a.object, a.verb))?;
        let owner = members
            .iter()
            .position(|r| r.adapter() == i.adapter)
            .ok_or_else(|| format!("no runner of `impl \"{}\"` joins", i.adapter))?;
        let named = format!("`action \"{}\"`", a.verb);
        let mut drawn = live(whole(&named, &d.drawn(a))?)
            .ok_or_else(|| format!("{named} has an input with no value that weft draws"))?;
        for (draw, count) in draws.iter().zip(&mut found) {
            let (narrow, n) = narrowed(drawn, draw);
            *count += n;
            drawn = narrow;
        }
        steps.push(Step {
            verb: a.verb.clone(),
            object: a.object.clone(),
            owner,
            action: at,
            function,
            drawn,
        });
    }
    if let Some((draw, _)) = draws
        .iter()
        .zip(&found)
        .find(|(draw, n)| !draw.keys && **n == 0)
    {
        return Err(format!(
            "the line `{}` of `draw` names a field of no input, or no value of the line is a value of the field",
            draw.path.join(".")
        ));
    }
    match steps.is_empty() {
        true => Err("the test declares no `action`".into()),
        false => Ok(steps),
    }
}

pub(crate) fn run<'a>(
    d: &'a Diagram,
    file: &str,
    members: &mut Members,
    key: Option<&str>,
    stream: &'a mut Stream<'a>,
) -> Result<Outcome, String> {
    let steps = owners(d, members)?;
    let declared = d.actions();
    let mut run = Machine {
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
        limit: i64::try_from(d.run.steps).map_err(|e| e.to_string())?,
        invariants: Invariants::new(d, file),
        runs: 0,
        first: State::new(),
        found: None,
        trace: Vec::new(),
        story: Vec::new(),
        refused: BTreeMap::new(),
        counts: Counts::default(),
        order: Vec::new(),
        stream,
    };
    (run.stream)(Event::Actions(&declared));
    let mut engine = Engine::start(d.run.cases, key)?;
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
    let (trace, origin) = match (ended.failed, run.found) {
        (true, Some((found, origin))) => (found, Some(origin)),
        (true, None) => return Err("Hegel fails the run with no failing case".into()),
        (false, _) => (run.story, None),
    };
    Ok(Outcome {
        cases: d.run.cases,
        runs: run.runs,
        counts: Counts {
            invariants: run.invariants.checked,
            ..run.counts
        },
        failed: ended.failed,
        trace,
        origin,
        tally: run.tally,
        caveat: ended.caveat,
        output: ended.output,
        ran,
    })
}
