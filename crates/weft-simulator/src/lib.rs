mod engine;
mod host;
mod hub;

use std::collections::BTreeSet;
use std::ffi::c_void;

use serde_json::json;
use weft_parse::{Check, Diagram, Ran, Span, State, shown};

use engine::{Case, Engine, Halt, INTERESTING, INVALID, OVERRUN, VALID};

pub use host::{CALLS, PRINT};
pub use hub::{Hub, Peer, Remote, Session};

const STEPS: i64 = 20;
const ORIGIN: &str = "weft";

#[derive(Default, Clone)]
pub struct Tagged {
    pub tag: String,
    pub from: Vec<String>,
    pub run: Vec<String>,
}

#[derive(Default, Clone)]
pub struct Hello {
    pub reads: Vec<Tagged>,
    pub actions: Vec<Tagged>,
    pub mocks: Vec<Tagged>,
}

#[derive(Clone)]
pub struct Mark {
    pub status: u32,
    pub origin: Option<String>,
}

pub enum Turn<'a> {
    Reset,
    Run(&'a str, &'a str),
    Read,
    Done,
    Failed(&'a [String]),
}

pub enum Report {
    State(State),
    Ran(State, String),
    Thrown(String, String),
    Refused,
    Left(Mark),
}

pub trait Member {
    fn hello(&self) -> &Hello;
    fn begin(&mut self, tc: *mut c_void);
    fn turn(&mut self, turn: &Turn) -> Result<Report, String>;
    fn end(&mut self, turn: &Turn) -> Result<Mark, String>;
    fn told(&mut self, origin: &str);
    fn finish(&mut self, result: *mut c_void) -> Result<Ran, String>;
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
            .map_or("start".into(), |(verb, object)| {
                format!("{verb} to={object}")
            })
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

enum End {
    Done,
    Failed(Vec<String>),
    Left(usize),
}

struct Run<'a> {
    d: &'a Diagram,
    c: &'a Check,
    steps: Vec<(String, String, usize)>,
    runs: u64,
    tally: Vec<Tally>,
    found: Option<Vec<Line>>,
    trace: Vec<Line>,
    longest: Vec<Line>,
    refused: BTreeSet<String>,
}

type Members<'m> = [Box<dyn Member + 'm>];

