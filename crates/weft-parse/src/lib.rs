use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fmt;

use kdl::KdlValue;
pub use kdl::{KdlDocument, KdlEntry, KdlNode};
use miette::SourceCode;
use serde_json::{Value as Json, json};

pub type Pairs = BTreeMap<String, Json>;

pub type State = BTreeMap<String, Pairs>;

#[derive(Debug, Clone, Default)]
pub struct Diagram {
    pub objects: Vec<Object>,
    pub path: Vec<Step>,
    pub impls: Vec<Impl>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Start,
    Read,
    Run,
}

#[derive(Debug, Clone)]
pub struct Draw {
    pub path: Vec<String>,
    pub values: Vec<Json>,
    pub keys: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Name {
    pub module: String,
    pub export: String,
}

impl fmt::Display for Name {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self.export.is_empty() {
            true => f.write_str(&self.module),
            false => write!(f, "{}.{}", self.module, self.export),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Use {
    pub role: Role,
    pub function: Name,
    pub input: Option<Name>,
    pub output: Option<Name>,
    pub error: Option<Name>,
    pub tag: Option<String>,
    pub verb: Option<String>,
    pub pool: Option<String>,
    pub from: Option<(String, bool)>,
}

impl Use {
    pub fn line(&self) -> String {
        let role = match self.role {
            Role::Start => "start",
            Role::Read => "read",
            Role::Run => "run",
        };
        let named =
            |key: &str, name: &Option<Name>| name.as_ref().map(|n| format!(" {key}=\"{n}\""));
        let text =
            |key: &str, text: &Option<String>| text.as_ref().map(|t| format!(" {key}=\"{t}\""));
        let parts = [
            named("input", &self.input),
            named("output", &self.output),
            named("error", &self.error),
            text("tag", &self.tag),
            text("verb", &self.verb),
            text("pool", &self.pool),
            self.from.as_ref().map(|(pool, consume)| match consume {
                true => format!(" consume=\"{pool}\""),
                false => format!(" from=\"{pool}\""),
            }),
        ];
        format!(
            "`{role} \"{}\"{}`",
            self.function,
            parts.into_iter().flatten().collect::<String>()
        )
    }
}

#[derive(Debug, Clone)]
pub struct Impl {
    pub adapter: String,
    pub modules: Vec<(String, String)>,
    pub uses: Vec<Use>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    pub file: String,
    pub from: usize,
    pub to: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Ran {
    pub start: Vec<Span>,
    pub actions: Vec<(String, Vec<Span>)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    Person,
    Element,
    Browser,
    Db,
    Api,
    File,
}

impl Kind {
    const ALL: [Kind; 6] = [
        Kind::Person,
        Kind::Element,
        Kind::Browser,
        Kind::Db,
        Kind::Api,
        Kind::File,
    ];

    fn parse(s: &str) -> Result<Kind, String> {
        match s {
            "person" => Ok(Kind::Person),
            "element" => Ok(Kind::Element),
            "browser" => Ok(Kind::Browser),
            "db" => Ok(Kind::Db),
            "api" => Ok(Kind::Api),
            "file" => Ok(Kind::File),
            other => Err(format!(
                "`{other}` is not a kind; a kind is any of {}",
                list(&Kind::ALL.map(|k| format!("`{k}`")), "and")
            )),
        }
    }
}

impl fmt::Display for Kind {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(match self {
            Kind::Person => "person",
            Kind::Element => "element",
            Kind::Browser => "browser",
            Kind::Db => "db",
            Kind::Api => "api",
            Kind::File => "file",
        })
    }
}

#[derive(Debug, Clone)]
pub struct Object {
    pub name: String,
    pub kind: Kind,
    pub icon: String,
    pub line: usize,
    pub start: Pairs,
    pub rules: Vec<Rule>,
    pub invariants: Vec<Invariant>,
    pub draws: Vec<Draw>,
    pub pools: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct Step {
    pub from: String,
    pub to: String,
    pub verb: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Next {
    Value(Json),
    Input(Vec<String>),
    Before(String, String),
    After(String, String),
    Start(String, String),
}

#[derive(Debug, Clone)]
pub struct Rule {
    pub verb: String,
    pub when: Pairs,
    pub next: Vec<(String, Next)>,
    pub line: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Now {
    Key(String, String),
    Start(String, String),
    Value(Json),
    App(Op, Vec<Now>),
}

#[derive(Debug, Clone)]
pub struct Invariant {
    pub name: String,
    pub body: Now,
    pub line: usize,
}

#[derive(Clone, Copy)]
pub struct Seen<'a> {
    pub start: &'a State,
    pub before: &'a State,
    pub after: &'a State,
    pub input: &'a Json,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    And,
    Or,
    Not,
    Implies,
    Iff,
    Eq,
    Neq,
    In,
    Ilt,
    Ilte,
    Igt,
    Igte,
    Iadd,
    Isub,
    Imul,
    Idiv,
    Imod,
    Ipow,
    Iuminus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error(pub String);

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

fn list(names: &[String], word: &str) -> String {
    match names.split_last() {
        Some((last, rest)) if !rest.is_empty() => format!("{} {word} {last}", rest.join(", ")),
        _ => names.concat(),
    }
}

impl Op {
    const ALL: [Op; 19] = [
        Op::And,
        Op::Or,
        Op::Not,
        Op::Implies,
        Op::Iff,
        Op::Eq,
        Op::Neq,
        Op::In,
        Op::Ilt,
        Op::Ilte,
        Op::Igt,
        Op::Igte,
        Op::Iadd,
        Op::Isub,
        Op::Imul,
        Op::Idiv,
        Op::Imod,
        Op::Ipow,
        Op::Iuminus,
    ];

    fn name(self) -> &'static str {
        match self {
            Op::And => "and",
            Op::Or => "or",
            Op::Not => "not",
            Op::Implies => "implies",
            Op::Iff => "iff",
            Op::Eq => "eq",
            Op::Neq => "neq",
            Op::In => "in",
            Op::Ilt => "ilt",
            Op::Ilte => "ilte",
            Op::Igt => "igt",
            Op::Igte => "igte",
            Op::Iadd => "iadd",
            Op::Isub => "isub",
            Op::Imul => "imul",
            Op::Idiv => "idiv",
            Op::Imod => "imod",
            Op::Ipow => "ipow",
            Op::Iuminus => "iuminus",
        }
    }

    fn parse(s: &str) -> Option<Op> {
        Op::ALL.into_iter().find(|o| o.name() == s)
    }

    fn arity(self) -> (usize, usize) {
        match self {
            Op::And | Op::Or | Op::In => (2, usize::MAX),
            Op::Not | Op::Iuminus => (1, 1),
            _ => (2, 2),
        }
    }

    fn takes(self) -> &'static str {
        match self {
            Op::And | Op::Or => "two or more conditions",
            Op::Not => "one condition",
            Op::Implies | Op::Iff => "two conditions",
            Op::Eq | Op::Neq => "two terms of one type",
            Op::In => "a term and one or more terms of its type",
            Op::Iuminus => "one integer",
            _ => "two integers",
        }
    }

    fn compares(self) -> bool {
        matches!(
            self,
            Op::Eq | Op::Neq | Op::In | Op::Ilt | Op::Ilte | Op::Igt | Op::Igte
        )
    }

    fn boolean(self) -> bool {
        !matches!(
            self,
            Op::Iadd | Op::Isub | Op::Imul | Op::Idiv | Op::Imod | Op::Ipow | Op::Iuminus
        )
    }
}

impl Op {
    fn apply(self, args: &[Json]) -> Option<Json> {
        let ints: Option<Vec<i64>> = args.iter().map(Json::as_i64).collect();
        match (self, args) {
            (Op::Not, [Json::Bool(a)]) => Some(json!(!a)),
            (Op::Iff, [Json::Bool(a), Json::Bool(b)]) => Some(json!(a == b)),
            (Op::Eq, [a, b]) => Some(json!(a == b)),
            (Op::Neq, [a, b]) => Some(json!(a != b)),
            (Op::In, [a, rest @ ..]) => Some(json!(rest.contains(a))),
            _ => self.compute(&ints?),
        }
    }

