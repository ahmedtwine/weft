mod engine;
mod memo;

use std::cell::RefCell;
use std::collections::BTreeMap;

use serde_json::{Map, Value as Json};
use weft_parse::{Check, Diagram, Ran, Span, State, Value, shown};

use memo::{Memo, Next};

const REPLAYS: usize = 10;

pub enum Fault {
    Thrown(String),
    Broken(String),
    Changed,
}

pub struct Answer {
    pub state: Json,
    pub refused: Option<String>,
}

impl From<Json> for Answer {
    fn from(state: Json) -> Answer {
        Answer {
            state,
            refused: None,
        }
    }
}

#[derive(Clone, PartialEq)]
pub enum Choice {
    Any,
    Bool,
    Int(i64, i64),
    Text(usize, Option<usize>),
    Pattern(String),
    Format(Format),
    Number(Bound, Bound),
    Enum(Vec<Json>),
    AnyOf(Vec<Choice>),
    Array(Box<Choice>, usize, Option<usize>, bool),
    Tuple(Vec<Choice>),
    Record(Box<Choice>, Box<Choice>),
    Object(Vec<(String, Choice, bool)>),
    Rec(String, Box<Choice>, Box<Choice>),
    Var(String),
}

#[derive(Clone, Copy, PartialEq)]
pub enum Bound {
    Open,
    At(f64),
    Past(f64),
}

#[derive(Clone, Copy, PartialEq)]
pub enum Format {
    Email,
    Uri,
    Uuid,
    Date,
    Time,
    DateTime,
    Ipv4,
    Ipv6,
    Hostname,
}

impl Format {
    const ALL: [(&'static str, Format); 9] = [
        ("email", Format::Email),
        ("uri", Format::Uri),
        ("uuid", Format::Uuid),
        ("date", Format::Date),
        ("time", Format::Time),
        ("date-time", Format::DateTime),
        ("ipv4", Format::Ipv4),
        ("ipv6", Format::Ipv6),
        ("hostname", Format::Hostname),
    ];

    fn parse(name: &str) -> Option<Format> {
        Format::ALL
            .iter()
            .find(|(n, _)| *n == name)
            .map(|(_, f)| *f)
    }
}

pub trait Source {
    fn draw(&self, choice: &Choice) -> Json;
    fn index(&self, count: usize) -> usize;
    fn target(&self, score: f64);
}

impl Choice {
    pub fn ends(&self) -> bool {
        match self {
            Choice::Var(_) => false,
            Choice::Rec(_, _, shown) => shown.ends(),
            Choice::AnyOf(all) => all.iter().any(Choice::ends),
            Choice::Array(item, min, ..) => *min == 0 || item.ends(),
            Choice::Tuple(all) => all.iter().all(Choice::ends),
            Choice::Object(fields) => fields.iter().all(|(_, c, required)| !required || c.ends()),
            _ => true,
        }
    }

