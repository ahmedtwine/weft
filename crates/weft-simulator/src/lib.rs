mod bun;

use std::cell::{Cell, RefCell};
use std::panic::{self, AssertUnwindSafe};

use hegel::generators as gs;
use hegel::stateful::{self, Invariant, Rule, StateMachine};
use hegel::{Database, Hegel, Settings, TestCase, Verbosity};
use serde_json::{Map, Value as Json};
use weft_parse::{Adapter, Check, Diagram, State, Value, shown};

pub use bun::Bun;

const STEPS: i64 = 20;
const STOP: &str = "weft";

pub enum Fault {
    Thrown(String),
    Broken(String),
}

pub trait Runner {
    fn schema(&self) -> &Json;
    fn reset(&mut self) -> Result<Json, Fault>;
    fn apply(&mut self, object: &str, verb: &str, input: &Json) -> Result<Json, Fault>;
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
    pub adapter: String,
    pub cases: u64,
    pub runs: u64,
    pub tally: Vec<Tally>,
    pub found: Option<Vec<Line>>,
}

enum Field {
    Text(usize, Option<usize>),
    Int(i64, i64),
    Bool,
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

fn field(name: &str, node: &Json) -> Result<Field, String> {
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
            .ok_or("the adapter sends no schema of the state")?;
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
            .ok_or("the adapter sends no schemas of the actions")?
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

    fn input(&self, tc: &TestCase, verb: &str) -> Json {
        let fields = self
            .actions
            .iter()
            .find(|(v, _)| v == verb)
            .map_or(&[][..], |(_, f)| f);
        Json::Object(
            fields
                .iter()
                .map(|(k, f)| {
                    let v = match f {
                        Field::Text(min, Some(max)) => {
                            Json::from(tc.draw(gs::text().min_size(*min).max_size(*max)))
                        }
                        Field::Text(min, None) => Json::from(tc.draw(gs::text().min_size(*min))),
                        Field::Int(min, max) => Json::from(
                            tc.draw(gs::integers::<i64>().min_value(*min).max_value(*max)),
                        ),
                        Field::Bool => Json::from(tc.draw(gs::booleans())),
                    };
                    (k.clone(), v)
                })
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

fn written(input: &Json) -> String {
    input.as_object().map_or(String::new(), |all| {
        all.iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect::<Vec<_>>()
            .join(" ")
    })
}

fn said(p: &(dyn std::any::Any + Send)) -> String {
    p.downcast_ref::<&str>()
        .map(|s| s.to_string())
        .or_else(|| p.downcast_ref::<String>().cloned())
        .unwrap_or_default()
}

struct Run<'a> {
    d: &'a Diagram,
    c: &'a Check,
    tags: Tags,
    runner: RefCell<&'a mut dyn Runner>,
    runs: Cell<u64>,
    tally: RefCell<Vec<Tally>>,
    found: RefCell<Option<Vec<Line>>>,
    fatal: RefCell<Option<String>>,
}

impl Run<'_> {
    fn fail(&self, trace: Vec<Line>) -> ! {
        *self.found.borrow_mut() = Some(trace);
        panic!("{STOP}")
    }

    fn wire(&self, e: String) -> ! {
        *self.fatal.borrow_mut() = Some(e);
        panic!("{STOP}")
    }

    fn case(&self, tc: TestCase) {
        self.runs.set(self.runs.get() + 1);
        let known = self.fatal.borrow().clone();
        if let Some(e) = known {
            self.wire(e)
        }
        let start = match self.runner.borrow_mut().reset() {
            Ok(raw) => self.tags.read(&raw).unwrap_or_else(|e| self.wire(e)),
            Err(Fault::Thrown(e) | Fault::Broken(e)) => self.wire(format!("reset: {e}")),
        };
        if let Some(evidence) = self.c.judge(&start, &start, "") {
            self.fail(vec![Line {
                step: "start".into(),
                input: String::new(),
                state: view(&start),
                evidence,
            }])
        }
        stateful::machine(Case {
            run: self,
            model: self.d.init.clone(),
            real: start,
            trace: Vec::new(),
        })
        .steps(STEPS)
        .run(tc);
    }
}

struct Case<'r, 'a> {
    run: &'r Run<'a>,
    model: State,
    real: State,
    trace: Vec<Line>,
}

impl Case<'_, '_> {
    fn step(&mut self, tc: TestCase) {
        let r = self.run;
        let enabled = r.d.enabled(&self.model);
        if enabled.is_empty() {
            return;
        }
        let k = tc.draw(gs::sampled_from((0..enabled.len()).collect::<Vec<_>>()));
        let (o, verb, to) = enabled[k].clone();
        let input = r.tags.input(&tc, verb);
        let mut line = Line {
            step: format!("{verb} to={}", o.name),
            input: written(&input),
            state: String::new(),
            evidence: Vec::new(),
        };
        let raw = match r.runner.borrow_mut().apply(&o.name, verb, &input) {
            Ok(raw) => raw,
            Err(Fault::Thrown(e)) => {
                line.evidence = vec![format!("throws {e}")];
                self.trace.push(line);
                r.fail(std::mem::take(&mut self.trace))
            }
            Err(Fault::Broken(e)) => r.wire(e),
        };
        let next = r.tags.read(&raw).unwrap_or_else(|e| r.wire(e));
        line.state = view(&next);
        self.model.insert(o.name.clone(), to);
        if let Some(t) = r
            .tally
            .borrow_mut()
            .iter_mut()
            .find(|t| t.step == line.step)
        {
            t.steps += 1;
            t.checks += u64::from(r.c.applies(&self.real, &next, verb));
        }
        let judged = r.c.judge(&self.real, &next, verb);
        line.evidence = judged.clone().unwrap_or_default();
        self.trace.push(line);
        if judged.is_some() {
            r.fail(std::mem::take(&mut self.trace))
        }
        self.real = next;
    }
}

impl StateMachine for Case<'_, '_> {
    fn rules(&self) -> Vec<Rule<Self>> {
        vec![Rule::new("step", 1.0, Case::step)]
    }

    fn invariants(&self) -> Vec<Invariant<Self>> {
        Vec::new()
    }
}

pub fn simulate(
    d: &Diagram,
    runner: &mut dyn Runner,
    adapter: &Adapter,
) -> Result<Outcome, String> {
    let run = Run {
        d,
        c: d.check.as_ref().ok_or("needs `check`")?,
        tags: Tags::new(d, runner.schema())?,
        runner: RefCell::new(runner),
        runs: Cell::new(0),
        tally: RefCell::new(
            steps(d)
                .into_iter()
                .map(|step| Tally {
                    step,
                    steps: 0,
                    checks: 0,
                })
                .collect(),
        ),
        found: RefCell::new(None),
        fatal: RefCell::new(None),
    };
    let ran = panic::catch_unwind(AssertUnwindSafe(|| {
        Hegel::new(|tc: TestCase| run.case(tc))
            .settings(
                Settings::new()
                    .test_cases(adapter.cases)
                    .database(Database::Disabled)
                    .verbosity(Verbosity::Quiet)
                    .print_blob(false),
            )
            .run()
    }));
    if let Some(e) = run.fatal.take() {
        return Err(e);
    }
    let outcome = |found| Outcome {
        adapter: adapter.name.clone(),
        cases: adapter.cases,
        runs: run.runs.get(),
        tally: run.tally.take(),
        found,
    };
    match ran {
        Ok(()) => Ok(outcome(None)),
        Err(p) if said(&*p) == STOP => Ok(outcome(run.found.take())),
        Err(p) => Err(said(&*p)),
    }
}

pub fn run(d: &Diagram, file: &std::path::Path, adapter: &Adapter) -> Result<Outcome, String> {
    let stem = file
        .to_str()
        .and_then(|f| f.strip_suffix(".kdl"))
        .ok_or_else(|| format!("{} does not end in .kdl", file.display()))?;
    let mut runner: Box<dyn Runner> = match adapter.name.as_str() {
        "bun" => Box::new(Bun::start(std::path::Path::new(&format!("{stem}.ts")))?),
        other => return Err(format!("`adapter {other}` is not `adapter bun`")),
    };
    simulate(d, runner.as_mut(), adapter)
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

        fn reset(&mut self) -> Result<Json, Fault> {
            self.count = 0;
            Ok(json!({ "page": { "count": self.count } }))
        }

        fn apply(&mut self, _: &str, verb: &str, input: &Json) -> Result<Json, Fault> {
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
        let config = weft_parse::config(CONFIG).unwrap();
        let d = weft_parse::parse(PAGE, &config).unwrap();
        let adapter = config.adapter.as_ref().unwrap();
        simulate(&d, page, adapter)
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