    fn compute(self, n: &[i64]) -> Option<Json> {
        match (self, n) {
            (Op::Ilt, [a, b]) => Some(json!(a < b)),
            (Op::Ilte, [a, b]) => Some(json!(a <= b)),
            (Op::Igt, [a, b]) => Some(json!(a > b)),
            (Op::Igte, [a, b]) => Some(json!(a >= b)),
            (Op::Iadd, [a, b]) => a.checked_add(*b).map(Json::from),
            (Op::Isub, [a, b]) => a.checked_sub(*b).map(Json::from),
            (Op::Imul, [a, b]) => a.checked_mul(*b).map(Json::from),
            (Op::Idiv, [a, b]) => a.checked_div(*b).map(Json::from),
            (Op::Imod, [a, b]) => a.checked_rem(*b).map(Json::from),
            (Op::Ipow, [a, b]) if (*a, *b) != (0, 0) => u32::try_from(*b)
                .ok()
                .and_then(|e| a.checked_pow(e))
                .map(Json::from),
            (Op::Iuminus, [a]) => a.checked_neg().map(Json::from),
            _ => None,
        }
    }
}

fn read<'a>(s: &'a State, o: &str, k: &str) -> Option<&'a Json> {
    s.get(o)?.get(k)
}

fn found(v: Option<&Json>) -> String {
    v.map_or("no value".into(), Json::to_string)
}

fn show(v: &Json) -> String {
    match v {
        Json::String(s) if !s.is_empty() => s.clone(),
        other => other.to_string(),
    }
}

fn joined(p: &Pairs, pair: impl Fn(&str, &Json) -> String, by: &str) -> String {
    p.iter()
        .map(|(k, v)| pair(k, v))
        .collect::<Vec<_>>()
        .join(by)
}

pub fn shown(p: &Pairs) -> String {
    joined(p, |k, v| format!("{k} = {}", show(v)), ", ")
}

fn written(p: &Pairs) -> String {
    joined(p, |k, v| format!("{k}={v}"), " ")
}

impl fmt::Display for Next {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Next::Value(v) => write!(f, "{v}"),
            Next::Input(path) => write!(f, "(input)\"{}\"", path.join(".")),
            Next::Before(o, k) => write!(f, "\"{o}.{k}\""),
            Next::After(o, k) => write!(f, "(next)\"{o}.{k}\""),
            Next::Start(o, k) => write!(f, "(start)\"{o}.{k}\""),
        }
    }
}

impl Next {
    fn eval(&self, c: &Seen) -> Option<Json> {
        match self {
            Next::Value(v) => Some(v.clone()),
            Next::Input(path) => path
                .iter()
                .try_fold(c.input, |node, part| node.get(part))
                .cloned(),
            Next::Before(o, k) => read(c.before, o, k).cloned(),
            Next::After(o, k) => read(c.after, o, k).cloned(),
            Next::Start(o, k) => read(c.start, o, k).cloned(),
        }
    }

    fn key(&self) -> Option<(&String, &String)> {
        match self {
            Next::Before(o, k) | Next::After(o, k) | Next::Start(o, k) => Some((o, k)),
            Next::Value(_) | Next::Input(_) => None,
        }
    }
}

impl Rule {
    pub fn source(&self) -> String {
        let terms: String = self
            .next
            .iter()
            .map(|(k, term)| format!(" {k}={term}"))
            .collect();
        match self.when.is_empty() {
            true => format!("on \"{}\"{terms}", self.verb),
            false => format!(
                "state {} {{ on \"{}\"{terms} }}",
                written(&self.when),
                self.verb
            ),
        }
    }

    fn applies(&self, o: &str, verb: &str, before: &State) -> bool {
        self.verb == verb && self.when.iter().all(|(k, v)| read(before, o, k) == Some(v))
    }

    fn differs(&self, o: &str, error: Option<&str>, c: &Seen) -> Option<String> {
        let keys: BTreeSet<&String> = c
            .after
            .get(o)
            .into_iter()
            .flat_map(Pairs::keys)
            .chain(self.next.iter().map(|(k, _)| k))
            .collect();
        keys.into_iter().find_map(|k| {
            let want = match self.next.iter().find(|(key, _)| key == k) {
                Some((_, term)) => term.eval(c),
                None if error == Some(k.as_str()) => Some(json!("")),
                None => read(c.before, o, k).cloned(),
            };
            let code = read(c.after, o, k);
            (want.as_ref() != code)
                .then(|| format!("{o}.{k} = {}, not {}", found(code), found(want.as_ref())))
        })
    }
}

impl Now {
    fn leaf(&self) -> Option<String> {
        match self {
            Now::Key(o, k) => Some(format!("{o}.{k}")),
            Now::Start(o, k) => Some(format!("start({o}.{k})")),
            Now::Value(v) => Some(v.to_string()),
            Now::App(..) => None,
        }
    }

    fn show(&self, part: &dyn Fn(&Now) -> Option<String>) -> String {
        let each = |args: &[Now]| {
            args.iter()
                .map(|e| e.show(part))
                .collect::<Vec<_>>()
                .join(", ")
        };
        match (part(self), self) {
            (Some(s), _) => s,
            (None, Now::App(Op::In, args)) => {
                format!("in({}, Set({}))", args[0].show(part), each(&args[1..]))
            }
            (None, Now::App(op, args)) => format!("{}({})", op.name(), each(args)),
            (None, _) => String::new(),
        }
    }

    pub fn quint(&self) -> String {
        self.show(&Now::leaf)
    }

    fn valued(&self, start: &State, now: &State) -> String {
        self.show(&|e| match e {
            Now::App(op, _) if op.boolean() => None,
            e => Some(found(e.eval(start, now).as_ref())),
        })
    }

    fn eval(&self, start: &State, now: &State) -> Option<Json> {
        let first = |all: &[Now], stop: bool| {
            all.iter()
                .map(|e| e.eval(start, now))
                .find(|t| *t != Some(json!(!stop)))
                .unwrap_or(Some(json!(!stop)))
                .filter(Json::is_boolean)
        };
        match self {
            Now::Key(o, k) => read(now, o, k).cloned(),
            Now::Start(o, k) => read(start, o, k).cloned(),
            Now::Value(v) => Some(v.clone()),
            Now::App(Op::And, all) => first(all, false),
            Now::App(Op::Or, all) => first(all, true),
            Now::App(Op::Implies, all) => match all[0].eval(start, now)? {
                Json::Bool(false) => Some(json!(true)),
                Json::Bool(true) => all[1].eval(start, now).filter(Json::is_boolean),
                _ => None,
            },
            Now::App(op, all) => op.apply(
                &all.iter()
                    .map(|e| e.eval(start, now))
                    .collect::<Option<Vec<_>>>()?,
            ),
        }
    }

    fn passes(&self, start: &State, now: &State) -> bool {
        self.eval(start, now) == Some(json!(true))
    }

    fn evidence(&self, start: &State, now: &State) -> Vec<String> {
        match self {
            Now::App(Op::And, all) => all
                .iter()
                .find(|e| !e.passes(start, now))
                .map_or_else(Vec::new, |e| e.evidence(start, now)),
            Now::App(Op::Implies, all) => [
                vec![format!("{} = true", all[0].valued(start, now))],
                all[1].evidence(start, now),
            ]
            .concat(),
            e => vec![format!("{} = {} = false", e.quint(), e.valued(start, now))],
        }
    }

    fn all(&self) -> Vec<&Now> {
        match self {
            Now::App(_, args) => [vec![self], args.iter().flat_map(Now::all).collect()].concat(),
            e => vec![e],
        }
    }

    fn keys(&self) -> Vec<(&String, &String)> {
        self.all()
            .into_iter()
            .filter_map(|e| match e {
                Now::Key(o, k) | Now::Start(o, k) => Some((o, k)),
                _ => None,
            })
            .collect()
    }
}

impl Invariant {
    pub fn source(&self) -> String {
        format!("invariant \"{}\" {{ {} }}", self.name, self.body.quint())
    }

    pub fn judge(&self, start: &State, now: &State) -> Option<Vec<String>> {
        (!self.body.passes(start, now))
            .then(|| [vec![self.source()], self.body.evidence(start, now)].concat())
    }
}

impl Object {
    fn rules(&self, verb: &str, before: &State) -> Vec<&Rule> {
        let all: Vec<&Rule> = self
            .rules
            .iter()
            .filter(|r| r.applies(&self.name, verb, before))
            .collect();
        match all.iter().any(|r| !r.when.is_empty()) {
            true => all.into_iter().filter(|r| !r.when.is_empty()).collect(),
            false => all,
        }
    }