    fn merged(self, other: Choice, name: &str) -> Result<Choice, String> {
        match (self, other) {
            (Choice::Rec(key, body, shown), other) | (other, Choice::Rec(key, body, shown)) => {
                Ok(Choice::Rec(key, body, Box::new(shown.merged(other, name)?)))
            }
            (Choice::AnyOf(all), other) | (other, Choice::AnyOf(all)) => all
                .into_iter()
                .map(|c| c.merged(other.clone(), name))
                .collect::<Result<_, _>>()
                .map(Choice::AnyOf),
            (Choice::Object(mut fields), Choice::Object(more)) => {
                for (key, choice, required) in more {
                    match fields.iter_mut().find(|(k, ..)| *k == key) {
                        Some((_, known, needed)) if *known == choice => *needed |= required,
                        Some(_) => {
                            return Err(format!(
                                "the field `{name}.{key}` has two schemas in one `allOf`"
                            ));
                        }
                        None => fields.push((key, choice, required)),
                    }
                }
                Ok(Choice::Object(fields))
            }
            _ => Err(format!(
                "the field `{name}` has an `allOf` that is not of objects"
            )),
        }
    }
}

enum Stop {
    Fail,
    End,
}

trait Case {
    fn start(&mut self, source: &dyn Source) -> Result<(), Stop>;
    fn step(&mut self, source: &dyn Source) -> Result<(), Stop>;
}

pub trait Runner {
    fn schema(&self) -> &Json;
    fn reset(&mut self, source: &dyn Source) -> Result<Answer, Fault>;
    fn apply(
        &mut self,
        object: &str,
        verb: &str,
        input: &Json,
        source: &dyn Source,
    ) -> Result<Answer, Fault>;
    fn check(
        &mut self,
        check: &Check,
        cur: &State,
        nxt: &State,
        verb: &str,
    ) -> Result<Option<Vec<String>>, Fault> {
        Ok(check.judge(cur, nxt, verb))
    }
    fn ran(&mut self) -> Result<Ran, String>;
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

struct Read {
    path: Vec<String>,
    object: String,
    key: String,
    like: Value,
}

struct Tags {
    reads: Vec<Read>,
    actions: Vec<(String, Choice)>,
}

pub fn tags(node: &Json) -> Vec<&str> {
    match node.get("weft") {
        Some(Json::String(t)) => vec![t.as_str()],
        Some(Json::Array(all)) => all.iter().filter_map(Json::as_str).collect(),
        _ => Vec::new(),
    }
}

fn tagged<'a>(node: &'a Json, path: &[String], out: &mut Vec<(Vec<String>, &'a str)>) {
    for t in tags(node) {
        out.push((path.to_vec(), t));
    }
    if let Some(Json::Object(all)) = node.get("properties") {
        for (k, v) in all {
            tagged(v, &[path, std::slice::from_ref(k)].concat(), out);
        }
    }
}

const SHAPED: [&str; 7] = ["type", "enum", "const", "anyOf", "oneOf", "allOf", "not"];

pub fn choice(name: &str, node: &Json) -> Result<Choice, String> {
    Drawn {
        defs: node.get("$defs"),
        open: Vec::new(),
    }
    .choice(name, node)
}

struct Drawn<'a> {
    defs: Option<&'a Json>,
    open: Vec<(String, bool)>,
}

impl Drawn<'_> {
    fn named(&mut self, name: &str, key: &str, wrong: &str) -> Result<Choice, String> {
        if let Some((_, used)) = self.open.iter_mut().find(|(o, _)| o == key) {
            *used = true;
            return Ok(Choice::Var(key.to_string()));
        }
        let def = self
            .defs
            .and_then(|all| all.get(key))
            .ok_or_else(|| wrong.to_string())?;
        self.open.push((key.to_string(), false));
        let body = self.choice(name, def);
        let used = self.open.pop().is_some_and(|(_, used)| used);
        match (body?, used) {
            (body, false) => Ok(body),
            (body, true) if body.ends() => Ok(Choice::Rec(
                key.to_string(),
                Box::new(body.clone()),
                Box::new(body),
            )),
            _ => Err(format!(
                "the field `{name}` has the schema `{key}` in each value of `{key}`, and weft draws no value without an end"
            )),
        }
    }

    fn each(&mut self, name: &str, all: &[Json]) -> Result<Vec<Choice>, String> {
        all.iter().map(|b| self.choice(name, b)).collect()
    }

