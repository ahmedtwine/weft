mod engine;

use std::cell::RefCell;

use serde_json::{Map, Value as Json};
use weft_parse::{Check, Diagram, State, Value, shown};

pub use engine::{Hegel, Random};

const STEPS: u32 = 20;

pub enum Fault {
    Thrown(String),
    Broken(String),
}

pub enum Field {
    Text(usize, Option<usize>),
    Int(i64, i64),
    Bool,
}

pub trait Source {
    fn draw(&self, field: &Field) -> Json;
    fn pick(&self, count: usize) -> usize;
    fn target(&self, score: f64);
}

pub struct Stop;

pub trait Case {
    fn start(&mut self, source: &dyn Source) -> Result<(), Stop>;
    fn step(&mut self, source: &dyn Source) -> Result<(), Stop>;
}

pub trait Engine {
    fn run(&mut self, cases: u64, steps: u32, case: &mut dyn Case) -> Result<(), String>;
}

pub fn engine(engine: &weft_parse::Engine) -> Box<dyn Engine> {
    match engine {
        weft_parse::Engine::Hegel => Box::new(Hegel),
        weft_parse::Engine::Random(seed) => Box::new(Random::new(*seed)),
    }
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
}

pub struct Line {
    pub step: String,
    pub input: String,
    pub state: String,
    pub evidence: Vec<String>,
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
    pub found: Option<Vec<Line>>,
}

struct Read {
    path: Vec<String>,
    object: String,
    key: String,
    like: Value,
}