impl Turn<'_> {
    pub fn json(&self) -> String {
        match self {
            Turn::Reset => json!({ "reset": true }),
            Turn::Run(verb, object) => json!({ "run": verb, "object": object }),
            Turn::Read => json!({ "read": true }),
            Turn::Done => json!({ "done": true }),
            Turn::Failed(evidence) => json!({ "failed": evidence }),
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

fn merged(mut state: State, part: State) -> State {
    for (object, pairs) in part {
        state.entry(object).or_default().extend(pairs);
    }
    state
}

fn moves(d: &Diagram) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for o in &d.objects {
        for m in &o.moves {
            let step = (m.verb.clone(), o.name.clone());
            if !out.contains(&step) {
                out.push(step);
            }
        }
    }
    out
}

impl Run<'_> {
    fn stepped(
        &mut self,
        case: &mut Case,
        members: &mut Members,
        marks: &mut [Option<Mark>],
    ) -> Result<End, Halt> {
        let wrong = |turn: &str| Halt::Fatal(format!("the runner answers `{turn}` with no state"));
        let mut real = State::new();
        for (i, m) in members.iter_mut().enumerate() {
            match m.turn(&Turn::Reset)? {
                Report::State(part) => real = merged(real, part),
                Report::Left(mark) => {
                    marks[i] = Some(mark);
                    return Ok(End::Left(i));
                }
                _ => return Err(wrong("reset")),
            }
        }
        let judged = self.c.judge(&real, &real, "");
        self.trace = vec![Line {
            action: None,
            input: String::new(),
            state: real.clone(),
            evidence: judged.clone().unwrap_or_default(),
        }];
        if let Some(evidence) = judged {
            return Ok(End::Failed(evidence));
        }
        let mut model = self.d.init.clone();
        let rules: Vec<String> = self.tally.iter().map(|t| t.step.clone()).collect();
        case.machine(&rules, STEPS)?;
        while case.round()? {
            let mut rejected = false;
            while let Some(rule) = case.rule()? {
                let (verb, object, owner) = self.steps[rule].clone();
                let to = self
                    .d
                    .enabled(&model)
                    .into_iter()
                    .find(|(o, v, _)| o.name == object && *v == verb)
                    .map(|(_, _, to)| to);
                let report = match &to {
                    Some(_) => members[owner].turn(&Turn::Run(&verb, &object))?,
                    None => Report::Refused,
                };
                let mut line = Line {
                    action: Some((verb.clone(), object.clone())),
                    input: String::new(),
                    state: State::new(),
                    evidence: Vec::new(),
                };
                let mut next = match report {
                    Report::Ran(part, input) => {
                        line.input = input;
                        merged(real.clone(), part)
                    }
                    Report::Refused => {
                        if to.is_some() {
                            self.refused.insert(verb);
                        }
                        case.rejected()?;
                        rejected = true;
                        continue;
                    }
                    Report::Thrown(input, error) => {
                        line.input = input;
                        line.evidence = vec![format!("throws {error}")];
                        self.trace.push(line.clone());
                        case.close(false)?;
                        return Ok(End::Failed(line.evidence));
                    }
                    Report::Left(mark) => {
                        marks[owner] = Some(mark);
                        case.close(false)?;
                        return Ok(End::Left(owner));
                    }
                    Report::State(_) => return Err(wrong("run")),
                };
                for (i, m) in members.iter_mut().enumerate().filter(|(i, _)| *i != owner) {
                    match m.turn(&Turn::Read)? {
                        Report::State(part) => next = merged(next, part),
                        Report::Left(mark) => {
                            marks[i] = Some(mark);
                            case.close(false)?;
                            return Ok(End::Left(i));
                        }
                        _ => return Err(wrong("read")),
                    }
                }
                model.insert(object, to.unwrap_or_default());
                let applies = self.c.applies(&real, &next, &verb);
                self.tally[rule].steps += 1;
                self.tally[rule].checks += u64::from(applies);
                let judged = self.c.judge(&real, &next, &verb);
                line.state = next.clone();
                line.evidence = judged.clone().unwrap_or_default();
                self.trace.push(line);
                if let Some(evidence) = judged {
                    case.close(false)?;
                    return Ok(End::Failed(evidence));
                }
                real = next;
            }
            case.close(rejected)?;
        }
        case.close(false)?;
        Ok(End::Done)
    }

    fn case(&mut self, mut case: Case, members: &mut Members) -> Result<(), String> {
        self.runs += 1;
        members.iter_mut().for_each(|m| m.begin(case.raw().cast()));
        let mut marks: Vec<Option<Mark>> = vec![None; members.len()];
        let left = |marks: &[Option<Mark>], i: usize| {
            marks[i]
                .as_ref()
                .filter(|m| m.status == INTERESTING)
                .map(|m| {
                    vec![format!(
                        "the runner fails at {}",
                        m.origin.as_deref().unwrap_or(ORIGIN)
                    )]
                })
        };
        let (evidence, own) = match self.stepped(&mut case, members, &mut marks) {
            Ok(End::Done) => (None, VALID),
            Ok(End::Failed(evidence)) => (Some(evidence), INTERESTING),
            Ok(End::Left(i)) => (left(&marks, i), VALID),
            Err(Halt::Overrun) => (None, OVERRUN),
            Err(Halt::Invalid) => (None, INVALID),
            Err(Halt::Fatal(e)) => return Err(e),
        };
        let turn = evidence.as_deref().map_or(Turn::Done, Turn::Failed);
        for (m, mark) in members.iter_mut().zip(&mut marks) {
            if mark.is_none() {
                *mark = Some(m.end(&turn)?);
            }
        }
        let status = marks.iter().flatten().map(|m| m.status).fold(own, u32::max);
        let origin = marks
            .iter()
            .flatten()
            .filter(|m| m.status == INTERESTING)
            .find_map(|m| m.origin.clone())
            .unwrap_or_else(|| ORIGIN.to_string());
        match (status == INTERESTING, own == INTERESTING) {
            (true, true) => self.found = Some(self.trace.clone()),
            (false, _) if self.trace.len() > self.longest.len() => {
                self.longest = self.trace.clone()
            }
            _ => {}
        }
        if status == INTERESTING {
            members.iter_mut().for_each(|m| m.told(&origin));
        }
        case.complete(status, (status == INTERESTING).then_some(origin.as_str()))
    }
}