    fn choice(&mut self, name: &str, node: &Json) -> Result<Choice, String> {
        let int = |k: &str| node.get(k).and_then(Json::as_i64);
        let number = |k: &str| node.get(k).and_then(Json::as_f64);
        let size = |k: &str| int(k).and_then(|n| usize::try_from(n).ok());
        let wrong = format!("the field `{name}` is not a JSON Schema that weft draws: `{node}`");
        let listed = |k: &str| {
            node.get(k)
                .and_then(Json::as_array)
                .filter(|all| !all.is_empty())
        };
        let bound = |at: &str, past: &str| match (number(past), number(at)) {
            (Some(n), _) => Bound::Past(n),
            (_, Some(n)) => Bound::At(n),
            _ => Bound::Open,
        };
        if let Some(path) = node.get("$ref").and_then(Json::as_str) {
            let key = path.strip_prefix("#/$defs/").ok_or(&wrong)?;
            return self.named(name, key, &wrong);
        }
        if node.is_object() && !SHAPED.iter().any(|k| node.get(k).is_some()) {
            return Ok(Choice::Any);
        }
        if let Some(all) = listed("allOf") {
            let mut rest = node.clone();
            rest.as_object_mut().ok_or(&wrong)?.remove("allOf");
            let own = match rest.get("type").is_some() || rest.get("properties").is_some() {
                true => vec![self.choice(name, &rest)?],
                false => Vec::new(),
            };
            return [own, self.each(name, all)?]
                .concat()
                .into_iter()
                .map(Ok)
                .reduce(|a, b| a?.merged(b?, name))
                .ok_or(wrong)?;
        }
        if let Some(types) = node.get("type").and_then(Json::as_array) {
            return types
                .iter()
                .map(|t| {
                    let mut one = node.clone();
                    one["type"] = t.clone();
                    self.choice(name, &one)
                })
                .collect::<Result<_, _>>()
                .map(Choice::AnyOf);
        }
        match (
            listed("enum"),
            node.get("const"),
            listed("anyOf").or_else(|| listed("oneOf")),
            node.get("type").and_then(Json::as_str),
        ) {
            (Some(values), ..) => Ok(Choice::Enum(values.clone())),
            (_, Some(value), ..) => Ok(Choice::Enum(vec![value.clone()])),
            (_, _, Some(all), _) => self.each(name, all).map(Choice::AnyOf),
            (.., Some("integer")) => Ok(Choice::Int(
                int("exclusiveMinimum")
                    .map(|n| n.saturating_add(1))
                    .or(int("minimum"))
                    .unwrap_or(i64::MIN),
                int("exclusiveMaximum")
                    .map(|n| n.saturating_sub(1))
                    .or(int("maximum"))
                    .unwrap_or(i64::MAX),
            )),
            (.., Some("string")) => Ok(
                match (
                    node.get("pattern").and_then(Json::as_str),
                    node.get("format").and_then(Json::as_str),
                ) {
                    (Some(pattern), _) => Choice::Pattern(pattern.to_string()),
                    (None, Some(format)) => Choice::Format(Format::parse(format).ok_or(&wrong)?),
                    (None, None) => Choice::Text(size("minLength").unwrap_or(0), size("maxLength")),
                },
            ),
            (.., Some("number")) => Ok(Choice::Number(
                bound("minimum", "exclusiveMinimum"),
                bound("maximum", "exclusiveMaximum"),
            )),
            (.., Some("boolean")) => Ok(Choice::Bool),
            (.., Some("null")) => Ok(Choice::Enum(vec![Json::Null])),
            (.., Some("array")) => match (listed("prefixItems"), node.get("items")) {
                (Some(all), _) => all
                    .iter()
                    .enumerate()
                    .map(|(i, item)| self.choice(&format!("{name}[{i}]"), item))
                    .collect::<Result<_, _>>()
                    .map(Choice::Tuple),
                (None, Some(item)) => Ok(Choice::Array(
                    Box::new(self.choice(&format!("{name}[]"), item)?),
                    size("minItems").unwrap_or(0),
                    size("maxItems"),
                    node.get("uniqueItems").and_then(Json::as_bool) == Some(true),
                )),
                (None, None) => Err(wrong),
            },
            (.., Some("object"))
                if node.get("properties").is_none()
                    && node
                        .get("additionalProperties")
                        .is_some_and(Json::is_object) =>
            {
                let key = match node.get("propertyNames") {
                    Some(names) => self.choice(&format!("{name}{{}}"), names)?,
                    None => Choice::Text(0, None),
                };
                Ok(Choice::Record(
                    Box::new(key),
                    Box::new(self.choice(&format!("{name}{{}}"), &node["additionalProperties"])?),
                ))
            }
            (.., Some("object")) => {
                let required: Vec<&str> = node
                    .get("required")
                    .and_then(Json::as_array)
                    .map_or(Vec::new(), |all| {
                        all.iter().filter_map(Json::as_str).collect()
                    });
                match node.get("properties") {
                    Some(Json::Object(all)) => all
                        .iter()
                        .map(|(k, v)| {
                            self.choice(&format!("{name}.{k}"), v)
                                .map(|c| (k.clone(), c, required.contains(&k.as_str())))
                        })
                        .collect::<Result<_, _>>()
                        .map(Choice::Object),
                    _ => Ok(Choice::Object(Vec::new())),
                }
            }
            _ => Err(wrong),
        }
    }
}

impl Tags {
    fn new(d: &Diagram, schema: &Json) -> Result<Tags, String> {
        let state = schema
            .get("state")
            .ok_or("the runner sends no schema of the state")?;
        let mut found = Vec::new();
        tagged(state, &[], &mut found);
        let mut reads: Vec<Read> = Vec::new();
        for (path, tag) in found {
            let (o, k) = tag
                .split_once('.')
                .ok_or_else(|| format!("the tag `{tag}` of the state is not `<object>.<key>`"))?;
            let Some(x) = d.objects.iter().find(|x| x.name == o) else {
                continue;
            };
            let like = x
                .initial
                .get(k)
                .ok_or_else(|| format!("the tag `{tag}` names no key of `{o}`"))?
                .clone();
            if reads.iter().any(|r| r.object == o && r.key == k) {
                return Err(format!("the tag `{tag}` is on two fields of the state"));
            }
            reads.push(Read {
                path,
                object: o.to_string(),
                key: k.to_string(),
                like,
            });
        }
        let c = d.check.as_ref().ok_or("needs `check`")?;
        if let Some((o, k)) = c
            .reads()
            .into_iter()
            .find(|(o, k)| !reads.iter().any(|r| &r.object == *o && &r.key == *k))
        {
            return Err(format!(
                "the check reads `{o}.{k}`, and no field of the state has the tag `{o}.{k}`"
            ));
        }
        let verbs: Vec<&String> = d
            .objects
            .iter()
            .flat_map(|o| &o.moves)
            .map(|m| &m.verb)
            .collect();
        let mut actions: Vec<(String, Choice)> = Vec::new();
        for node in schema
            .get("actions")
            .and_then(Json::as_array)
            .ok_or("the runner sends no schemas of the actions")?
        {
            let verb = match tags(node)[..] {
                [verb] => verb,
                _ => return Err(format!("the action `{node}` has not one tag")),
            };
            if !verbs.iter().any(|v| *v == verb) {
                continue;
            }
            if actions.iter().any(|(v, _)| v == verb) {
                return Err(format!("the tag `{verb}` is on two actions"));
            }
            actions.push((verb.to_string(), choice(verb, node)?));
        }
        if let Some(v) = verbs
            .iter()
            .find(|v| !actions.iter().any(|(a, _)| a == **v))
        {
            return Err(format!("the verb `{v}` has no action with the tag `{v}`"));
        }
        Ok(Tags { reads, actions })
    }

