use std::collections::{BTreeMap, HashSet};
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
    pub impls: Vec<Impl>,
}

#[derive(Debug, Clone)]
pub struct Draw {
    pub path: Vec<String>,
    pub values: Vec<Json>,
    pub keys: bool,
}

#[derive(Debug, Clone)]
pub struct Impl {
    pub adapter: String,
    pub modules: Vec<(String, String)>,
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

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Kind(String);

impl Kind {
    fn parse(s: &str) -> Result<Kind, String> {
        match !s.is_empty() && s.chars().all(|c| c.is_ascii_lowercase() || c == '-') {
            true => Ok(Kind(s.to_string())),
            false => Err(format!("`{s}` is no kind; a kind is a lowercase word")),
        }
    }

    pub fn is_person(&self) -> bool {
        self.0 == "person"
    }
}

impl fmt::Display for Kind {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Clone)]
pub struct Object {
    pub name: String,
    pub kind: Kind,
    pub icon: String,
    pub line: usize,
    pub start: Pairs,
    pub invariants: Vec<Invariant>,
    pub draws: Vec<Draw>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Now {
    Key(String, String),
    Start(String, String),
    Value(Json),
    Var(String, Vec<String>),
    App(Op, Vec<Now>),
    Each(Quant, String, Box<Now>, Box<Now>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Quant {
    Forall,
    Exists,
}

impl Quant {
    fn name(self) -> &'static str {
        match self {
            Quant::Forall => "forall",
            Quant::Exists => "exists",
        }
    }
}

type Env = Vec<(String, String, Json)>;

fn items(v: Json) -> Option<Vec<Json>> {
    match v {
        Json::Array(all) => Some(all),
        Json::Object(all) => Some(all.into_iter().map(|(_, v)| v).collect()),
        _ => None,
    }
}

#[derive(Debug, Clone)]
pub struct Invariant {
    pub name: String,
    pub body: Now,
    pub line: usize,
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

impl Now {
    fn leaf(&self, env: &Env) -> Option<String> {
        match self {
            Now::Key(o, k) => Some(format!("{o}.{k}")),
            Now::Start(o, k) => Some(format!("start({o}.{k})")),
            Now::Value(v) => Some(v.to_string()),
            Now::Var(v, path) => {
                let head = env
                    .iter()
                    .rev()
                    .find(|(n, _, _)| n == v)
                    .map_or(v.clone(), |(_, label, _)| label.clone());
                Some([vec![head], path.clone()].concat().join("."))
            }
            Now::App(..) | Now::Each(..) => None,
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
            (None, Now::Each(q, v, set, body)) => format!(
                "{}.{}({v} => {})",
                set.show(part),
                q.name(),
                body.show(part)
            ),
            (None, _) => String::new(),
        }
    }

    pub fn quint(&self) -> String {
        self.named(&Vec::new())
    }

    fn named(&self, env: &Env) -> String {
        self.show(&|e| e.leaf(env))
    }

    fn valued(&self, start: &State, now: &State, env: &Env) -> String {
        self.show(&|e| match e {
            Now::App(op, _) if op.boolean() => None,
            Now::Each(..) => None,
            e => Some(found(e.eval(start, now, env).as_ref())),
        })
    }

    fn bound(&self, start: &State, now: &State, env: &Env) -> Option<Vec<Env>> {
        let Now::Each(_, v, set, _) = self else {
            return None;
        };
        let label = set.named(env);
        Some(
            items(set.eval(start, now, env)?)?
                .into_iter()
                .enumerate()
                .map(|(i, item)| {
                    let mut inner = env.clone();
                    inner.push((v.clone(), format!("{label}[{i}]"), item));
                    inner
                })
                .collect(),
        )
    }

    fn eval(&self, start: &State, now: &State, env: &Env) -> Option<Json> {
        let first = |all: &[Now], stop: bool| {
            all.iter()
                .map(|e| e.eval(start, now, env))
                .find(|t| *t != Some(json!(!stop)))
                .unwrap_or(Some(json!(!stop)))
                .filter(Json::is_boolean)
        };
        match self {
            Now::Key(o, k) => read(now, o, k).cloned(),
            Now::Start(o, k) => read(start, o, k).cloned(),
            Now::Value(v) => Some(v.clone()),
            Now::Var(v, path) => env
                .iter()
                .rev()
                .find(|(n, _, _)| n == v)
                .and_then(|(_, _, item)| path.iter().try_fold(item, |node, part| node.get(part)))
                .cloned(),
            Now::App(Op::And, all) => first(all, false),
            Now::App(Op::Or, all) => first(all, true),
            Now::App(Op::Implies, all) => match all[0].eval(start, now, env)? {
                Json::Bool(false) => Some(json!(true)),
                Json::Bool(true) => all[1].eval(start, now, env).filter(Json::is_boolean),
                _ => None,
            },
            Now::App(op, all) => op.apply(
                &all.iter()
                    .map(|e| e.eval(start, now, env))
                    .collect::<Option<Vec<_>>>()?,
            ),
            Now::Each(q, _, _, body) => {
                let seen = self
                    .bound(start, now, env)?
                    .iter()
                    .map(|inner| body.eval(start, now, inner)?.as_bool())
                    .collect::<Option<Vec<bool>>>()?;
                Some(json!(match q {
                    Quant::Forall => seen.iter().all(|b| *b),
                    Quant::Exists => seen.iter().any(|b| *b),
                }))
            }
        }
    }

    fn passes(&self, start: &State, now: &State, env: &Env) -> bool {
        self.eval(start, now, env) == Some(json!(true))
    }

    fn evidence(&self, start: &State, now: &State, env: &Env) -> Vec<String> {
        let witness = match self {
            Now::Each(Quant::Forall, _, _, body) => self
                .bound(start, now, env)
                .unwrap_or_default()
                .into_iter()
                .find(|inner| !body.passes(start, now, inner))
                .map(|inner| body.evidence(start, now, &inner)),
            _ => None,
        };
        match (witness, self) {
            (Some(found), _) => found,
            (None, Now::App(Op::And, all)) => all
                .iter()
                .find(|e| !e.passes(start, now, env))
                .map_or_else(Vec::new, |e| e.evidence(start, now, env)),
            (None, Now::App(Op::Implies, all)) => [
                vec![format!("{} = true", all[0].valued(start, now, env))],
                all[1].evidence(start, now, env),
            ]
            .concat(),
            (None, e) => vec![format!(
                "{} = {} = false",
                e.named(env),
                e.valued(start, now, env)
            )],
        }
    }

    fn all(&self) -> Vec<&Now> {
        match self {
            Now::App(_, args) => [vec![self], args.iter().flat_map(Now::all).collect()].concat(),
            Now::Each(_, _, set, body) => [vec![self], set.all(), body.all()].concat(),
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

    fn fields(&self) -> Vec<(&String, &String, &[String])> {
        self.all()
            .into_iter()
            .filter_map(|e| match e {
                Now::Each(_, v, set, body) => match set.as_ref() {
                    Now::Key(o, k) | Now::Start(o, k) => Some((o, k, v, body)),
                    _ => None,
                },
                _ => None,
            })
            .flat_map(|(o, k, v, body)| {
                body.all().into_iter().filter_map(move |e| match e {
                    Now::Var(n, path) if n == v => Some((o, k, path.as_slice())),
                    _ => None,
                })
            })
            .collect()
    }

    fn refers(&self) -> bool {
        self.all()
            .into_iter()
            .any(|e| matches!(e, Now::Key(..) | Now::Start(..) | Now::Var(..)))
    }
}

impl Invariant {
    pub fn source(&self) -> String {
        format!("invariant \"{}\" {{ {} }}", self.name, self.body.quint())
    }

    pub fn items(&self, now: &State) -> Option<usize> {
        let sets: HashSet<(&String, &String)> = self
            .body
            .fields()
            .into_iter()
            .map(|(o, k, _)| (o, k))
            .collect();
        (!sets.is_empty()).then(|| {
            sets.into_iter()
                .filter_map(|(o, k)| read(now, o, k)?.as_array())
                .map(Vec::len)
                .sum()
        })
    }

    pub fn judge(&self, start: &State, now: &State) -> Option<Vec<String>> {
        let env = Vec::new();
        (!self.body.passes(start, now, &env))
            .then(|| [vec![self.source()], self.body.evidence(start, now, &env)].concat())
    }
}

impl Object {
    fn keys(&self) -> Vec<(&String, &String)> {
        let o = &self.name;
        self.start
            .keys()
            .map(|k| (o, k))
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

    pub fn caller(&self) -> Option<&str> {
        self.objects
            .iter()
            .find(|o| o.kind.is_person())
            .map(|o| o.name.as_str())
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

    pub fn fields(&self) -> Vec<(&String, &String, &[String])> {
        self.invariants()
            .into_iter()
            .flat_map(|i| i.body.fields())
            .collect()
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

fn implemented(n: &KdlNode) -> Result<Impl, String> {
    let mut found = Impl {
        adapter: shape(n, 1, &[], true)?.remove(0),
        modules: Vec::new(),
    };
    for c in lines(n) {
        if name(c) != "module" {
            return Err(format!(
                "`impl \"{}\"` has `module \"<object>\" \"<path>\"` lines, not `{}`; the module gives its `reset`, its `url` and its actions",
                found.adapter,
                name(c)
            ));
        }
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
    match !found.modules.is_empty() {
        true => Ok(found),
        false => Err(format!("`impl \"{}\"` has no `module` line", found.adapter)),
    }
}

const INVARIANT: &str = "`invariant \"<name>\" { <condition> }` has one condition on one state; a condition is a Quint builtin `<op> <term>… { <op line>… }`: `and`, `or`, `not`, `implies`, `iff`, `eq`, `neq`, `in`, `ilt`, `ilte`, `igt`, `igte`, `iadd`, `isub`, `imul`, `idiv`, `imod`, `ipow` or `iuminus`, or `forall` and `exists` as `forall \"<name>\" in=\"<object>.<key>\" { <condition> }` over each item of a list or a map; a term is `\"<name>.<field>\"` inside it, `\"<object>.<key>\"`, `(start)\"<object>.<key>\"` or a value, and each comparison names at least one `\"<object>.<key>\"`";

fn refer(names: &[String], s: &str) -> Option<(String, String)> {
    let (o, k) = s.split_once('.')?;
    (names.iter().any(|n| n == o) && !k.is_empty() && !k.contains('.'))
        .then(|| (o.to_string(), k.to_string()))
}

fn term(e: &KdlEntry, names: &[String], bound: &[String]) -> Result<Now, String> {
    match (e.name(), e.ty().map(kdl::KdlIdentifier::value), e.value()) {
        (None, None, KdlValue::String(s))
            if bound
                .iter()
                .any(|v| s.split('.').next() == Some(v.as_str())) =>
        {
            let mut path = s.split('.').map(String::from);
            Ok(Now::Var(path.next().unwrap_or_default(), path.collect()))
        }
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

fn each(n: &KdlNode, q: Quant, names: &[String], bound: &[String]) -> Result<Now, String> {
    let wrong = || {
        format!(
            "`{}` is `{} \"<name>\" in=\"<object>.<key>\" {{ <condition> }}`: {INVARIANT}",
            n.to_string().trim(),
            q.name()
        )
    };
    let named: Vec<String> = n
        .entries()
        .iter()
        .filter(|e| e.name().is_none())
        .filter_map(|e| e.value().as_string().map(String::from))
        .collect();
    let set = n
        .get("in")
        .and_then(KdlValue::as_string)
        .and_then(|s| refer(names, s))
        .map(|(o, k)| Now::Key(o, k));
    match (named.as_slice(), set, lines(n), n.entries().len()) {
        ([v], Some(set), [one], 2) if !names.contains(v) && !bound.contains(v) => {
            let inner = [bound, std::slice::from_ref(v)].concat();
            let body = condition(one, names, &inner)?;
            match &body {
                Now::App(op, _) if op.boolean() => {}
                Now::Each(..) => {}
                _ => return Err(wrong()),
            }
            Ok(Now::Each(q, v.clone(), Box::new(set), Box::new(body)))
        }
        _ => Err(wrong()),
    }
}

fn condition(n: &KdlNode, names: &[String], bound: &[String]) -> Result<Now, String> {
    match name(n) {
        "forall" => return each(n, Quant::Forall, names, bound),
        "exists" => return each(n, Quant::Exists, names, bound),
        _ => {}
    }
    let op =
        Op::parse(name(n)).ok_or_else(|| format!("`{}` is not a builtin: {INVARIANT}", name(n)))?;
    let args = n
        .entries()
        .iter()
        .map(|e| term(e, names, bound))
        .chain(lines(n).iter().map(|c| condition(c, names, bound)))
        .collect::<Result<Vec<_>, _>>()?;
    let (min, max) = op.arity();
    let app = Now::App(op, args);
    match &app {
        Now::App(_, args) if args.len() < min || args.len() > max => {
            Err(format!("`{}` takes {}: {INVARIANT}", op.name(), op.takes()))
        }
        _ if op.compares() && !app.refers() => Err(format!(
            "`{}` names at least one `\"<object>.<key>\"`: {INVARIANT}",
            app.quint()
        )),
        _ => Ok(app),
    }
}

fn invariant(n: &KdlNode, names: &[String], src: &str) -> Result<Invariant, String> {
    let named = shape(n, 1, &[], true)?.remove(0);
    let body = match lines(n) {
        [one] => condition(one, names, &[])?,
        _ => return Err(INVARIANT.into()),
    };
    match &body {
        Now::App(op, _) if op.boolean() => Ok(Invariant {
            name: named,
            body,
            line: line(src, n),
        }),
        Now::Each(..) => Ok(Invariant {
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
        .ok_or_else(|| {
            format!(
                "`{kind}` is a kind once `weft.kdl` has `style \"{kind}\" icon=\"<lucide icon>\"`"
            )
        })?;
    let named = shape(n, 1, &keys(n), true)
        .map_err(|e| format!("{kind}: {e}"))?
        .remove(0);
    let at = |e: String| format!("{kind} {named}: {e}");
    let mut o = Object {
        name: named.clone(),
        kind: kind.clone(),
        icon,
        line: line(src, n),
        start: pairs(n).map_err(at)?,
        invariants: Vec::new(),
        draws: Vec::new(),
    };
    for c in n.iter_children() {
        match name(c) {
            "invariant" => invariant(c, names, src).map(|found| o.invariants.push(found)),
            "draw" if !o.draws.is_empty() => Err("has one `draw`".into()),
            "draw" => draw(c, &[], &mut o.draws),
            other => Err(format!("has `draw` and `invariant`, not `{other}`")),
        }
        .map_err(at)?;
    }
    Ok(o)
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
        if parent != "setup" {
            return Err(format!("`{parent}` is not setup or impl"));
        }
        shape(n, 0, &[], true)?;
        if !seen.insert(parent) {
            return Err(format!("`{parent}` is set twice"));
        }
        for c in n.iter_children() {
            d.objects.push(object(c, config, &names, src)?);
        }
    }
    if d.objects.is_empty() {
        return Err("no objects".into());
    }
    let mut known = HashSet::new();
    if let Some(o) = d.objects.iter().find(|o| !known.insert(o.name.as_str())) {
        return Err(format!("`{}` is declared twice", o.name));
    }
    for (module, _) in d.impls.iter().flat_map(|i| &i.modules) {
        if d.object(module).is_none() {
            return Err(format!("`module \"{module}\"` names no object of `setup`"));
        }
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
    pub styles: Vec<(Kind, String)>,
    pub adapter: Adapter,
    pub transport: Option<Transport>,
    pub cases: u64,
    pub steps: u64,
    pub runners: Vec<(String, Start)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Start {
    Command(String),
    Bun,
}

pub fn config(src: &str) -> Result<Config, Error> {
    let doc = document(src)?;
    let mut c = Config {
        include: Vec::new(),
        styles: Vec::new(),
        adapter: Adapter::Default,
        transport: None,
        cases: 100,
        steps: 50,
        runners: Vec::new(),
    };
    let mut set = false;
    for n in doc.nodes() {
        match name(n) {
            "include" => c.include.extend(shape(n, 1, &[], false).map_err(Error)?),
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
                let name = shape(n, 1, &["cases", "steps"], false)
                    .map_err(Error)?
                    .remove(0);
                let count = |key: &str, default: Option<u64>| {
                    match (n.get(key), default) {
                        (Some(KdlValue::Integer(k)), _) => {
                            u64::try_from(*k).ok().filter(|k| *k > 0)
                        }
                        (None, default) => default,
                        _ => None,
                    }
                    .ok_or_else(|| {
                        Error(format!(
                            "`adapter \"{name}\"` takes `{key}=<count>`, a count above 0"
                        ))
                    })
                };
                c.adapter = adapter;
                c.cases = count("cases", None)?;
                c.steps = count("steps", Some(c.steps))?;
                set = true;
            }
            "runner" => {
                let adapter = shape(n, 1, &["command", "harness"], false)
                    .map_err(Error)?
                    .remove(0);
                if c.runners.iter().any(|(known, _)| *known == adapter) {
                    return Err(Error(format!("`runner \"{adapter}\"` is set twice")));
                }
                let start = match (n.get("command").map(text), n.get("harness").map(text)) {
                    (Some(command), None) => Start::Command(command),
                    (None, Some(harness)) if harness == "bun" => Start::Bun,
                    _ => {
                        return Err(Error(format!(
                            "`runner \"{adapter}\"` takes `command=\"<command>\"` or `harness=\"bun\"`"
                        )));
                    }
                };
                c.runners.push((adapter, start));
            }
            other => {
                return Err(Error(format!(
                    "`{other}` is not include, style, adapter, transport or runner"
                )));
            }
        }
    }
    match c.include.is_empty() {
        true => Err(Error("needs `include \"<glob>\"`".into())),
        false => Ok(c),
    }
}

pub fn parse(src: &str, config: &Config) -> Result<Diagram, Error> {
    build(&document(src)?, src, config).map_err(Error)
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONFIG: &str = r#"include "**/*.weft.kdl"
style "person" icon="user"
style "browser" icon="app-window"
style "element" icon="square-dashed-mouse-pointer""#;

    const BASE: &str = r#"
setup {
  person "user"
  browser "history" snapshot="snap_0"
  element "origin" value="San Francisco" {
    invariant "filled" {
      neq "origin.value" ""
    }
  }
}
impl "bun" {
  module "origin" "../src/origin.ts"
}
"#;

    const POOL: &str = r#"
setup {
  person "user"
  browser "page" count=0 opened=0 {
    invariant "closed" {
      implies {
        eq "page.opened" 0
        eq "page.count" (start)"page.count"
      }
    }
  }
}
impl "bun" {
  module "page" "../src/page.ts"
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
    fn an_adapter_takes_the_count_of_cases_and_steps() {
        let steps = |line: &str| config(&format!("{CONFIG}\n{line}")).map(|c| (c.cases, c.steps));
        assert_eq!(steps(r#"adapter "bun" cases=5"#).ok(), Some((5, 50)));
        assert_eq!(
            steps(r#"adapter "bun" cases=5 steps=20"#).ok(),
            Some((5, 20))
        );
        assert!(steps(r#"adapter "bun" cases=5 steps=0"#).is_err());
        assert!(steps(r#"adapter "bun" steps=20"#).is_err());
    }

    #[test]
    fn a_diagram_has_objects_and_the_code() {
        let d = parse(POOL, &config(CONFIG).unwrap()).unwrap();
        assert_eq!(d.initial("page").unwrap(), "count = 0, opened = 0");
        assert_eq!(d.impls[0].modules.len(), 1);
        let cases = [
            (
                "impl \"bun\" {",
                "rules {\n}\nimpl \"bun\" {",
                Err("`rules` is not setup or impl"),
            ),
            (
                "  module \"page\" \"../src/page.ts\"\n",
                "  module \"page\" \"../src/page.ts\"\n  run \"page.open\"\n",
                Err(
                    "`impl \"bun\"` has `module \"<object>\" \"<path>\"` lines, not `run`; the module gives its `reset`, its `url` and its actions",
                ),
            ),
            (
                "  module \"page\" \"../src/page.ts\"\n",
                "  module \"tab\" \"../src/tab.ts\"\n",
                Err("`module \"tab\"` names no object of `setup`"),
            ),
        ];
        for (from, to, want) in cases {
            assert_eq!(built(POOL, from, to), want.map_err(String::from), "{to}");
        }
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
            let body = condition(&doc.nodes()[0], &["page".to_string()], &[]).unwrap();
            assert_eq!(
                body.passes(&page(start), &page(now), &Vec::new()),
                passes,
                "{src} from {start} to {now}"
            );
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
                "module \"origin\" \"../src/origin.ts\"",
                "module origin \"../src/origin.ts\"",
                Err(value("origin", "module")),
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
                "module \"origin\" \"../src/origin.ts\"",
                "module \"origin\" \\\n    \"../src/origin.ts\"",
                Ok(()),
            ),
        ];
        for (from, to, want) in cases {
            assert_eq!(built(BASE, from, to), want, "{to}");
        }
    }

    #[test]
    fn a_draw_block_has_the_values_of_each_key_of_an_input() {
        let block = "    draw {\n      id 1 2\n      attributes {\n        class \"a\" \"b\"\n        title\n      }\n    }\n";
        let base = POOL.replacen(
            "  browser \"page\" count=0 opened=0 {\n",
            &format!("  browser \"page\" count=0 opened=0 {{\n{block}"),
            1,
        );
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
        ];
        for (from, to, want) in cases {
            assert_eq!(built(&base, from, to), want, "{to}");
        }
    }
}