    fn keys(&self) -> Vec<(&String, &String)> {
        let o = &self.name;
        self.start
            .keys()
            .chain(
                self.rules
                    .iter()
                    .flat_map(|r| r.when.keys().chain(r.next.iter().map(|(k, _)| k))),
            )
            .map(|k| (o, k))
            .chain(
                self.rules
                    .iter()
                    .flat_map(|r| r.next.iter().filter_map(|(_, term)| term.key())),
            )
            .chain(self.invariants.iter().flat_map(|i| i.body.keys()))
            .collect()
    }
}

impl Diagram {
    fn object(&self, name: &str) -> Option<&Object> {
        self.objects.iter().find(|o| o.name == name)
    }

    pub fn initial(&self, name: &str) -> Option<String> {
        self.object(name)
            .filter(|o| !o.start.is_empty())
            .map(|o| shown(&o.start))
    }

    pub fn verbs(&self) -> Vec<(&str, &str)> {
        let mut out: Vec<(&str, &str)> = Vec::new();
        for o in &self.objects {
            for r in &o.rules {
                if !out.contains(&(o.name.as_str(), r.verb.as_str())) {
                    out.push((&o.name, &r.verb));
                }
            }
        }
        out
    }

    pub fn rules(&self, object: &str, verb: &str) -> Vec<&Rule> {
        self.object(object)
            .into_iter()
            .flat_map(|o| &o.rules)
            .filter(|r| r.verb == verb)
            .collect()
    }

    pub fn invariants(&self) -> Vec<&Invariant> {
        self.objects.iter().flat_map(|o| &o.invariants).collect()
    }

    pub fn keys(&self) -> Vec<(&String, &String)> {
        let mut out: Vec<(&String, &String)> = Vec::new();
        for key in self.objects.iter().flat_map(Object::keys) {
            if !out.contains(&key) {
                out.push(key);
            }
        }
        out
    }

    pub fn enabled(&self, object: &str, verb: &str, before: &State) -> bool {
        self.object(object)
            .is_some_and(|o| !o.rules(verb, before).is_empty())
    }

    pub fn started(&self, real: &State) -> Option<Vec<String>> {
        self.objects.iter().find_map(|o| {
            o.start.iter().find_map(|(k, v)| {
                let code = read(real, &o.name, k);
                (code != Some(v)).then(|| {
                    vec![
                        format!("{} \"{}\" {}", o.kind, o.name, written(&o.start)),
                        format!("{}.{k} = {}, not {v}", o.name, found(code)),
                    ]
                })
            })
        })
    }

    pub fn stepped(
        &self,
        object: &str,
        verb: &str,
        error: Option<&str>,
        c: &Seen,
    ) -> Option<Vec<String>> {
        let rules = self.object(object)?.rules(verb, c.before);
        let names = |r: &Rule| r.next.iter().any(|(k, _)| error == Some(k.as_str()));
        let thrown = error
            .filter(|k| read(c.after, object, k).is_some_and(|v| v != ""))
            .filter(|_| !rules.iter().any(|r| names(r)))
            .map(|k| Rule {
                verb: verb.to_string(),
                when: Pairs::new(),
                next: vec![(
                    k.to_string(),
                    Next::After(object.to_string(), k.to_string()),
                )],
                line: 0,
            });
        let differs: Vec<(&Rule, Option<String>)> = match &thrown {
            Some(rule) => vec![rule],
            None => rules,
        }
        .into_iter()
        .map(|r| (r, r.differs(object, error, c)))
        .collect();
        match differs.iter().any(|(_, d)| d.is_none()) {
            true => None,
            false => Some(
                differs
                    .into_iter()
                    .flat_map(|(r, d)| [r.source(), d.unwrap_or_default()])
                    .collect(),
            ),
        }
    }
}

fn name(n: &KdlNode) -> &str {
    n.name().value()
}

fn text(v: &KdlValue) -> String {
    match v {
        KdlValue::String(s) => s.clone(),
        other => other.to_string(),
    }
}

fn args(n: &KdlNode) -> Vec<String> {
    n.entries()
        .iter()
        .filter(|e| e.name().is_none())
        .map(|e| text(e.value()))
        .collect()
}

fn keys(n: &KdlNode) -> Vec<&str> {
    let mut out: Vec<&str> = Vec::new();
    for k in n.entries().iter().filter_map(|e| e.name()) {
        if !out.contains(&k.value()) {
            out.push(k.value());
        }
    }
    out
}

fn value(v: &KdlValue) -> Json {
    match v {
        KdlValue::Integer(n) => json!(*n as i64),
        other => json!(text(other)),
    }
}

fn pairs(n: &KdlNode) -> Result<Pairs, String> {
    let mut out = Pairs::new();
    for e in n.entries() {
        if let Some(k) = e.name()
            && out
                .insert(k.value().to_string(), value(e.value()))
                .is_some()
        {
            return Err(format!("`{}` sets `{}` twice", name(n), k.value()));
        }
    }
    Ok(out)
}

fn shape(n: &KdlNode, count: usize, allowed: &[&str], block: bool) -> Result<Vec<String>, String> {
    if let Some(e) = n
        .entries()
        .iter()
        .find(|e| e.ty().is_some() || matches!((e.name(), e.value()), (None, KdlValue::Integer(_))))
    {
        return Err(format!(
            "`{}` takes strings, and integers as values of keys, not `{}`",
            name(n),
            e.to_string().trim()
        ));
    }
    if let Some(k) = keys(n).into_iter().find(|k| {
        n.entries()
            .iter()
            .filter(|e| e.name().is_some_and(|key| key.value() == *k))
            .count()
            > 1
    }) {
        return Err(format!("`{}` sets `{k}` twice", name(n)));
    }
    let given = args(n);
    if given.len() != count {
        return Err(format!(
            "`{}` takes {count} names, not {}",
            name(n),
            given.len()
        ));
    }
    if let Some(k) = keys(n).into_iter().find(|k| !allowed.contains(k)) {
        return Err(format!("`{}` takes no `{k}`", name(n)));
    }
    if !block && n.children().is_some() {
        return Err(format!("`{}` has no lines", name(n)));
    }
    Ok(given)
}

fn need(n: &KdlNode, k: &str) -> Result<String, String> {
    n.get(k)
        .map(text)
        .ok_or_else(|| format!("`{}` needs `{k}`", name(n)))
}

fn lines(n: &KdlNode) -> &[KdlNode] {
    n.children().map_or(&[], KdlDocument::nodes)
}

fn line(src: &str, n: &KdlNode) -> usize {
    src[..n.span().offset()].matches('\n').count() + 1
}

const DRAW: &str = "`draw` has `<key> <value>…` lines; a line with `{ }` has the lines of the keys of its part, and no values";

fn draw(n: &KdlNode, path: &[String], out: &mut Vec<Draw>) -> Result<(), String> {
    if n.entries()
        .iter()
        .any(|e| e.name().is_some() || e.ty().is_some())
    {
        return Err(DRAW.into());
    }
    let values: Vec<Json> = n.entries().iter().map(|e| value(e.value())).collect();
    let keys: Vec<String> = lines(n).iter().map(|c| name(c).to_string()).collect();
    let at = |key: &str| [path, &[key.to_string()]].concat().join(".");
    if let Some(key) = keys
        .iter()
        .enumerate()
        .find_map(|(i, key)| keys[..i].contains(key).then_some(key))
    {
        return Err(format!("`draw` sets `{}` twice", at(key)));
    }
    if let Some(value) = values
        .iter()
        .enumerate()
        .find_map(|(i, value)| values[..i].contains(value).then_some(value))
    {
        return Err(format!(
            "`{}` of `draw` has `{}` twice",
            path.join("."),
            value
        ));
    }
    match (path.is_empty(), values.is_empty(), keys.is_empty()) {
        (true, true, false) => {}
        (false, true, false) => out.push(Draw {
            path: path.to_vec(),
            values: keys.iter().map(|key| json!(key)).collect(),
            keys: true,
        }),
        (false, _, true) if n.children().is_none() => out.push(Draw {
            path: path.to_vec(),
            values,
            keys: false,
        }),
        _ => return Err(DRAW.into()),
    }
    lines(n)
        .iter()
        .try_for_each(|c| draw(c, &[path, &[name(c).to_string()]].concat(), out))
}