    fn read(&self, raw: &Json) -> Result<State, String> {
        let mut out = State::new();
        for r in &self.reads {
            let tag = format!("{}.{}", r.object, r.key);
            let at = r
                .path
                .iter()
                .try_fold(raw, |j, p| j.get(p))
                .ok_or_else(|| format!("the state has no field with the tag `{tag}`"))?;
            let v = match (&r.like, at.as_str(), at.as_i64()) {
                (Value::Text(_), Some(s), _) => Value::Text(s.to_string()),
                (Value::Int(_), _, Some(n)) => Value::Int(n.into()),
                _ => {
                    return Err(format!(
                        "`{tag}` is `{at}`, and `{}` is of the type of `{}`",
                        r.key, r.like
                    ));
                }
            };
            out.entry(r.object.clone())
                .or_default()
                .insert(r.key.clone(), v);
        }
        Ok(out)
    }

    fn input(&self, source: &dyn Source, verb: &str) -> Json {
        self.actions
            .iter()
            .find(|(v, _)| v == verb)
            .map_or(Json::Object(Map::new()), |(_, c)| source.draw(c))
    }
}

fn moves(d: &Diagram) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for o in &d.objects {
        for m in &o.moves {
            let step = format!("{} to={}", m.verb, o.name);
            if !out.contains(&step) {
                out.push(step);
            }
        }
    }
    out
}

pub fn view(s: &State) -> String {
    s.iter()
        .map(|(o, p)| format!("{o}: {}", shown(p)))
        .collect::<Vec<_>>()
        .join("; ")
}

struct Seen<'s> {
    source: &'s dyn Source,
    drawn: RefCell<Vec<String>>,
}

impl Seen<'_> {
    fn written(&self, input: &Json) -> String {
        let drawn = self.drawn.borrow();
        [
            written(input),
            match drawn.is_empty() {
                true => String::new(),
                false => format!("drawn={}", drawn.join(",")),
            },
        ]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
    }
}

impl Source for Seen<'_> {
    fn draw(&self, choice: &Choice) -> Json {
        let value = self.source.draw(choice);
        self.drawn.borrow_mut().push(value.to_string());
        value
    }

    fn index(&self, count: usize) -> usize {
        self.source.index(count)
    }

    fn target(&self, score: f64) {
        self.source.target(score);
    }
}

fn written(input: &Json) -> String {
    input.as_object().map_or(String::new(), |all| {
        all.iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect::<Vec<_>>()
            .join(" ")
    })
}

struct Run<'a> {
    d: &'a Diagram,
    c: &'a Check,
    runner: Memo<'a>,
    runs: u64,
    tally: Vec<Tally>,
    found: Option<Vec<Line>>,
    fatal: Option<String>,
    model: State,
    real: State,
    trace: Vec<Line>,
    longest: Vec<Line>,
    refused: BTreeMap<String, String>,
}

impl Run<'_> {
    fn fail(&mut self, trace: Vec<Line>) -> Stop {
        self.runner.keep();
        self.found = Some(trace);
        Stop::Fail
    }

    fn passed(&mut self) {
        if self.trace.len() > self.longest.len() {
            self.longest = self.trace.clone();
        }
    }

    fn wire(&mut self, e: String) -> Stop {
        self.fatal = Some(e);
        Stop::Fail
    }

    fn halt(&mut self, fault: Fault) -> Stop {
        match fault {
            Fault::Thrown(e) | Fault::Broken(e) => self.wire(e),
            Fault::Changed => Stop::End,
        }
    }

    fn judge(&mut self, cur: &State, nxt: &State, verb: &str) -> Result<Option<Vec<String>>, Stop> {
        match self.runner.check(self.c, cur, nxt, verb) {
            Ok(evidence) => Ok(evidence),
            Err(Fault::Thrown(e)) => Ok(Some(vec![format!("throws {e}")])),
            Err(fault) => Err(self.halt(fault)),
        }
    }

    fn caveat(&mut self) -> Option<String> {
        if self.fatal.is_some() || self.found.is_none() {
            return None;
        }
        match self.runner.repeats(REPLAYS) {
            Ok(REPLAYS) => None,
            Ok(seen) => Some(format!("fails in {seen} of {REPLAYS} replays")),
            Err(fault) => {
                self.halt(fault);
                None
            }
        }
    }
}