pub fn listed(ran: Ran, tally: &[Tally]) -> Result<Ran, String> {
    let verb = |t: &Tally| t.step.split(' ').next().unwrap_or_default().to_string();
    let spans = ran
        .actions
        .iter()
        .flat_map(|(_, spans)| spans)
        .chain(&ran.reset);
    if let Some(s) = spans.clone().find(|s| s.from == 0 || s.to < s.from) {
        return Err(format!(
            "`ran` has the lines {}-{} of `{}`, and lines go up from 1",
            s.from, s.to, s.file
        ));
    }
    if ran.reset.is_empty() {
        return Err("the runner sends no `ran` for `reset`".into());
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
        reset: ran.reset,
        actions,
    })
}

fn owners(
    d: &Diagram,
    c: &Check,
    members: &Members,
) -> Result<Vec<(String, String, usize)>, String> {
    if let Some((o, k)) = c.reads().into_iter().find(|(o, k)| {
        let tag = format!("{o}.{k}");
        !members
            .iter()
            .any(|m| m.hello().reads.iter().any(|r| r.tag == tag))
    }) {
        return Err(format!(
            "the check reads `{o}.{k}`, and no field of the state has the tag `{o}.{k}`"
        ));
    }
    moves(d)
        .into_iter()
        .map(|(verb, object)| {
            let found: Vec<usize> = members
                .iter()
                .enumerate()
                .filter(|(_, m)| m.hello().actions.iter().any(|a| a.tag == verb))
                .map(|(i, _)| i)
                .collect();
            match found.as_slice() {
                [owner] => Ok((verb, object, *owner)),
                [] => Err(format!(
                    "the verb `{verb}` has no action with the tag `{verb}`"
                )),
                _ => Err(format!(
                    "the verb `{verb}` has an action in {} runners",
                    found.len()
                )),
            }
        })
        .collect()
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
            .map(|(verb, object, _)| Tally {
                step: format!("{verb} to={object}"),
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
    let rans: Result<Vec<Ran>, String> = members
        .iter_mut()
        .map(|m| m.finish(ended.result.cast()))
        .collect();
    let (failed, caveat) = (ended.failed, ended.caveat.clone());
    engine.free(ended);
    let rans = rans?;
    if let Some(t) = run.tally.iter().find(|t| {
        t.steps == 0
            && run
                .refused
                .contains(t.step.split(' ').next().unwrap_or_default())
    }) {
        return Err(format!(
            "each input of `{}` fails the schema of its action",
            t.step.split(' ').next().unwrap_or_default()
        ));
    }
    let ran = Ran {
        reset: rans.iter().flat_map(|r| r.reset.clone()).collect(),
        actions: rans.into_iter().flat_map(|r| r.actions).collect(),
    };
    let ran = listed(ran, &run.tally)?;
    let trace = match (failed, run.found) {
        (true, Some(found)) => found,
        (true, None) => return Err("a runner fails outside a step".into()),
        (false, _) => run.longest,
    };
    Ok(Outcome {
        cases,
        runs: run.runs,
        failed,
        trace,
        tally: run.tally,
        caveat,
        ran,
    })
}