fn named(n: &KdlNode, text: &str) -> Result<Name, String> {
    match text.split_once('.') {
        Some((module, export)) if !module.is_empty() && !export.is_empty() => Ok(Name {
            module: module.to_string(),
            export: export.to_string(),
        }),
        _ => Err(format!(
            "`{}` takes `\"<module>.<export>\"`, not `{text}`",
            name(n)
        )),
    }
}

fn used(n: &KdlNode) -> Result<Use, String> {
    let (role, allowed): (Role, &[&str]) = match name(n) {
        "start" => (Role::Start, &["input", "tag", "error", "pool"]),
        "read" => (Role::Read, &["output"]),
        "run" => (
            Role::Run,
            &[
                "input", "output", "error", "verb", "pool", "from", "consume",
            ],
        ),
        other => {
            return Err(format!(
                "`impl` has `module`, `start`, `read` and `run`, not `{other}`"
            ));
        }
    };
    let function = shape(n, 1, allowed, false)?.remove(0);
    let given = |key: &str| n.get(key).map(text);
    let schema = |key: &str| given(key).map(|s| named(n, &s)).transpose();
    let from = match (given("from"), given("consume")) {
        (Some(_), Some(_)) => Err("has `from` and `consume`, and a line takes one of them"),
        (Some(pool), None) => Ok(Some((pool, false))),
        (None, Some(pool)) => Ok(Some((pool, true))),
        (None, None) => Ok(None),
    };
    let wrong = match (
        role,
        given("input"),
        given("output"),
        given("tag"),
        given("verb"),
    ) {
        (_, None, _, Some(_), _) => Err("has a `tag` and no `input`"),
        (Role::Read, _, None, ..) => Err("needs `output`"),
        (Role::Run, None, _, _, None) => Err("needs `input` or `verb`"),
        _ => from,
    };
    let from = wrong.map_err(|wrong| format!("`{}` {wrong}", name(n)))?;
    let function = match &from {
        None => named(n, &function)?,
        Some((pool, _)) => match function.split_once('.').unwrap_or((&function, "")) {
            (head, method) if head == pool => Name {
                module: pool.clone(),
                export: method.to_string(),
            },
            _ => {
                return Err(format!(
                    "`{} \"{function}\"` draws from the pool `{pool}`, and takes `\"{pool}\"` or `\"{pool}.<method>\"`",
                    name(n)
                ));
            }
        },
    };
    Ok(Use {
        role,
        function,
        input: schema("input")?,
        output: schema("output")?,
        error: schema("error")?,
        tag: given("tag"),
        verb: given("verb"),
        pool: given("pool"),
        from,
    })
}

fn implemented(n: &KdlNode) -> Result<Impl, String> {
    let mut found = Impl {
        adapter: shape(n, 1, &[], true)?.remove(0),
        modules: Vec::new(),
        uses: Vec::new(),
    };
    for c in lines(n) {
        match name(c) {
            "module" => {
                let mut given = shape(c, 2, &[], false)?;
                let (place, module) = (given.remove(1), given.remove(0));
                if found.modules.iter().any(|(known, _)| *known == module) {
                    return Err(format!("`module \"{module}\"` is set twice"));
                }
                if place.starts_with('.') && !place.starts_with("../") {
                    return Err(format!(
                        "`module \"{module}\"` is a file in the directory of the diagram, and a module is code of the app"
                    ));
                }
                found.modules.push((module, place));
            }
            _ => found.uses.push(used(c)?),
        }
    }
    let names: Vec<&Name> = found
        .uses
        .iter()
        .flat_map(|u| {
            [
                u.from.is_none().then_some(&u.function),
                u.input.as_ref(),
                u.output.as_ref(),
                u.error.as_ref(),
            ]
        })
        .flatten()
        .collect();
    if let Some(unknown) = names
        .iter()
        .find(|n| !found.modules.iter().any(|(module, _)| *module == n.module))
    {
        return Err(format!(
            "`{unknown}` names `{}`, and `impl \"{}\"` has no `module \"{}\"`",
            unknown.module, found.adapter, unknown.module
        ));
    }
    if let Some((module, _)) = found
        .modules
        .iter()
        .find(|(module, _)| !names.iter().any(|n| n.module == *module))
    {
        return Err(format!(
            "no line of `impl \"{}\"` names `module \"{module}\"`",
            found.adapter
        ));
    }
    match found.uses.iter().any(|u| u.role != Role::Start) {
        true => Ok(found),
        false => Err(format!(
            "`impl \"{}\"` has no `read` line and no `run` line",
            found.adapter
        )),
    }
}

const ON: &str = "`on \"<verb>\" <key>=<term>…` has the value of each key after the step; a term is a value, `(input)\"<path>\"`, `\"<object>.<key>\"`, `(next)\"<object>.<key>\"` or `(start)\"<object>.<key>\"`, and a key with no term has its value from before the step";

const INVARIANT: &str = "`invariant \"<name>\" { <condition> }` has one condition on one state; a condition is a Quint builtin `<op> <term>… { <op line>… }`: `and`, `or`, `not`, `implies`, `iff`, `eq`, `neq`, `in`, `ilt`, `ilte`, `igt`, `igte`, `iadd`, `isub`, `imul`, `idiv`, `imod`, `ipow` or `iuminus`; a term is `\"<object>.<key>\"`, `(start)\"<object>.<key>\"` or a value, and each comparison names at least one `\"<object>.<key>\"`";

fn refer(names: &[String], s: &str) -> Option<(String, String)> {
    let (o, k) = s.split_once('.')?;
    (names.iter().any(|n| n == o) && !k.is_empty() && !k.contains('.'))
        .then(|| (o.to_string(), k.to_string()))
}

fn next(e: &KdlEntry, names: &[String]) -> Result<Next, String> {
    let named = |s: &str, to: fn(String, String) -> Next| {
        refer(names, s).map(|(o, k)| to(o, k)).ok_or_else(|| {
            format!(
                "`{}` names `\"<object>.<key>\"`: {ON}",
                e.to_string().trim()
            )
        })
    };
    match (e.ty().map(|t| t.value()), e.value()) {
        (None, KdlValue::String(s)) => {
            Ok(named(s, Next::Before).unwrap_or_else(|_| Next::Value(json!(s))))
        }
        (None, v @ KdlValue::Integer(_)) => Ok(Next::Value(value(v))),
        (Some("input"), KdlValue::String(s)) if !s.is_empty() => {
            Ok(Next::Input(s.split('.').map(String::from).collect()))
        }
        (Some("next"), KdlValue::String(s)) => named(s, Next::After),
        (Some("start"), KdlValue::String(s)) => named(s, Next::Start),
        _ => Err(format!("`{}` is not a term: {ON}", e.to_string().trim())),
    }
}

fn on(n: &KdlNode, when: &Pairs, names: &[String], src: &str) -> Result<Rule, String> {
    let verb = match n
        .entries()
        .iter()
        .filter(|e| e.name().is_none())
        .map(|e| (e.ty(), e.value()))
        .collect::<Vec<_>>()
        .as_slice()
    {
        [(None, KdlValue::String(verb))] if n.children().is_none() => verb.clone(),
        _ => return Err(ON.into()),
    };
    let mut terms: Vec<(String, Next)> = Vec::new();
    for (key, e) in n
        .entries()
        .iter()
        .filter_map(|e| e.name().map(|key| (key.value(), e)))
    {
        if terms.iter().any(|(k, _)| k == key) {
            return Err(format!("`on \"{verb}\"` sets `{key}` twice"));
        }
        terms.push((key.to_string(), next(e, names)?));
    }
    Ok(Rule {
        verb,
        when: when.clone(),
        next: terms,
        line: line(src, n),
    })
}

fn state(n: &KdlNode, o: &mut Object, names: &[String], src: &str) -> Result<(), String> {
    shape(n, 0, &keys(n), true)?;
    let when = pairs(n)?;
    if when.is_empty() || lines(n).is_empty() {
        return Err("`state <key>=<value>… { on … }` has the `on` lines of one state".into());
    }
    for c in lines(n) {
        match name(c) {
            "on" => o.rules.push(on(c, &when, names, src)?),
            other => {
                return Err(format!(
                    "`state {}` has `on`, not `{other}`",
                    written(&when)
                ));
            }
        }
    }
    Ok(())
}