impl Case for Run<'_> {
    fn start(&mut self, source: &dyn Source) -> Result<(), Stop> {
        self.runs += 1;
        if self.fatal.is_some() {
            return Err(Stop::Fail);
        }
        let seen = Seen {
            source,
            drawn: RefCell::new(Vec::new()),
        };
        let start = match self.runner.reset(&seen) {
            Ok(Next {
                state,
                refused: None,
            }) => state,
            Ok(Next {
                refused: Some(e), ..
            })
            | Err(Fault::Thrown(e)) => return Err(self.wire(format!("reset: {e}"))),
            Err(fault) => return Err(self.halt(fault)),
        };
        let judged = self.judge(&start, &start, "")?;
        let line = Line {
            action: None,
            input: seen.written(&Json::Null),
            state: start.clone(),
            evidence: judged.clone().unwrap_or_default(),
        };
        if judged.is_some() {
            return Err(self.fail(vec![line]));
        }
        self.model = self.d.init.clone();
        self.real = start;
        self.trace = vec![line];
        self.passed();
        Ok(())
    }

    fn step(&mut self, source: &dyn Source) -> Result<(), Stop> {
        let d = self.d;
        let enabled = d.enabled(&self.model);
        if enabled.is_empty() {
            return Ok(());
        }
        let (o, verb, to) = enabled[source.index(enabled.len())].clone();
        let input = self.runner.tags.input(source, verb);
        let seen = Seen {
            source,
            drawn: RefCell::new(Vec::new()),
        };
        let applied = self.runner.apply(&o.name, verb, &input, &seen);
        let mut line = Line {
            action: Some((verb.to_string(), o.name.clone())),
            input: seen.written(&input),
            state: State::new(),
            evidence: Vec::new(),
        };
        let next = match applied {
            Ok(Next {
                refused: Some(e), ..
            }) => {
                self.refused.insert(verb.to_string(), e);
                return Ok(());
            }
            Ok(Next {
                state,
                refused: None,
            }) => state,
            Err(Fault::Thrown(e)) => {
                line.evidence = vec![format!("throws {e}")];
                self.trace.push(line);
                let trace = std::mem::take(&mut self.trace);
                return Err(self.fail(trace));
            }
            Err(fault) => return Err(self.halt(fault)),
        };
        line.state = next.clone();
        self.model.insert(o.name.clone(), to);
        let applies = self.c.applies(&self.real, &next, verb);
        if let Some(t) = self.tally.iter_mut().find(|t| t.step == line.step()) {
            t.steps += 1;
            t.checks += u64::from(applies);
        }
        let real = std::mem::take(&mut self.real);
        let judged = self.judge(&real, &next, verb)?;
        line.evidence = judged.clone().unwrap_or_default();
        self.trace.push(line);
        if judged.is_some() {
            let trace = std::mem::take(&mut self.trace);
            return Err(self.fail(trace));
        }
        self.real = next;
        self.passed();
        Ok(())
    }
}

