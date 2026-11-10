mod engine;
mod memo;

use std::cell::RefCell;

use serde_json::{Map, Value as Json};
use weft_parse::{Check, Diagram, Ran, Span, State, Value, shown};

use memo::Memo;

const REPLAYS: usize = 10;

pub enum Fault {
    Thrown(String),
    Broken(String),
    Changed,
}

#[derive(Clone, PartialEq)]
pub enum Choice {
    Bool,
    Int(i64, i64),
    Text(usize, Option<usize>),
    Enum(Vec<Json>),
    AnyOf(Vec<Choice>),
    Array(Box<Choice>, usize, usize),
    Object(Vec<(String, Choice, bool)>),
}

pub trait Source {
    fn draw(&self, choice: &Choice) -> Json;
    fn bool(&self) -> bool;
    fn int(&self, min: i64, max: i64) -> i64;
    fn text(&self, min: usize, max: Option<usize>) -> String;
    fn index(&self, count: usize) -> usize;
    fn target(&self, score: f64);
}

impl Choice {
    pub fn draw(&self, s: &dyn Source) -> Json {
        match self {
            Choice::Bool => Json::from(s.bool()),
            Choice::Int(min, max) => Json::from(s.int(*min, *max)),
            Choice::Text(min, max) => Json::from(s.text(*min, *max)),
            Choice::Enum(values) => values[s.index(values.len())].clone(),
            Choice::AnyOf(all) => all[s.index(all.len())].draw(s),
            Choice::Array(item, min, max) => {
                let size = s.int(*min as i64, *max as i64) as usize;
                Json::Array((0..size).map(|_| item.draw(s)).collect())
            }
            Choice::Object(fields) => Json::Object(
                fields
                    .iter()
                    .filter(|(_, _, required)| *required || s.bool())
                    .map(|(k, c, _)| (k.clone(), c.draw(s)))
                    .collect::<Map<_, _>>(),
            ),
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
    fn reset(&mut self, source: &dyn Source) -> Result<Json, Fault>;
    fn apply(
        &mut self,
        object: &str,
        verb: &str,
        input: &Json,
        source: &dyn Source,
    ) -> Result<Json, Fault>;
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

fn tags(node: &Json) -> Vec<&str> {
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

pub fn choice(name: &str, node: &Json) -> Result<Choice, String> {
    let int = |k: &str| node.get(k).and_then(Json::as_i64);
    let size = |k: &str| int(k).and_then(|n| usize::try_from(n).ok());
    let wrong = || format!("the field `{name}` is not a JSON Schema that weft draws: `{node}`");
    let listed = |k: &str| {
        node.get(k)
            .and_then(Json::as_array)
            .filter(|all| !all.is_empty())
    };
    match (
        listed("enum"),
        node.get("const"),
        listed("anyOf").or_else(|| listed("oneOf")),
        node.get("type").and_then(Json::as_str),
    ) {
        (Some(values), ..) => Ok(Choice::Enum(values.clone())),
        (_, Some(value), ..) => Ok(Choice::Enum(vec![value.clone()])),
        (_, _, Some(all), _) => Ok(Choice::AnyOf(
            all.iter()
                .map(|b| choice(name, b))
                .collect::<Result<_, _>>()?,
        )),
        (.., Some("integer")) => Ok(Choice::Int(
            int("minimum").unwrap_or(i64::MIN),
            int("maximum").unwrap_or(i64::MAX),
        )),
        (.., Some("string")) => Ok(Choice::Text(
            size("minLength").unwrap_or(0),
            size("maxLength"),
        )),
        (.., Some("boolean")) => Ok(Choice::Bool),
        (.., Some("array")) => {
            let item = node.get("items").ok_or_else(wrong)?;
            let max = size("maxItems")
                .ok_or_else(|| format!("the array `{name}` takes `maxItems`: `{node}`"))?;
            Ok(Choice::Array(
                Box::new(choice(&format!("{name}[]"), item)?),
                size("minItems").unwrap_or(0),
                max,
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
                        choice(&format!("{name}.{k}"), v)
                            .map(|c| (k.clone(), c, required.contains(&k.as_str())))
                    })
                    .collect::<Result<_, _>>()
                    .map(Choice::Object),
                _ => Ok(Choice::Object(Vec::new())),
            }
        }
        _ => Err(wrong()),
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
                return Err(format!(
                    "the tag `{verb}` of an action is the verb of no `on` line"
                ));
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
            .map_or(Json::Object(Map::new()), |(_, c)| c.draw(source))
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

    fn bool(&self) -> bool {
        self.source.bool()
    }

    fn int(&self, min: i64, max: i64) -> i64 {
        self.source.int(min, max)
    }

    fn text(&self, min: usize, max: Option<usize>) -> String {
        self.source.text(min, max)
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
        let start = match self.runner.reset(source) {
            Ok(start) => start,
            Err(Fault::Thrown(e)) => return Err(self.wire(format!("reset: {e}"))),
            Err(fault) => return Err(self.halt(fault)),
        };
        let judged = self.judge(&start, &start, "")?;
        let line = Line {
            action: None,
            input: String::new(),
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
            Ok(next) => next,
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
    };
    let ended = engine::run(cases, key, &mut run);
    let caveat = run.caveat();
    if let Some(e) = run.fatal {
        return Err(e);
    }
    ended?;
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
                schema,
                ran: code(&["send", "reload"]),
            }
        }

        fn state(&mut self) -> Result<Json, Fault> {
            self.calls += 1;
            Ok(json!({ "page": { "count": self.count }, "calls": self.calls }))
        }
    }

    impl Runner for Page {
        fn schema(&self) -> &Json {
            &self.schema
        }

        fn reset(&mut self, _: &dyn Source) -> Result<Json, Fault> {
            self.count = 0;
            self.state()
        }

        fn apply(
            &mut self,
            _: &str,
            verb: &str,
            input: &Json,
            _: &dyn Source,
        ) -> Result<Json, Fault> {
            let times = input.get("times").and_then(Json::as_i64);
            match (verb, times) {
                ("send", _) => self.count += 1,
                (_, Some(t)) if self.thrown && t > 6 => {
                    return Err(Fault::Thrown(format!("reload {t}")));
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

        fn reset(&mut self, _: &dyn Source) -> Result<Json, Fault> {
            self.count = 0;
            Ok(json!({ "page": { "count": self.count } }))
        }

        fn apply(
            &mut self,
            _: &str,
            verb: &str,
            _: &Json,
            source: &dyn Source,
        ) -> Result<Json, Fault> {
            if verb == "send" {
                let c = choice("draw", &self.drawn).map_err(Fault::Broken)?;
                self.count += match (self.fails)(&source.draw(&c)) {
                    true => 2,
                    false => 1,
                };
            }
            Ok(json!({ "page": { "count": self.count } }))
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
        let drawn = json!({ "type": "array", "items": { "type": "integer", "minimum": 0, "maximum": 9 }, "maxItems": 5 });
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
        let cases = [
            (
                json!({ "type": "array", "items": { "type": "boolean" } }),
                "the array `draw` takes `maxItems`",
            ),
            (
                json!({ "type": "null" }),
                "the field `draw` is not a JSON Schema that weft draws",
            ),
        ];
        for (node, error) in cases {
            assert_eq!(
                choice("draw", &node).err(),
                Some(format!("{error}: `{node}`"))
            );
        }
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
            (
                schema("page.count", &["send", "reload", "open"]),
                "the tag `open` of an action is the verb of no `on` line",
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