fn term(e: &KdlEntry, names: &[String]) -> Result<Now, String> {
    match (e.name(), e.ty().map(|t| t.value()), e.value()) {
        (None, None, KdlValue::String(s)) => {
            Ok(refer(names, s).map_or_else(|| Now::Value(json!(s)), |(o, k)| Now::Key(o, k)))
        }
        (None, Some("start"), KdlValue::String(s)) => refer(names, s)
            .map(|(o, k)| Now::Start(o, k))
            .ok_or_else(|| format!("`(start)` takes `\"<object>.<key>\"`, not `{s}`: {INVARIANT}")),
        (None, None, v @ KdlValue::Integer(_)) => Ok(Now::Value(value(v))),
        _ => Err(format!(
            "`{}` is not a term: {INVARIANT}",
            e.to_string().trim()
        )),
    }
}

fn condition(n: &KdlNode, names: &[String]) -> Result<Now, String> {
    let op =
        Op::parse(name(n)).ok_or_else(|| format!("`{}` is not a builtin: {INVARIANT}", name(n)))?;
    let args = n
        .entries()
        .iter()
        .map(|e| term(e, names))
        .chain(lines(n).iter().map(|c| condition(c, names)))
        .collect::<Result<Vec<_>, _>>()?;
    let (min, max) = op.arity();
    let app = Now::App(op, args);
    match &app {
        Now::App(_, args) if args.len() < min || args.len() > max => {
            Err(format!("`{}` takes {}: {INVARIANT}", op.name(), op.takes()))
        }
        _ if op.compares() && app.keys().is_empty() => Err(format!(
            "`{}` names at least one `\"<object>.<key>\"`: {INVARIANT}",
            app.quint()
        )),
        _ => Ok(app),
    }
}

fn invariant(n: &KdlNode, names: &[String], src: &str) -> Result<Invariant, String> {
    let named = shape(n, 1, &[], true)?.remove(0);
    let body = match lines(n) {
        [one] => condition(one, names)?,
        _ => return Err(INVARIANT.into()),
    };
    match &body {
        Now::App(op, _) if op.boolean() => Ok(Invariant {
            name: named,
            body,
            line: line(src, n),
        }),
        _ => Err(format!(
            "`invariant \"{named}\"` has a condition, not `{}`: {INVARIANT}",
            body.quint()
        )),
    }
}

fn object(n: &KdlNode, config: &Config, names: &[String], src: &str) -> Result<Object, String> {
    let kind = Kind::parse(name(n))?;
    let icon = config
        .styles
        .iter()
        .find(|(k, _)| *k == kind)
        .map(|(_, i)| i.clone())
        .ok_or_else(|| format!("`{kind}` has no `style \"{kind}\"` in the config"))?;
    let named = shape(n, 1, &keys(n), true)
        .map_err(|e| format!("{kind}: {e}"))?
        .remove(0);
    let at = |e: String| format!("{kind} {named}: {e}");
    let mut o = Object {
        name: named.clone(),
        kind,
        icon,
        line: line(src, n),
        start: pairs(n).map_err(at)?,
        rules: Vec::new(),
        invariants: Vec::new(),
        draws: Vec::new(),
        pools: Vec::new(),
    };
    for c in n.iter_children() {
        match name(c) {
            "on" => on(c, &Pairs::new(), names, src).map(|rule| o.rules.push(rule)),
            "state" => state(c, &mut o, names, src),
            "invariant" => invariant(c, names, src).map(|found| o.invariants.push(found)),
            "draw" if !o.draws.is_empty() => Err("has one `draw`".into()),
            "draw" => draw(c, &[], &mut o.draws),
            "pool" => shape(c, 1, &[], false).map(|mut given| o.pools.push(given.remove(0))),
            other => Err(format!(
                "has `pool`, `draw`, `on`, `state` and `invariant`, not `{other}`"
            )),
        }
        .map_err(at)?;
    }
    if let Some(pool) = o
        .pools
        .iter()
        .find(|pool| o.start.get(*pool).is_some_and(|v| !v.is_i64()))
    {
        return Err(at(format!(
            "`{pool}` is the count of the values of `pool \"{pool}\"`, and a count is an integer"
        )));
    }
    Ok(o)
}

fn step(n: &KdlNode) -> Result<Step, String> {
    shape(n, 0, &["from", "to"], false)?;
    Ok(Step {
        from: need(n, "from")?,
        to: need(n, "to")?,
        verb: name(n).to_string(),
    })
}

fn build(doc: &KdlDocument, src: &str, config: &Config) -> Result<Diagram, String> {
    let mut d = Diagram::default();
    let mut seen = HashSet::new();
    let names: Vec<String> = doc
        .nodes()
        .iter()
        .filter(|n| name(n) == "setup")
        .flat_map(lines)
        .filter_map(|c| args(c).into_iter().next())
        .collect();
    for n in doc.nodes() {
        let parent = name(n);
        if parent == "impl" {
            let found = implemented(n)?;
            if d.impls.iter().any(|i| i.adapter == found.adapter) {
                return Err(format!("`impl \"{}\"` is set twice", found.adapter));
            }
            d.impls.push(found);
            continue;
        }
        if !matches!(parent, "setup" | "action") {
            return Err(format!("`{parent}` is not setup, action or impl"));
        }
        shape(n, 0, &[], true)?;
        if !seen.insert(parent) {
            return Err(format!("`{parent}` is set twice"));
        }
        for c in n.iter_children() {
            match parent {
                "setup" => d.objects.push(object(c, config, &names, src)?),
                _ => d.path.push(step(c)?),
            }
        }
    }
    if d.objects.is_empty() {
        return Err("no objects".into());
    }
    let mut known = HashSet::new();
    if let Some(o) = d.objects.iter().find(|o| !known.insert(o.name.as_str())) {
        return Err(format!("`{}` is declared twice", o.name));
    }
    let pools: Vec<&String> = d.objects.iter().flat_map(|o| &o.pools).collect();
    if let Some(pool) = pools
        .iter()
        .enumerate()
        .find_map(|(at, pool)| pools[..at].contains(pool).then_some(pool))
    {
        return Err(format!("`pool \"{pool}\"` is set twice"));
    }
    if let Some((u, pool)) = d.impls.iter().flat_map(|i| &i.uses).find_map(|u| {
        [u.pool.as_ref(), u.from.as_ref().map(|(pool, _)| pool)]
            .into_iter()
            .flatten()
            .find(|pool| !pools.contains(pool))
            .map(|pool| (u, pool))
    }) {
        return Err(format!(
            "{} names the pool `{pool}`, and no object of `setup` has `pool \"{pool}\"`",
            u.line()
        ));
    }
    let verbs = d.verbs();
    if let Some((u, verb)) = d.impls.iter().flat_map(|i| &i.uses).find_map(|u| {
        [u.tag.as_ref(), u.verb.as_ref()]
            .into_iter()
            .flatten()
            .find(|verb| !verbs.iter().any(|(_, v)| v == verb))
            .map(|verb| (u, verb))
    }) {
        return Err(format!(
            "{} names the verb `{verb}`, and no object of `setup` has `on \"{verb}\"`",
            u.line()
        ));
    }
    let invariants = d.invariants();
    if let Some(found) = invariants.iter().enumerate().find_map(|(at, i)| {
        invariants[..at]
            .iter()
            .any(|j| j.name == i.name)
            .then_some(i)
    }) {
        return Err(format!("`invariant \"{}\"` is set twice", found.name));
    }
    if verbs.is_empty() {
        return Err("`setup` has one `on \"<verb>\"` line or more".into());
    }
    if d.path.is_empty() {
        return Err(
            "`action` has one `<verb> from=\"<object>\" to=\"<object>\"` line or more".into(),
        );
    }
    for (k, s) in d.path.iter().enumerate() {
        let at = |e: String| format!("`action` line {}: {e}", k + 1);
        if let Some(unknown) = [&s.from, &s.to].into_iter().find(|x| d.object(x).is_none()) {
            return Err(at(format!("unknown object `{unknown}`")));
        }
        if !verbs.contains(&(s.to.as_str(), s.verb.as_str())) {
            return Err(at(format!("`{}` has no `on \"{}\"` line", s.to, s.verb)));
        }
    }
    Ok(d)
}

const FORM: &str = "a file of weft has the KDL that versions 1 and 2 share: the name of a line and a key are words with no `<`, `>` or `,`, a value is an integer or a string in `\"` with the escapes `\\n`, `\\r`, `\\t`, `\\\\`, `\\\"`, `\\b`, `\\f` and `\\u{…}`, a line ends at the end of its text line, and `\\` at the end of a text line continues the line";