fn listed(ran: Ran, tally: &[Tally]) -> Result<Ran, String> {
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

pub fn simulate(
    d: &Diagram,
    runner: &mut dyn Runner,
    cases: u64,
    key: Option<&str>,
) -> Result<Outcome, String> {
    let tags = Tags::new(d, runner.schema())?;
    let mut run = Run {
        d,
        c: d.check.as_ref().ok_or("needs `check`")?,
        runner: Memo::new(runner, tags),
        runs: 0,
        tally: moves(d)
            .into_iter()
            .map(|step| Tally {
                step,
                steps: 0,
                checks: 0,
            })
            .collect(),
        found: None,
        fatal: None,
        model: State::new(),
        real: State::new(),
        trace: Vec::new(),
        longest: Vec::new(),
        refused: BTreeMap::new(),
    };
    let ended = engine::run(cases, key, &mut run);
    let caveat = run.caveat();
    if let Some(e) = run.fatal {
        return Err(e);
    }
    ended?;
    if let Some((verb, e)) = run.refused.iter().find(|(verb, _)| {
        run.tally
            .iter()
            .any(|t| t.steps == 0 && t.step.split(' ').next() == Some(verb.as_str()))
    }) {
        return Err(format!(
            "each input of `{verb}` fails the schema of its action: {e}"
        ));
    }
    let ran = listed(run.runner.ran()?, &run.tally)?;
    Ok(Outcome {
        cases,
        runs: run.runs,
        tally: run.tally,
        failed: run.found.is_some(),
        trace: run.found.unwrap_or(run.longest),
        caveat,
        ran,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const CONFIG: &str = r#"include "**/*.weft.kdl"
theme light
style person icon=user
style browser icon=app-window
adapter bun cases=100"#;

    const PAGE: &str = r#"
setup {
  person user
  browser page count=0 {
    state count=0 {
      on send count=1
    }
    state count=1 {
      on reload count=1
    }
  }
}
action {
  send from=user to=page
  reload from=user to=page page.count=2
}
check {
  invariant {
    forall browser page {
      and {
        implies {
          in step send
          eq (next)browser.count {
            iadd browser.count 1
          }
        }
        implies {
          in step reload
          eq (next)browser.count browser.count
        }
      }
    }
  }
}
"#;

    struct Page {
        count: i64,
        calls: u64,
        losses: u64,
        thrown: bool,
        refused: fn(i64) -> bool,
        schema: Json,
        ran: Ran,
    }

    fn span(from: usize, to: usize) -> Span {
        Span {
            file: "page.rs".into(),
            from,
            to,
        }
    }

    fn code(verbs: &[&str]) -> Ran {
        Ran {
            reset: vec![span(1, 2)],
            actions: verbs
                .iter()
                .enumerate()
                .map(|(i, v)| (v.to_string(), vec![span(i + 3, i + 3)]))
                .collect(),
        }
    }

    impl Page {
        fn new(losses: u64, thrown: bool, schema: Json) -> Page {
            Page {
                count: 0,
                calls: 0,
                losses,
                thrown,
                refused: |_| false,
                schema,
                ran: code(&["send", "reload"]),
            }
        }

        fn state(&mut self) -> Result<Answer, Fault> {
            self.calls += 1;
            Ok(json!({ "page": { "count": self.count }, "calls": self.calls }).into())
        }
    }

    impl Runner for Page {
        fn schema(&self) -> &Json {
            &self.schema
        }

        fn reset(&mut self, _: &dyn Source) -> Result<Answer, Fault> {
            self.count = 0;
            self.state()
        }

        fn apply(
            &mut self,
            _: &str,
            verb: &str,
            input: &Json,
            _: &dyn Source,
        ) -> Result<Answer, Fault> {
            let times = input.get("times").and_then(Json::as_i64);
            match (verb, times) {
                ("send", _) => self.count += 1,
                (_, Some(t)) if self.thrown && t > 6 => {
                    return Err(Fault::Thrown(format!("reload {t}")));
                }
                (_, t) if (self.refused)(t.unwrap_or(0)) => {
                    return self.state().map(|answer| Answer {
                        refused: Some("too many".into()),
                        ..answer
                    });
                }
                (_, Some(t)) if self.losses > 0 && t > 3 => {
                    self.count = 0;
                    self.losses -= 1;
                }
                _ => {}
            }
            self.state()
        }

        fn ran(&mut self) -> Result<Ran, String> {
            Ok(self.ran.clone())
        }
    }

    fn schema(count: &str, actions: &[&str]) -> Json {
        json!({
            "state": {
                "type": "object",
                "properties": {
                    "page": {
                        "type": "object",
                        "properties": { "count": { "type": "integer", "weft": count } }
                    }
                }
            },
            "actions": actions.iter().map(|verb| json!({
                "type": "object",
                "weft": verb,
                "properties": match *verb {
                    "reload" => json!({ "times": { "type": "integer", "minimum": 0, "maximum": 9 } }),
                    _ => json!({}),
                }
            })).collect::<Vec<_>>()
        })
    }

    fn simulated(runner: &mut dyn Runner) -> Result<Outcome, String> {
        let config = weft_parse::config(CONFIG).unwrap();
        let d = weft_parse::parse(PAGE, &config).unwrap();
        simulate(&d, runner, config.cases, None)
    }

    #[test]
    fn a_page_that_keeps_its_count_holds_in_each_case() {
        let o = simulated(&mut Page::new(
            0,
            false,
            schema("page.count", &["send", "reload"]),
        ))
        .unwrap();
        assert!(!o.failed);
        assert_eq!((o.cases, o.runs), (100, 100));
    }

    #[test]
    fn the_outcome_has_the_code_of_reset_and_of_each_action_that_ran() {
        let page = || Page::new(0, false, schema("page.count", &["send", "reload"]));
        assert_eq!(
            simulated(&mut page()).unwrap().ran,
            code(&["send", "reload"])
        );
        let cases = [
            (
                Ran {
                    reset: Vec::new(),
                    ..code(&["send", "reload"])
                },
                "the runner sends no `ran` for `reset`",
            ),
            (
                code(&["send"]),
                "the runner sends no `ran` for the action `reload`",
            ),
            (
                code(&["send", "reload", "open"]),
                "`ran` names `open`, the verb of no `on` line",
            ),
            (
                Ran {
                    reset: vec![span(2, 1)],
                    ..code(&["send", "reload"])
                },
                "`ran` has the lines 2-1 of `page.rs`, and lines go up from 1",
            ),
        ];
        for (ran, error) in cases {
            let mut runner = Page { ran, ..page() };
            assert_eq!(simulated(&mut runner).err().as_deref(), Some(error));
        }
    }

    #[test]
    fn hegel_shrinks_a_lost_count_to_the_smallest_steps_and_values() {
        let o = simulated(&mut Page::new(
            u64::MAX,
            false,
            schema("page.count", &["send", "reload"]),
        ))
        .unwrap();
        assert!(o.caveat.is_none());
        assert!(o.failed);
        let rows: Vec<(String, &str, String)> = o
            .trace
            .iter()
            .map(|l| (l.step(), l.input.as_str(), view(&l.state)))
            .collect();
        assert_eq!(
            rows,
            [
                ("start".into(), "", "page: count = 0".into()),
                ("send to=page".into(), "", "page: count = 1".into()),
                ("reload to=page".into(), "times=4", "page: count = 0".into()),
            ]
        );
        assert_eq!(
            o.trace[2].evidence.last().map(String::as_str),
            Some("eq(next(browser.count), browser.count) = eq(0, 1) = false")
        );
        assert!(o.runs > o.cases);
    }

    #[test]
    fn a_count_that_is_lost_one_time_is_found_and_fails_in_no_replay() {
        let o = simulated(&mut Page::new(
            1,
            false,
            schema("page.count", &["send", "reload"]),
        ))
        .unwrap();
        assert!(o.failed);
        assert_eq!(o.caveat.as_deref(), Some("fails in 0 of 10 replays"));
    }

    #[test]
    fn a_throw_of_the_code_is_a_failing_step() {
        let o = simulated(&mut Page::new(
            0,
            true,
            schema("page.count", &["send", "reload"]),
        ))
        .unwrap();
        assert!(o.failed);
        assert_eq!(
            o.trace
                .last()
                .map(|l| (l.input.as_str(), l.evidence.clone())),
            Some(("times=7", vec!["throws reload 7".to_string()]))
        );
    }

    #[test]
    fn an_input_that_the_action_refuses_is_no_step() {
        let page = |refused| Page {
            refused,
            ..Page::new(0, false, schema("page.count", &["send", "reload"]))
        };
        let o = simulated(&mut page(|t| t > 2)).unwrap();
        assert!(!o.failed);
        assert!(o.tally.iter().all(|t| t.steps > 0));
        assert_eq!(
            simulated(&mut page(|_| true)).err().as_deref(),
            Some("each input of `reload` fails the schema of its action: too many")
        );
    }

    struct Picks {
        schema: Json,
        drawn: Json,
        fails: fn(&Json) -> bool,
        count: i64,
    }

    impl Runner for Picks {
        fn schema(&self) -> &Json {
            &self.schema
        }

        fn reset(&mut self, _: &dyn Source) -> Result<Answer, Fault> {
            self.count = 0;
            Ok(json!({ "page": { "count": self.count } }).into())
        }

        fn apply(
            &mut self,
            _: &str,
            verb: &str,
            _: &Json,
            source: &dyn Source,
        ) -> Result<Answer, Fault> {
            if verb == "send" {
                let c = choice("draw", &self.drawn).map_err(Fault::Broken)?;
                self.count += match (self.fails)(&source.draw(&c)) {
                    true => 2,
                    false => 1,
                };
            }
            Ok(json!({ "page": { "count": self.count } }).into())
        }

        fn ran(&mut self) -> Result<Ran, String> {
            Ok(code(&["send", "reload"]))
        }
    }

    fn shrunk(drawn: Json, fails: fn(&Json) -> bool) -> Json {
        let mut picks = Picks {
            schema: schema("page.count", &["send", "reload"]),
            drawn,
            fails,
            count: 0,
        };
        let o = simulated(&mut picks).unwrap();
        assert!(o.failed);
        let input = &o.trace[1].input;
        serde_json::from_str(input.strip_prefix("drawn=").unwrap()).unwrap()
    }

    #[test]
    fn hegel_shrinks_a_union_to_its_first_failing_branch_and_value() {
        let drawn = json!({ "anyOf": [
            { "type": "object", "properties": { "verb": { "const": "send" } }, "required": ["verb"] },
            { "type": "object", "properties": {
                "verb": { "const": "reload" },
                "times": { "type": "integer", "minimum": 0, "maximum": 999 }
            }, "required": ["verb", "times"] }
        ] });
        assert_eq!(
            shrunk(drawn, |v| v["times"].as_u64() >= Some(3)),
            json!({ "verb": "reload", "times": 3 })
        );
    }

    #[test]
    fn hegel_shrinks_an_array_to_the_fewest_items_that_fail() {
        let drawn =
            json!({ "type": "array", "items": { "type": "integer", "minimum": 0, "maximum": 9 } });
        let v = shrunk(drawn, |v| {
            v.as_array()
                .unwrap()
                .iter()
                .filter_map(Json::as_i64)
                .sum::<i64>()
                >= 10
        });
        let all: Vec<i64> = v
            .as_array()
            .unwrap()
            .iter()
            .filter_map(Json::as_i64)
            .collect();
        assert_eq!((all.len(), all.iter().sum::<i64>()), (2, 10));
    }

    #[test]
    fn each_draw_is_a_json_schema_that_weft_draws() {
        assert!(choice("draw", &json!({ "weft": "any" })) == Ok(Choice::Any));
        for node in [json!({ "type": "array" }), json!({ "not": {} })] {
            assert_eq!(
                choice("draw", &node).err(),
                Some(format!(
                    "the field `draw` is not a JSON Schema that weft draws: `{node}`"
                ))
            );
        }
    }

    #[test]
    fn hegel_shrinks_a_number_a_null_and_a_record_to_the_least_that_fails() {
        let number = json!({ "type": "number", "minimum": 0, "maximum": 9 });
        assert_eq!(shrunk(number, |v| v.as_f64() >= Some(2.5)), json!(3.0));
        let null = json!({ "anyOf": [{ "type": "integer", "minimum": 0, "maximum": 9 }, { "type": "null" }] });
        assert_eq!(shrunk(null, Json::is_null), Json::Null);
        let record = json!({
            "type": "object",
            "propertyNames": { "enum": ["a", "b"] },
            "additionalProperties": { "type": "integer", "minimum": 0, "maximum": 9 }
        });
        assert_eq!(
            shrunk(record, |v| v["b"].as_i64() >= Some(4)),
            json!({ "b": 4 })
        );
    }

    #[test]
    fn hegel_shrinks_a_schema_of_itself_a_pattern_a_tuple_and_an_all_of() {
        let tree = json!({
            "$ref": "#/$defs/node",
            "$defs": { "node": {
                "type": "object",
                "properties": {
                    "tag": { "enum": ["div", "input"] },
                    "children": { "type": "array", "items": { "$ref": "#/$defs/node" } }
                },
                "required": ["tag", "children"]
            } }
        });
        assert_eq!(
            shrunk(tree, |v| v["children"][0]["children"][0]["tag"] == "input"),
            json!({ "tag": "div", "children": [
                { "tag": "div", "children": [{ "tag": "input", "children": [] }] }
            ] })
        );
        let pattern = json!({ "type": "string", "pattern": "^snap_[0-9]+$" });
        assert_eq!(
            shrunk(pattern, |v| v.as_str().is_some_and(|s| s.len() > 6)),
            json!("snap_00")
        );
        let digits = json!({ "type": "string", "pattern": "^(0|[1-9]\\d*)$" });
        assert_eq!(
            shrunk(digits, |v| v
                .as_str()
                .is_some_and(
                    |s| !s.bytes().all(|b| b.is_ascii_digit()) || s.len() > 1
                )),
            json!("10")
        );
        let tuple = json!({ "type": "array", "prefixItems": [
            { "type": "integer", "minimum": 0, "maximum": 9 },
            { "type": "boolean" }
        ] });
        assert_eq!(
            shrunk(tuple, |v| v[0].as_i64() >= Some(2) && v[1] == true),
            json!([2, true])
        );
        let all = json!({ "allOf": [
            { "type": "object", "properties": { "key": { "const": "k" } }, "required": ["key"] },
            { "anyOf": [
                { "type": "object", "properties": { "kind": { "const": "a" } }, "required": ["kind"] },
                { "type": "object", "properties": {
                    "kind": { "const": "b" },
                    "size": { "type": "integer", "minimum": 0, "maximum": 9 }
                }, "required": ["kind", "size"] }
            ] }
        ] });
        assert_eq!(
            shrunk(all, |v| v["size"].as_i64() >= Some(5)),
            json!({ "key": "k", "kind": "b", "size": 5 })
        );
        let unique = json!({
            "type": "array",
            "uniqueItems": true,
            "items": { "type": "integer", "minimum": 0, "maximum": 3 }
        });
        assert_eq!(
            shrunk(unique, |v| v.as_array().is_some_and(|all| all.len() >= 3)),
            json!([0, 1, 2])
        );
    }

    #[test]
    fn a_schema_that_has_itself_in_each_value_has_no_value_that_weft_draws() {
        let node = json!({
            "$ref": "#/$defs/node",
            "$defs": { "node": {
                "type": "object",
                "properties": { "next": { "$ref": "#/$defs/node" } },
                "required": ["next"]
            } }
        });
        assert_eq!(
            choice("draw", &node).err().as_deref(),
            Some(
                "the field `draw` has the schema `node` in each value of `node`, and weft draws no value without an end"
            )
        );
    }

    #[test]
    fn each_read_of_the_check_and_each_verb_has_a_tag() {
        let cases = [
            (
                schema("page.size", &["send", "reload"]),
                "the tag `page.size` names no key of `page`",
            ),
            (
                schema("card.count", &["send", "reload"]),
                "the check reads `page.count`, and no field of the state has the tag `page.count`",
            ),
            (
                schema("page.count", &["send"]),
                "the verb `reload` has no action with the tag `reload`",
            ),
        ];
        for (schema, error) in cases {
            assert_eq!(
                simulated(&mut Page::new(0, false, schema)).err().as_deref(),
                Some(error)
            );
        }
    }
}