struct Tags {
    reads: Vec<Read>,
    actions: Vec<(String, Vec<(String, Field)>)>,
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

fn bound(node: &Json, k: &str) -> Option<i64> {
    node.get(k).and_then(Json::as_i64)
}

pub fn field(name: &str, node: &Json) -> Result<Field, String> {
    let size = |k: &str| bound(node, k).and_then(|n| usize::try_from(n).ok());
    match node.get("type").and_then(Json::as_str) {
        Some("string") => Ok(Field::Text(
            size("minLength").unwrap_or(0),
            size("maxLength"),
        )),
        Some("integer") => Ok(Field::Int(
            bound(node, "minimum").unwrap_or(i64::MIN),
            bound(node, "maximum").unwrap_or(i64::MAX),
        )),
        Some("boolean") => Ok(Field::Bool),
        _ => Err(format!(
            "the field `{name}` is a string, an integer or a boolean, not `{node}`"
        )),
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
        let mut actions: Vec<(String, Vec<(String, Field)>)> = Vec::new();
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
            let fields = match node.get("properties") {
                Some(Json::Object(all)) => all
                    .iter()
                    .map(|(k, v)| field(k, v).map(|f| (k.clone(), f)))
                    .collect::<Result<_, _>>()?,
                _ => Vec::new(),
            };
            actions.push((verb.to_string(), fields));
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
        let fields = self
            .actions
            .iter()
            .find(|(v, _)| v == verb)
            .map_or(&[][..], |(_, f)| f);
        Json::Object(
            fields
                .iter()
                .map(|(k, f)| (k.clone(), source.draw(f)))
                .collect::<Map<_, _>>(),
        )
    }
}

fn steps(d: &Diagram) -> Vec<String> {
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

fn view(s: &State) -> String {
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
    fn draw(&self, field: &Field) -> Json {
        let value = self.source.draw(field);
        self.drawn.borrow_mut().push(value.to_string());
        value
    }

    fn pick(&self, count: usize) -> usize {
        self.source.pick(count)
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
    tags: Tags,
    runner: &'a mut dyn Runner,
    runs: u64,
    tally: Vec<Tally>,
    found: Option<Vec<Line>>,
    fatal: Option<String>,
    model: State,
    real: State,
    trace: Vec<Line>,
}

impl Run<'_> {
    fn fail(&mut self, trace: Vec<Line>) -> Stop {
        self.found = Some(trace);
        Stop
    }

    fn wire(&mut self, e: String) -> Stop {
        self.fatal = Some(e);
        Stop
    }

    fn judge(&mut self, cur: &State, nxt: &State, verb: &str) -> Result<Option<Vec<String>>, Stop> {
        match self.runner.check(self.c, cur, nxt, verb) {
            Ok(evidence) => Ok(evidence),
            Err(Fault::Thrown(e)) => Ok(Some(vec![format!("throws {e}")])),
            Err(Fault::Broken(e)) => Err(self.wire(e)),
        }
    }

    fn read(&mut self, raw: &Json) -> Result<State, Stop> {
        self.tags.read(raw).map_err(|e| self.wire(e))
    }
}

impl Case for Run<'_> {
    fn start(&mut self, source: &dyn Source) -> Result<(), Stop> {
        self.runs += 1;
        if self.fatal.is_some() {
            return Err(Stop);
        }
        let start = match self.runner.reset(source) {
            Ok(raw) => self.read(&raw)?,
            Err(Fault::Thrown(e) | Fault::Broken(e)) => {
                return Err(self.wire(format!("reset: {e}")));
            }
        };
        if let Some(evidence) = self.judge(&start, &start, "")? {
            let line = Line {
                step: "start".into(),
                input: String::new(),
                state: view(&start),
                evidence,
            };
            return Err(self.fail(vec![line]));
        }
        self.model = self.d.init.clone();
        self.real = start;
        self.trace = Vec::new();
        Ok(())
    }

    fn step(&mut self, source: &dyn Source) -> Result<(), Stop> {
        let d = self.d;
        let enabled = d.enabled(&self.model);
        if enabled.is_empty() {
            return Ok(());
        }
        let (o, verb, to) = enabled[source.pick(enabled.len())].clone();
        let input = self.tags.input(source, verb);
        let seen = Seen {
            source,
            drawn: RefCell::new(Vec::new()),
        };
        let applied = self.runner.apply(&o.name, verb, &input, &seen);
        let mut line = Line {
            step: format!("{verb} to={}", o.name),
            input: seen.written(&input),
            state: String::new(),
            evidence: Vec::new(),
        };
        let raw = match applied {
            Ok(raw) => raw,
            Err(Fault::Thrown(e)) => {
                line.evidence = vec![format!("throws {e}")];
                self.trace.push(line);
                let trace = std::mem::take(&mut self.trace);
                return Err(self.fail(trace));
            }
            Err(Fault::Broken(e)) => return Err(self.wire(e)),
        };
        let next = self.read(&raw)?;
        line.state = view(&next);
        self.model.insert(o.name.clone(), to);
        let applies = self.c.applies(&self.real, &next, verb);
        if let Some(t) = self.tally.iter_mut().find(|t| t.step == line.step) {
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
        Ok(())
    }
}

pub fn simulate(
    d: &Diagram,
    runner: &mut dyn Runner,
    engine: &mut dyn Engine,
    cases: u64,
) -> Result<Outcome, String> {
    let mut run = Run {
        d,
        c: d.check.as_ref().ok_or("needs `check`")?,
        tags: Tags::new(d, runner.schema())?,
        runner,
        runs: 0,
        tally: steps(d)
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
    };
    let ran = engine.run(cases, STEPS, &mut run);
    if let Some(e) = run.fatal {
        return Err(e);
    }
    ran?;
    Ok(Outcome {
        cases,
        runs: run.runs,
        tally: run.tally,
        found: run.found,
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
        lost: bool,
        thrown: bool,
        schema: Json,
    }

    impl Page {
        fn new(lost: bool, thrown: bool, schema: Json) -> Page {
            Page {
                count: 0,
                lost,
                thrown,
                schema,
            }
        }
    }

    impl Runner for Page {
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
            input: &Json,
            _: &dyn Source,
        ) -> Result<Json, Fault> {
            let times = input.get("times").and_then(Json::as_i64);
            match (verb, times) {
                ("send", _) => self.count += 1,
                (_, Some(t)) if self.thrown && t > 6 => {
                    return Err(Fault::Thrown(format!("reload {t}")));
                }
                (_, Some(t)) if self.lost && t > 3 => self.count = 0,
                _ => {}
            }
            Ok(json!({ "page": { "count": self.count } }))
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

    fn simulated(page: &mut Page) -> Result<Outcome, String> {
        simulated_by(page, &mut Hegel)
    }

    fn simulated_by(page: &mut Page, engine: &mut dyn Engine) -> Result<Outcome, String> {
        let config = weft_parse::config(CONFIG).unwrap();
        let d = weft_parse::parse(PAGE, &config).unwrap();
        simulate(&d, page, engine, config.cases)
    }

    #[test]
    fn the_random_engine_finds_a_lost_count_without_a_shrink() {
        let o = simulated_by(
            &mut Page::new(true, false, schema("page.count", &["send", "reload"])),
            &mut Random::new(7),
        )
        .unwrap();
        let trace = o.found.unwrap();
        assert_eq!(
            trace.last().map(|l| l.state.as_str()),
            Some("page: count = 0")
        );
        assert!(o.runs <= o.cases);
    }

    #[test]
    fn a_page_that_keeps_its_count_holds_in_each_case() {
        let o = simulated(&mut Page::new(
            false,
            false,
            schema("page.count", &["send", "reload"]),
        ))
        .unwrap();
        assert!(o.found.is_none());
        assert_eq!((o.cases, o.runs), (100, 100));
    }

    #[test]
    fn hegel_shrinks_a_lost_count_to_the_smallest_steps_and_values() {
        let o = simulated(&mut Page::new(
            true,
            false,
            schema("page.count", &["send", "reload"]),
        ))
        .unwrap();
        let trace = o.found.unwrap();
        let rows: Vec<(&str, &str, &str)> = trace
            .iter()
            .map(|l| (l.step.as_str(), l.input.as_str(), l.state.as_str()))
            .collect();
        assert_eq!(
            rows,
            [
                ("send to=page", "", "page: count = 1"),
                ("reload to=page", "times=4", "page: count = 0"),
            ]
        );
        assert_eq!(
            trace[1].evidence.last().map(String::as_str),
            Some("eq(next(browser.count), browser.count) = eq(0, 1) = false")
        );
        assert!(o.runs > o.cases);
    }

    #[test]
    fn a_throw_of_the_code_is_a_failing_step() {
        let o = simulated(&mut Page::new(
            false,
            true,
            schema("page.count", &["send", "reload"]),
        ))
        .unwrap();
        let trace = o.found.unwrap();
        assert_eq!(
            trace.last().map(|l| (l.input.as_str(), l.evidence.clone())),
            Some(("times=7", vec!["throws reload 7".to_string()]))
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
            (
                schema("page.count", &["send", "reload", "open"]),
                "the tag `open` of an action is the verb of no `on` line",
            ),
        ];
        for (schema, error) in cases {
            assert_eq!(
                simulated(&mut Page::new(false, false, schema))
                    .err()
                    .as_deref(),
                Some(error)
            );
        }
    }
}