fn form(doc: &KdlDocument) -> Result<(), String> {
    let word = |name: &kdl::KdlIdentifier| {
        !name.repr().is_some_and(|r| r.starts_with(['"', '#']))
            && !name.value().contains(['<', '>', ','])
    };
    let escaped = |repr: &str| {
        let mut chars = repr.chars();
        std::iter::from_fn(|| chars.find(|c| *c == '\\').and_then(|_| chars.next()))
            .all(|c| "nrt\\\"bfu".contains(c))
    };
    for n in doc.nodes() {
        if n.ty().is_some()
            || !word(n.name())
            || n.format().is_some_and(|f| f.terminator.starts_with(';'))
        {
            return Err(format!("`{}` is not a line of weft: {FORM}", name(n)));
        }
        for e in n.entries() {
            let repr = e.format().map_or("", |f| f.value_repr.as_str());
            let fits = match e.value() {
                KdlValue::String(_) => {
                    repr.starts_with('"') && !repr.starts_with("\"\"\"") && escaped(repr)
                }
                KdlValue::Integer(_) => {
                    repr.strip_prefix('-')
                        .unwrap_or(repr)
                        .bytes()
                        .all(|b| b.is_ascii_digit())
                        && repr.parse::<i64>().is_ok()
                }
                _ => false,
            };
            if !fits || !e.name().is_none_or(word) {
                return Err(format!(
                    "`{}` of `{}` is not a value of weft: {FORM}",
                    e.to_string().trim(),
                    name(n)
                ));
            }
        }
        n.children().map_or(Ok(()), form)?;
    }
    Ok(())
}

fn document(src: &str) -> Result<KdlDocument, Error> {
    let doc = parsed(src)?;
    form(&doc).map_err(Error)?;
    Ok(doc)
}

fn parsed(src: &str) -> Result<KdlDocument, Error> {
    KdlDocument::parse_v2(src).map_err(|e| {
        let found: Vec<String> = e
            .diagnostics
            .iter()
            .map(|d| {
                let line = d
                    .input
                    .as_str()
                    .read_span(&d.span, 0, 0)
                    .map_or(0, |c| c.line() + 1);
                match &d.help {
                    Some(h) => format!("line {line}: {d} ({h})"),
                    None => format!("line {line}: {d}"),
                }
            })
            .collect();
        Error(found.join("; "))
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Adapter {
    Default,
    Bun,
}

impl fmt::Display for Adapter {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(match self {
            Adapter::Default => "default",
            Adapter::Bun => "bun",
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Transport {
    Unix(String),
    Http(String),
}

impl fmt::Display for Transport {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Transport::Unix(path) => write!(f, "unix {path}"),
            Transport::Http(url) => write!(f, "http {url}"),
        }
    }
}

pub struct Config {
    pub include: Vec<String>,
    pub theme: String,
    pub styles: Vec<(Kind, String)>,
    pub adapter: Adapter,
    pub transport: Option<Transport>,
    pub cases: u64,
}

pub fn config(src: &str) -> Result<Config, Error> {
    let doc = document(src)?;
    let mut c = Config {
        include: Vec::new(),
        theme: String::new(),
        styles: Vec::new(),
        adapter: Adapter::Default,
        transport: None,
        cases: 100,
    };
    let mut set = false;
    for n in doc.nodes() {
        match name(n) {
            "include" => c.include.extend(shape(n, 1, &[], false).map_err(Error)?),
            "theme" => c.theme = shape(n, 1, &[], false).map_err(Error)?.remove(0),
            "style" => {
                let named = shape(n, 1, &["icon"], false).map_err(Error)?.remove(0);
                let kind = Kind::parse(&named).map_err(|e| Error(format!("`style`: {e}")))?;
                if c.styles.iter().any(|(k, _)| *k == kind) {
                    return Err(Error(format!("`style \"{kind}\"` is set twice")));
                }
                c.styles.push((kind, need(n, "icon").map_err(Error)?));
            }
            "transport" if c.transport.is_some() => {
                return Err(Error("`transport` is set twice".into()));
            }
            "transport" => {
                let (key, transport): (&str, fn(String) -> Transport) = match args(n)
                    .first()
                    .map(String::as_str)
                {
                    Some("unix") => ("path", Transport::Unix),
                    Some("http") => ("url", Transport::Http),
                    _ => {
                        return Err(Error(
                                "`transport` is `\"unix\" path=\"<socket>\"` or `\"http\" url=\"<url>\"`".into(),
                            ));
                    }
                };
                shape(n, 1, &[key], false).map_err(Error)?;
                c.transport = Some(transport(need(n, key).map_err(Error)?));
            }
            "adapter" if set => {
                return Err(Error("`adapter` is set twice".into()));
            }
            "adapter" => {
                let adapter = match args(n).first().map(String::as_str) {
                    Some("default") => Adapter::Default,
                    Some("bun") => Adapter::Bun,
                    _ => return Err(Error("`adapter` is `\"default\"` or `\"bun\"`".into())),
                };
                let name = shape(n, 1, &["cases"], false).map_err(Error)?.remove(0);
                let cases = match n.get("cases") {
                    Some(KdlValue::Integer(k)) => u64::try_from(*k).ok().filter(|k| *k > 0),
                    _ => None,
                }
                .ok_or_else(|| {
                    Error(format!(
                        "`adapter \"{name}\"` takes `cases=<count>`, a count above 0"
                    ))
                })?;
                c.adapter = adapter;
                c.cases = cases;
                set = true;
            }
            other => {
                return Err(Error(format!(
                    "`{other}` is not include, theme, style, adapter or transport"
                )));
            }
        }
    }
    match (c.include.is_empty(), c.theme.is_empty()) {
        (true, _) => Err(Error("needs `include \"<glob>\"`".into())),
        (_, true) => Err(Error("needs `theme \"light\"` or `theme \"dark\"`".into())),
        _ => Ok(c),
    }
}

pub fn parse(src: &str, config: &Config) -> Result<Diagram, Error> {
    build(&document(src)?, src, config).map_err(Error)
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONFIG: &str = r#"include "**/*.weft.kdl"
theme "light"
style "person" icon="user"
style "browser" icon="app-window"
style "element" icon="square-dashed-mouse-pointer""#;

    const BASE: &str = r#"
setup {
  person "user"
  browser "history" snapshot="snap_0" {
    on "leave" snapshot="snap_1"
    state snapshot="snap_1" {
      on "back" snapshot="snap_0"
    }
  }
  element "origin" value="San Francisco" {
    on "type" value=(input)"text"
    invariant "filled" {
      neq "origin.value" ""
    }
  }
}
action {
  type from="user" to="origin"
  leave from="user" to="history"
  back from="user" to="history"
}
"#;

    const POOL: &str = r#"
setup {
  person "user"
  browser "page" count=0 opened=0 {
    pool "opened"
    on "open" count=(next)"page.count" opened=(next)"page.opened"
    on "close" count=(next)"page.count" opened=(next)"page.opened"
    invariant "closed" {
      implies {
        eq "page.opened" 0
        eq "page.count" (start)"page.count"
      }
    }
  }
}
action {
  open from="user" to="page"
  close from="user" to="page"
}
impl "bun" {
  module "page" "../src/page.ts"
  start "page.clear"
  read "page.page" output="page.pageSchema"
  run "page.open" input="page.openSchema" pool="opened"
  run "opened" consume="opened" verb="close"
}
"#;

    fn built(base: &str, from: &str, to: &str) -> Result<(), String> {
        assert!(base.contains(from), "the base has no `{from}`");
        parse(&base.replacen(from, to, 1), &config(CONFIG).unwrap())
            .map(drop)
            .map_err(|e| e.0)
    }

    fn state(object: &str, pairs: &[(&str, Json)]) -> State {
        State::from([(
            object.to_string(),
            pairs
                .iter()
                .map(|(k, v)| (k.to_string(), v.clone()))
                .collect(),
        )])
    }

    #[test]
    fn a_diagram_has_objects_a_path_and_the_code() {
        let d = parse(POOL, &config(CONFIG).unwrap()).unwrap();
        assert_eq!(d.verbs(), [("page", "open"), ("page", "close")]);
        assert_eq!(d.initial("page").unwrap(), "count = 0, opened = 0");
        assert_eq!(d.path.len(), 2);
        assert_eq!(d.impls[0].uses.len(), 4);
        let cases = [
            (
                "action {",
                "rules {\n}\naction {",
                Err("`rules` is not setup, action or impl"),
            ),
            (
                "  close from=\"user\" to=\"page\"\n",
                "  close from=\"user\" to=\"page\" page.count=1\n",
                Err("`close` takes no `page.count`"),
            ),
            (
                "  close from=\"user\" to=\"page\"\n",
                "  shut from=\"user\" to=\"page\"\n",
                Err("`action` line 2: `page` has no `on \"shut\"` line"),
            ),
            (
                "  open from=\"user\" to=\"page\"\n  close from=\"user\" to=\"page\"\n",
                "",
                Err("`action` has one `<verb> from=\"<object>\" to=\"<object>\"` line or more"),
            ),
            (
                "    invariant \"closed\" {",
                "    state count=0 {\n    }\n    invariant \"closed\" {",
                Err(
                    "browser page: `state <key>=<value>… { on … }` has the `on` lines of one state",
                ),
            ),
        ];
        for (from, to, want) in cases {
            assert_eq!(built(POOL, from, to), want.map_err(String::from), "{to}");
        }
    }

    #[test]
    fn an_on_line_has_the_value_of_each_key_after_its_step() {
        let d = parse(BASE, &config(CONFIG).unwrap()).unwrap();
        assert_eq!(
            d.rules("origin", "type")[0].source(),
            r#"on "type" value=(input)"text""#
        );
        let origin = |value: &str| state("origin", &[("value", json!(value))]);
        let (start, before, input) = (
            origin("San Francisco"),
            origin("San Francisco"),
            json!({ "text": "Lahore" }),
        );
        let seen = |after: &State| {
            d.stepped(
                "origin",
                "type",
                None,
                &Seen {
                    start: &start,
                    before: &before,
                    after,
                    input: &input,
                },
            )
        };
        assert_eq!(seen(&origin("Lahore")), None);
        assert_eq!(
            seen(&origin("Lahor")),
            Some(vec![
                r#"on "type" value=(input)"text""#.to_string(),
                r#"origin.value = "Lahor", not "Lahore""#.to_string(),
            ])
        );
        let term = |text: &str| built(BASE, "value=(input)\"text\"", &format!("value={text}"));
        assert_eq!(term("\"Lahore\""), Ok(()));
        assert_eq!(term("\"history.snapshot\""), Ok(()));
        assert_eq!(term("(next)\"origin.value\""), Ok(()));
        assert_eq!(term("(start)\"origin.value\""), Ok(()));
        assert_eq!(
            term("(next)\"value\""),
            Err(format!(
                "element origin: `value=(next)\"value\"` names `\"<object>.<key>\"`: {ON}"
            ))
        );
        assert_eq!(
            term("(later)\"origin.value\""),
            Err(format!(
                "element origin: `value=(later)\"origin.value\"` is not a term: {ON}"
            ))
        );
        assert_eq!(
            built(BASE, "on \"type\" value=", "on \"type\" value=1 value="),
            Err("element origin: `on \"type\"` sets `value` twice".to_string())
        );
    }

    #[test]
    fn a_key_with_no_term_has_its_value_from_before_the_step() {
        let d = parse(POOL, &config(CONFIG).unwrap()).unwrap();
        let d = Diagram {
            objects: d
                .objects
                .into_iter()
                .map(|mut o| {
                    o.rules.iter_mut().for_each(|r| r.next.truncate(1));
                    o
                })
                .collect(),
            ..d
        };
        let page = |count: i64, opened: i64| {
            state(
                "page",
                &[("count", json!(count)), ("opened", json!(opened))],
            )
        };
        let (start, before, input) = (page(0, 0), page(0, 0), Json::Null);
        let seen = |after: &State| {
            d.stepped(
                "page",
                "open",
                None,
                &Seen {
                    start: &start,
                    before: &before,
                    after,
                    input: &input,
                },
            )
        };
        assert_eq!(seen(&page(3, 0)), None);
        assert_eq!(
            seen(&page(3, 1)),
            Some(vec![
                r#"on "open" count=(next)"page.count""#.to_string(),
                "page.opened = 1, not 0".to_string(),
            ])
        );
    }

    #[test]
    fn a_state_block_has_the_on_lines_of_its_state() {
        let d = parse(BASE, &config(CONFIG).unwrap()).unwrap();
        let history = |snapshot: &str| {
            let mut all = state("history", &[("snapshot", json!(snapshot))]);
            all.extend(state("origin", &[("value", json!("San Francisco"))]));
            all
        };
        assert!(d.enabled("history", "leave", &history("snap_0")));
        assert!(!d.enabled("history", "back", &history("snap_0")));
        assert!(d.enabled("history", "back", &history("snap_1")));
        assert_eq!(
            d.rules("history", "back")[0].source(),
            r#"state snapshot="snap_1" { on "back" snapshot="snap_0" }"#
        );
        assert_eq!(d.started(&history("snap_0")), None);
        assert_eq!(
            d.started(&history("snap_1")),
            Some(vec![
                r#"browser "history" snapshot="snap_0""#.to_string(),
                r#"history.snapshot = "snap_1", not "snap_0""#.to_string(),
            ])
        );
    }

    #[test]
    fn an_invariant_has_one_condition_on_one_state() {
        let d = parse(POOL, &config(CONFIG).unwrap()).unwrap();
        let closed = d.invariants()[0];
        assert_eq!(
            closed.source(),
            r#"invariant "closed" { implies(eq(page.opened, 0), eq(page.count, start(page.count))) }"#
        );
        let page = |count: i64, opened: i64| {
            state(
                "page",
                &[("count", json!(count)), ("opened", json!(opened))],
            )
        };
        assert_eq!(closed.judge(&page(0, 0), &page(6, 1)), None);
        assert_eq!(
            closed.judge(&page(0, 0), &page(6, 0)),
            Some(vec![
                closed.source(),
                "eq(0, 0) = true".to_string(),
                "eq(page.count, start(page.count)) = eq(6, 0) = false".to_string(),
            ])
        );
        let at = |e: String| Err(format!("browser page: {e}"));
        let cases = [
            (
                "eq \"page.opened\" 0",
                "eq (next)\"page.opened\" 0",
                at(format!(
                    "`(next)\"page.opened\"` is not a term: {INVARIANT}"
                )),
            ),
            (
                "eq \"page.opened\" 0",
                "eq 0 0",
                at(format!(
                    "`eq(0, 0)` names at least one `\"<object>.<key>\"`: {INVARIANT}"
                )),
            ),
            (
                "eq \"page.opened\" 0",
                "sum \"page.opened\" 0",
                at(format!("`sum` is not a builtin: {INVARIANT}")),
            ),
            (
                "    invariant \"closed\" {\n",
                "    invariant \"closed\" {\n      eq \"page.count\" 0\n",
                at(INVARIANT.to_string()),
            ),
            (
                "      implies {\n        eq \"page.opened\" 0\n        eq \"page.count\" (start)\"page.count\"\n      }\n",
                "      iadd \"page.count\" 1\n",
                at(format!(
                    "`invariant \"closed\"` has a condition, not `iadd(page.count, 1)`: {INVARIANT}"
                )),
            ),
        ];
        for (from, to, want) in cases {
            assert_eq!(built(POOL, from, to), want, "{to}");
        }
    }

    #[test]
    fn each_builtin_gives_the_value_of_the_quint_evaluator() {
        let cases = [
            ("ilte \"page.count\" (start)\"page.count\"", 1, 1, true),
            ("ilte \"page.count\" (start)\"page.count\"", 1, 2, false),
            ("ilt \"page.count\" (start)\"page.count\"", 1, 0, true),
            ("ilt \"page.count\" (start)\"page.count\"", 1, 1, false),
            ("igte \"page.count\" (start)\"page.count\"", 1, 1, true),
            ("igte \"page.count\" (start)\"page.count\"", 1, 0, false),
            ("igt \"page.count\" (start)\"page.count\"", 1, 2, true),
            ("igt \"page.count\" (start)\"page.count\"", 1, 1, false),
            ("neq \"page.count\" (start)\"page.count\"", 1, 2, true),
            ("neq \"page.count\" (start)\"page.count\"", 1, 1, false),
            (
                "or {\neq \"page.count\" 1\neq \"page.count\" 2\n}",
                0,
                2,
                true,
            ),
            (
                "or {\neq \"page.count\" 1\neq \"page.count\" 2\n}",
                0,
                3,
                false,
            ),
            (
                "and {\nigt \"page.count\" 1\nilt \"page.count\" 3\n}",
                0,
                2,
                true,
            ),
            (
                "and {\nigt \"page.count\" 1\nilt \"page.count\" 3\n}",
                0,
                3,
                false,
            ),
            ("not {\neq \"page.count\" 1\n}", 0, 2, true),
            ("not {\neq \"page.count\" 1\n}", 0, 1, false),
            (
                "iff {\neq \"page.count\" 1\neq (start)\"page.count\" 1\n}",
                1,
                1,
                true,
            ),
            (
                "iff {\neq \"page.count\" 1\neq (start)\"page.count\" 1\n}",
                0,
                1,
                false,
            ),
            (
                "eq \"page.count\" {\niadd (start)\"page.count\" 1\n}",
                1,
                2,
                true,
            ),
            (
                "eq \"page.count\" {\nisub (start)\"page.count\" 1\n}",
                1,
                0,
                true,
            ),
            (
                "eq \"page.count\" {\nimul (start)\"page.count\" 3\n}",
                2,
                6,
                true,
            ),
            (
                "eq \"page.count\" {\nidiv (start)\"page.count\" 2\n}",
                7,
                3,
                true,
            ),
            (
                "eq \"page.count\" {\nimod (start)\"page.count\" 4\n}",
                7,
                3,
                true,
            ),
            (
                "eq \"page.count\" {\nipow (start)\"page.count\" 2\n}",
                3,
                9,
                true,
            ),
            (
                "eq \"page.count\" {\niuminus (start)\"page.count\"\n}",
                3,
                -3,
                true,
            ),
            ("eq 3 {\nidiv 6 \"page.count\"\n}", 0, 0, false),
            (
                "implies {\nneq \"page.count\" 0\neq 3 {\nidiv 6 \"page.count\"\n}\n}",
                0,
                0,
                true,
            ),
            ("in \"page.snapshot\" \"snap_1\" \"snap_0\"", 0, 0, true),
            ("in \"page.snapshot\" \"snap_1\" \"snap_2\"", 0, 0, false),
        ];
        let page = |count: i64| {
            state(
                "page",
                &[("count", json!(count)), ("snapshot", json!("snap_0"))],
            )
        };
        for (src, start, now, passes) in cases {
            let doc = document(src).unwrap();
            let body = condition(&doc.nodes()[0], &["page".to_string()]).unwrap();
            assert_eq!(
                body.passes(&page(start), &page(now)),
                passes,
                "{src} from {start} to {now}"
            );
        }
    }

    #[test]
    fn a_pool_has_the_results_of_a_run_line_for_a_later_run_line() {
        let d = parse(POOL, &config(CONFIG).unwrap()).unwrap();
        assert_eq!(d.objects[1].pools, ["opened"]);
        let lines: Vec<String> = d.impls[0].uses.iter().map(Use::line).collect();
        assert_eq!(
            lines,
            [
                r#"`start "page.clear"`"#,
                r#"`read "page.page" output="page.pageSchema"`"#,
                r#"`run "page.open" input="page.openSchema" pool="opened"`"#,
                r#"`run "opened" verb="close" consume="opened"`"#,
            ]
        );
        let cases = [
            (
                "    pool \"opened\"\n",
                "    pool \"closers\"\n",
                Err(
                    r#"`run "page.open" input="page.openSchema" pool="opened"` names the pool `opened`, and no object of `setup` has `pool "opened"`"#,
                ),
            ),
            (
                "browser \"page\" count=0 opened=0",
                "browser \"page\" count=0 opened=\"none\"",
                Err(
                    "browser page: `opened` is the count of the values of `pool \"opened\"`, and a count is an integer",
                ),
            ),
            (
                "verb=\"close\"",
                "verb=\"shut\"",
                Err(
                    r#"`run "opened" verb="shut" consume="opened"` names the verb `shut`, and no object of `setup` has `on "shut"`"#,
                ),
            ),
        ];
        for (from, to, want) in cases {
            assert_eq!(built(POOL, from, to), want.map_err(String::from), "{to}");
        }
    }

    #[test]
    fn a_file_of_weft_has_one_form_for_each_name_and_each_value() {
        let value = |found: &str, line: &str| {
            format!("`{found}` of `{line}` is not a value of weft: {FORM}")
        };
        let line = |name: &str| format!("`{name}` is not a line of weft: {FORM}");
        let cases = [
            (
                "person \"user\"",
                "person user",
                Err(value("user", "person")),
            ),
            (
                "type from=\"user\"",
                "type from=user",
                Err(value("from=user", "type")),
            ),
            (
                "person \"user\"",
                "person #\"user\"#",
                Err(value("#\"user\"#", "person")),
            ),
            (
                "person \"user\"",
                "person \"\"\"\n    user\n    \"\"\"",
                Err(value("\"\"\"\n    user\n    \"\"\"", "person")),
            ),
            (
                "person \"user\"",
                "person \"user\" agent=#true",
                Err(value("agent=#true", "person")),
            ),
            (
                "element \"origin\" value=",
                "element \"origin\" \"value\"=",
                Err(value("\"value\"=\"San Francisco\"", "element")),
            ),
            (
                "element \"origin\" value=\"San Francisco\"",
                "element \"origin\" value=9223372036854775808",
                Err(value("value=9223372036854775808", "element")),
            ),
            (
                "person \"user\"",
                "\"person\" \"user\"",
                Err(line("person")),
            ),
            ("person \"user\"", "a,b \"user\"", Err(line("a,b"))),
            (
                "person \"user\"",
                "person \"the\\suser\"",
                Err(value("\"the\\suser\"", "person")),
            ),
            (
                "person \"user\"",
                "(kind)person \"user\"",
                Err(line("person")),
            ),
            (
                "person \"user\"\n",
                "person \"user\";\n",
                Err(line("person")),
            ),
            (
                "type from=\"user\" to=\"origin\"",
                "type from=\"user\" \\\n    to=\"origin\"",
                Ok(()),
            ),
        ];
        for (from, to, want) in cases {
            assert_eq!(built(BASE, from, to), want, "{to}");
        }
    }

    #[test]
    fn a_draw_block_has_the_values_of_each_key_of_an_input() {
        let block = "    pool \"opened\"\n    draw {\n      id 1 2\n      attributes {\n        class \"a\" \"b\"\n        title\n      }\n    }\n";
        let base = POOL.replacen("    pool \"opened\"\n", block, 1);
        let d = parse(&base, &config(CONFIG).unwrap()).unwrap();
        let draws: Vec<(String, Vec<Json>, bool)> = d.objects[1]
            .draws
            .iter()
            .map(|d| (d.path.join("."), d.values.clone(), d.keys))
            .collect();
        assert_eq!(
            draws,
            [
                ("id".to_string(), vec![json!(1), json!(2)], false),
                (
                    "attributes".to_string(),
                    vec![json!("class"), json!("title")],
                    true
                ),
                (
                    "attributes.class".to_string(),
                    vec![json!("a"), json!("b")],
                    false
                ),
                ("attributes.title".to_string(), vec![], false),
            ]
        );
        let at = |e: &str| Err(format!("browser page: {e}"));
        let cases = [
            (
                "      id 1 2\n",
                "      id 1 1\n",
                at("`id` of `draw` has `1` twice"),
            ),
            (
                "      id 1 2\n",
                "      id 1\n      id 2\n",
                at("`draw` sets `id` twice"),
            ),
            (
                "      attributes {\n",
                "      attributes \"a\" {\n",
                at(DRAW),
            ),
            ("    draw {\n", "    draw \"id\" {\n", at(DRAW)),
            (
                "    draw {\n",
                "    draw {\n      n 1\n    }\n    draw {\n",
                at("has one `draw`"),
            ),
            (
                "  module \"page\" \"../src/page.ts\"\n",
                "  module \"page\" \"../src/page.ts\"\n  module \"other\" \"../src/other.ts\"\n",
                Err("no line of `impl \"bun\"` names `module \"other\"`".to_string()),
            ),
        ];
        for (from, to, want) in cases {
            assert_eq!(built(&base, from, to), want, "{to}");
        }
    }
}
