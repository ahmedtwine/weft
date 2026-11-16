use std::collections::{BTreeMap, HashSet};
use std::fmt;

use kdl::KdlValue;
pub use kdl::{KdlDocument, KdlEntry, KdlNode};
use miette::SourceCode;
use serde_json::{Value as Json, json};

pub type Pairs = BTreeMap<String, Json>;

pub type State = BTreeMap<String, Pairs>;

#[derive(Debug, Clone)]
pub struct Diagram {
    pub objects: Vec<Object>,
    pub entities: Vec<Entity>,
    pub records: BTreeMap<String, Vec<(String, Type)>>,
    pub pools: Vec<Pool>,
    pub actions: Vec<Action>,
    pub targets: Vec<(String, String)>,
    pub impls: Vec<Impl>,
    pub run: Run,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Type {
    Int(Option<(i64, i64)>),
    Str,
    Bool,
    Id(String),
    Record(String),
    Set(Box<Type>),
    List(Box<Type>),
    Map(String),
    Optional(Box<Type>),
    Text,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entity {
    pub name: String,
    pub ids: Option<u64>,
    pub fields: Vec<(String, Type)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pool {
    pub name: String,
    pub object: String,
    pub of: Type,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Take {
    Reuse,
    Consume,
    Maybe,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    Drawn(Type),
    Pool(String, Take),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Input {
    pub name: String,
    pub source: Source,
}

#[derive(Debug, Clone)]
pub struct Action {
    pub verb: String,
    pub object: String,
    pub line: usize,
    pub inputs: Vec<Input>,
    pub returns: Option<(String, Type)>,
    pub when: Option<Now>,
    pub adds: Vec<(String, String)>,
    pub writes: Vec<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Module {
    pub object: String,
    pub path: String,
    pub reset: Option<String>,
    pub state: Option<String>,
    pub url: Option<String>,
    pub actions: Vec<(String, String)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Check {
    Always,
    Sampled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Run {
    pub cases: u64,
    pub steps: u64,
    pub check: Check,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct At {
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Assert {
    pub op: &'static str,
    pub at: At,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    pub assert: Assert,
    pub evidence: Vec<String>,
}

impl Assert {
    pub fn origin(&self, file: &str) -> String {
        format!("{} at {file}:{}:{}", self.op, self.at.line, self.at.column)
    }
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
    pub modules: Vec<Module>,
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
    pub state: Vec<(String, Type)>,
    pub invariants: Vec<Invariant>,
    pub draws: Vec<Draw>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Now {
    Key(String, String),
    Start(String, String),
    Value(Json),
    Var(String, Vec<String>),
    Index(Box<Now>, Box<Now>, Vec<String>),
    App(Op, Vec<Now>, At),
    Each(Quant, String, Box<Now>, Box<Now>, At),
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

#[derive(Debug, Clone)]
pub struct Invariant {
    pub body: Now,
    pub at: At,
    pub check: Check,
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
    Contains,
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
            Op::In | Op::Contains => "in",
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
            Op::In | Op::Contains => {
                "a term and one or more terms of its type, or a term and a set, a list or a map of its type"
            }
            Op::Iuminus => "one integer",
            _ => "two integers",
        }
    }

    fn compares(self) -> bool {
        matches!(
            self,
            Op::Eq | Op::Neq | Op::In | Op::Contains | Op::Ilt | Op::Ilte | Op::Igt | Op::Igte
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
            (Op::Contains, [a, Json::Array(all)]) => Some(json!(all.contains(a))),
            (Op::Contains, [Json::String(a), Json::Object(all)]) => {
                Some(json!(all.contains_key(a)))
            }
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
            Now::Index(base, key, path) => Some(
                [
                    vec![format!("{}[{}]", base.leaf(env)?, key.leaf(env)?)],
                    path.clone(),
                ]
                .concat()
                .join("."),
            ),
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
            (None, Now::App(Op::In, args, _)) => {
                format!("in({}, Set({}))", args[0].show(part), each(&args[1..]))
            }
            (None, Now::App(op, args, _)) => format!("{}({})", op.name(), each(args)),
            (None, Now::Each(q, v, set, body, _)) => format!(
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
            Now::App(op, ..) if op.boolean() => None,
            Now::Each(..) => None,
            e => Some(found(e.eval(start, now, env).as_ref())),
        })
    }

    fn bound(&self, start: &State, now: &State, env: &Env) -> Option<Vec<Env>> {
        let Now::Each(_, v, set, ..) = self else {
            return None;
        };
        let label = set.named(env);
        let labelled: Vec<(String, Json)> = match set.eval(start, now, env)? {
            Json::Object(all) => all
                .into_iter()
                .map(|(key, item)| (format!("{label}[{key}]"), item))
                .collect(),
            Json::Array(all) => all
                .into_iter()
                .enumerate()
                .map(|(i, item)| match &item {
                    Json::String(_) | Json::Number(_) => {
                        (format!("{label}[{}]", show(&item)), item)
                    }
                    _ => (format!("{label}[{i}]"), item),
                })
                .collect(),
            _ => return None,
        };
        Some(
            labelled
                .into_iter()
                .map(|(at, item)| {
                    let mut inner = env.clone();
                    inner.push((v.clone(), at, item));
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
            Now::Index(base, key, path) => {
                let all = base.eval(start, now, env)?;
                let key = key.eval(start, now, env)?;
                let item = all.get(key.as_str()?)?;
                path.iter()
                    .try_fold(item, |node, part| node.get(part))
                    .cloned()
            }
            Now::App(Op::And, all, _) => first(all, false),
            Now::App(Op::Or, all, _) => first(all, true),
            Now::App(Op::Implies, all, _) => match all[0].eval(start, now, env)? {
                Json::Bool(false) => Some(json!(true)),
                Json::Bool(true) => all[1].eval(start, now, env).filter(Json::is_boolean),
                _ => None,
            },
            Now::App(op, all, _) => op.apply(
                &all.iter()
                    .map(|e| e.eval(start, now, env))
                    .collect::<Option<Vec<_>>>()?,
            ),
            Now::Each(q, _, _, body, _) => {
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

    fn assert(&self) -> Option<Assert> {
        match self {
            Now::App(op, _, at) => Some(Assert {
                op: op.name(),
                at: *at,
            }),
            Now::Each(q, .., at) => Some(Assert {
                op: q.name(),
                at: *at,
            }),
            _ => None,
        }
    }

    fn failure(&self, outer: Assert, start: &State, now: &State, env: &Env) -> Failure {
        let assert = self.assert().unwrap_or(outer);
        let witness = match self {
            Now::Each(Quant::Forall, _, _, body, _) => self
                .bound(start, now, env)
                .unwrap_or_default()
                .into_iter()
                .find(|inner| !body.passes(start, now, inner))
                .map(|inner| body.failure(assert, start, now, &inner)),
            _ => None,
        };
        match (witness, self) {
            (Some(found), _) => found,
            (None, Now::App(Op::And, all, _)) => {
                all.iter().find(|e| !e.passes(start, now, env)).map_or_else(
                    || Failure {
                        assert,
                        evidence: Vec::new(),
                    },
                    |e| e.failure(assert, start, now, env),
                )
            }
            (None, Now::App(Op::Implies, all, _)) => {
                let then = all[1].failure(assert, start, now, env);
                Failure {
                    evidence: [
                        vec![format!("{} = true", all[0].valued(start, now, env))],
                        then.evidence,
                    ]
                    .concat(),
                    ..then
                }
            }
            (None, e) => Failure {
                assert,
                evidence: vec![format!(
                    "{} = {} = false",
                    e.named(env),
                    e.valued(start, now, env)
                )],
            },
        }
    }

    fn all(&self) -> Vec<&Now> {
        match self {
            Now::App(_, args, _) => [vec![self], args.iter().flat_map(Now::all).collect()].concat(),
            Now::Each(_, _, set, body, _) => [vec![self], set.all(), body.all()].concat(),
            Now::Index(base, key, _) => [vec![self], base.all(), key.all()].concat(),
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
                Now::Each(_, v, set, body, _) => match set.as_ref() {
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
        format!("invariant {{ {} }}", self.body.quint())
    }

    pub fn place(&self) -> String {
        format!("{}:{}", self.at.line, self.at.column)
    }

    pub fn origin(&self, file: &str) -> String {
        format!("{file}:{}", self.place())
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

    pub fn judge(&self, start: &State, now: &State) -> Option<Failure> {
        let env = Vec::new();
        let outer = Assert {
            op: "invariant",
            at: self.at,
        };
        (!self.body.passes(start, now, &env)).then(|| {
            let found = self.body.failure(outer, start, now, &env);
            Failure {
                evidence: [vec![self.source()], found.evidence].concat(),
                ..found
            }
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Compare {
    pub op: Op,
    pub sides: [String; 2],
    pub objects: Vec<String>,
    pub holds: Option<bool>,
}

impl Now {
    fn owner<'a>(&'a self, outer: &[&'a Now]) -> Option<(&'a str, String)> {
        match self {
            Now::Key(o, k) => Some((o, k.clone())),
            Now::Start(o, k) => Some((o, format!("start {k}"))),
            Now::Var(v, path) => outer.iter().rev().find_map(|e| match e {
                Now::Each(_, n, set, ..) if n == v => {
                    let (object, label) = set.owner(outer)?;
                    Some((
                        object,
                        match path.is_empty() {
                            true => label,
                            false => path.join("."),
                        },
                    ))
                }
                _ => None,
            }),
            Now::Index(base, _, path) => {
                let (object, label) = base.owner(outer)?;
                Some((object, [vec![label], path.clone()].concat().join(".")))
            }
            _ => None,
        }
    }

    fn side(args: &[Now], outer: &[&Now], objects: &mut Vec<String>) -> String {
        let shown: Vec<String> = args
            .iter()
            .map(|a| match a.owner(outer) {
                Some((object, label)) => {
                    if !objects.iter().any(|o| o == object) {
                        objects.push(object.to_string());
                    }
                    label
                }
                None => match a {
                    Now::Value(v) => v.to_string(),
                    e => e.quint(),
                },
            })
            .collect();
        match shown.len() {
            1 => shown.concat(),
            _ => format!("{{{}}}", shown.join(", ")),
        }
    }

    fn compares<'a>(&'a self, outer: &mut Vec<&'a Now>, now: &State, found: &mut Vec<Compare>) {
        match self {
            Now::App(
                op @ (Op::Eq
                | Op::Neq
                | Op::In
                | Op::Contains
                | Op::Ilt
                | Op::Ilte
                | Op::Igt
                | Op::Igte),
                args,
                _,
            ) if args.len() >= 2 => {
                let mut objects = Vec::new();
                let sides = [
                    Now::side(&args[..1], outer, &mut objects),
                    Now::side(&args[1..], outer, &mut objects),
                ];
                let wrapped = outer.iter().rev().fold(self.clone(), |inner, e| match e {
                    Now::Each(q, v, set, _, at) => {
                        Now::Each(*q, v.clone(), set.clone(), Box::new(inner), *at)
                    }
                    _ => inner,
                });
                let holds = wrapped
                    .eval(&State::new(), now, &Vec::new())
                    .and_then(|v| v.as_bool());
                found.push(Compare {
                    op: *op,
                    sides,
                    objects,
                    holds,
                });
            }
            Now::App(_, args, _) => args.iter().for_each(|a| a.compares(outer, now, found)),
            Now::Each(_, _, _, body, _) => {
                outer.push(self);
                body.compares(outer, now, found);
                outer.pop();
            }
            _ => {}
        }
    }
}

impl Invariant {
    pub fn compares(&self, now: &State) -> Vec<Compare> {
        let mut found = Vec::new();
        self.body.compares(&mut Vec::new(), now, &mut found);
        found
    }
}

impl fmt::Display for Type {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Type::Int(None) => f.write_str("int"),
            Type::Int(Some((a, b))) => write!(f, "{a}..{b}"),
            Type::Str => f.write_str("string"),
            Type::Bool => f.write_str("bool"),
            Type::Id(e) | Type::Record(e) => f.write_str(e),
            Type::Set(t) => write!(f, "set of {t}"),
            Type::List(t) => write!(f, "list of {t}"),
            Type::Map(e) => write!(f, "map of {e}"),
            Type::Optional(t) => write!(f, "optional {t}"),
            Type::Text => f.write_str("a string"),
        }
    }
}

impl Type {
    fn plain(&self) -> &Type {
        match self {
            Type::Optional(t) => t.plain(),
            t => t,
        }
    }

    fn fits(&self, other: &Type) -> bool {
        match (self.plain(), other.plain()) {
            (Type::Text, Type::Text | Type::Str | Type::Id(_))
            | (Type::Str | Type::Id(_), Type::Text)
            | (Type::Int(_), Type::Int(_)) => true,
            (a, b) => a == b,
        }
    }

    fn element(&self) -> Option<Type> {
        match self.plain() {
            Type::Set(t) | Type::List(t) => Some(*t.clone()),
            Type::Map(e) => Some(Type::Record(e.clone())),
            _ => None,
        }
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

    pub fn actions(&self) -> Vec<(String, String)> {
        self.actions
            .iter()
            .map(|a| (a.verb.clone(), a.object.clone()))
            .collect()
    }

    pub fn action(&self, verb: &str) -> Option<&Action> {
        self.actions.iter().find(|a| a.verb == verb)
    }

    pub fn pool(&self, name: &str) -> Option<&Pool> {
        self.pools.iter().find(|p| p.name == name)
    }

    pub fn module(&self, object: &str) -> Option<(&Impl, &Module)> {
        self.impls.iter().find_map(|i| {
            i.modules
                .iter()
                .find(|m| m.object == object)
                .map(|m| (i, m))
        })
    }

    pub fn invariants(&self) -> Vec<&Invariant> {
        self.objects.iter().flat_map(|o| &o.invariants).collect()
    }

    pub fn keys(&self) -> Vec<(&String, &String)> {
        self.objects
            .iter()
            .flat_map(|o| o.state.iter().map(move |(k, _)| (&o.name, k)))
            .collect()
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

    fn entity(&self, name: &str) -> Option<&Entity> {
        self.entities.iter().find(|e| e.name == name)
    }

    fn ids(&self, entity: &str) -> Option<Vec<Json>> {
        let n = self.entity(entity)?.ids?;
        Some((1..=n).map(|i| json!(format!("{entity}{i}"))).collect())
    }

    fn word(&self, w: &str) -> Result<Type, String> {
        let range = w.split_once("..").and_then(|(a, b)| {
            let (a, b) = (a.parse::<i64>().ok()?, b.parse::<i64>().ok()?);
            (a <= b).then_some((a, b))
        });
        match (w, range) {
            ("int", _) => Ok(Type::Int(None)),
            ("string", _) => Ok(Type::Str),
            ("bool", _) => Ok(Type::Bool),
            (_, Some(r)) => Ok(Type::Int(Some(r))),
            ("set" | "list" | "map" | "optional", _) => Err(format!("`{w}` needs `of=\"<type>\"`")),
            (w, _) if self.entity(w).is_some() => Ok(Type::Id(w.to_string())),
            (w, _) if self.records.contains_key(w) => Ok(Type::Record(w.to_string())),
            (w, _) => Err(format!(
                "`{w}` is no type; a type is `int`, `<low>..<high>`, `string`, `bool`, an `entity`, a record of an `input` or `returns`, or `set`, `list`, `map` or `optional` with `of=\"<type>\"`"
            )),
        }
    }

    fn typed(&self, w: &str, of: Option<&str>) -> Result<Type, String> {
        match (w, of) {
            ("set", Some(of)) => Ok(Type::Set(Box::new(self.word(of)?))),
            ("list", Some(of)) => Ok(Type::List(Box::new(self.word(of)?))),
            ("optional", Some(of)) => Ok(Type::Optional(Box::new(self.word(of)?))),
            ("map", Some(of)) if self.entity(of).is_some() => Ok(Type::Map(of.to_string())),
            ("map", Some(of)) => Err(format!(
                "`map` has `of=\"<entity>\"`, its keys are the ids of the entity, and `{of}` is no entity"
            )),
            (w, Some(_)) => Err(format!("`{w}` takes no `of`")),
            (w, None) => self.word(w),
        }
    }

    fn field(&self, t: &Type, f: &str) -> Result<Type, String> {
        let Type::Record(r) = t.plain() else {
            return Err(format!("{t} has no field `{f}`"));
        };
        let fields = self
            .records
            .get(r)
            .ok_or_else(|| format!("`{r}` is no record"))?;
        fields
            .iter()
            .find(|(n, _)| n == f)
            .map(|(_, t)| t.clone())
            .ok_or_else(|| {
                format!(
                    "`{r}` has the fields {}, and no `{f}`",
                    fields
                        .iter()
                        .map(|(n, _)| format!("`{n}`"))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            })
    }

    fn walk(&self, t: Type, path: &[String]) -> Result<Type, String> {
        path.iter().try_fold(t, |t, f| self.field(&t, f))
    }

    fn drawable(&self, t: &Type) -> bool {
        match t {
            Type::Int(Some(_)) | Type::Str | Type::Bool => true,
            Type::Id(e) => self.entity(e).is_some_and(|e| e.ids.is_some()),
            Type::Optional(t) => self.drawable(t),
            Type::Record(r) => self
                .records
                .get(r)
                .is_some_and(|all| all.iter().all(|(_, t)| self.drawable(t))),
            _ => false,
        }
    }

    pub fn schema(&self, t: &Type) -> Json {
        match t {
            Type::Int(Some((a, b))) => json!({ "type": "integer", "minimum": a, "maximum": b }),
            Type::Int(None) => json!({ "type": "integer" }),
            Type::Str | Type::Text => json!({ "type": "string" }),
            Type::Bool => json!({ "type": "boolean" }),
            Type::Id(e) => match self.ids(e) {
                Some(all) => json!({ "enum": all }),
                None => json!({ "type": "string" }),
            },
            Type::Optional(t) => json!({ "anyOf": [self.schema(t), { "type": "null" }] }),
            Type::Record(r) => {
                self.object_schema(self.records.get(r).map_or(&[][..], Vec::as_slice))
            }
            Type::Set(t) | Type::List(t) => json!({ "type": "array", "items": self.schema(t) }),
            Type::Map(e) => json!({
                "type": "object",
                "additionalProperties": self.schema(&Type::Record(e.clone())),
            }),
        }
    }

    fn object_schema(&self, fields: &[(String, Type)]) -> Json {
        json!({
            "type": "object",
            "properties": Json::from_iter(fields.iter().map(|(n, t)| (n.clone(), self.schema(t)))),
            "required": fields.iter().map(|(n, _)| n.clone()).collect::<Vec<_>>(),
            "additionalProperties": false,
        })
    }

    pub fn drawn(&self, action: &Action) -> Json {
        let fields: Vec<(String, Type)> = action
            .inputs
            .iter()
            .filter_map(|i| match &i.source {
                Source::Drawn(t) => Some((i.name.clone(), t.clone())),
                Source::Pool(..) => None,
            })
            .collect();
        self.object_schema(&fields)
    }

    pub fn admits(&self, action: &Action, input: &Json) -> bool {
        let env: Env = action
            .inputs
            .iter()
            .map(|i| {
                let value = input.get(&i.name).cloned().unwrap_or(Json::Null);
                (i.name.clone(), i.name.clone(), value)
            })
            .collect();
        action
            .when
            .as_ref()
            .is_none_or(|w| w.passes(&State::new(), &State::new(), &env))
    }

    pub fn fit(&self, t: &Type, v: &Json, at: &str) -> Result<(), String> {
        let wrong = || Err(format!("`{at}` is `{v}`, and its type is {t}"));
        match (t, v) {
            (Type::Optional(t), v) if !v.is_null() => self.fit(t, v, at),
            (Type::Int(range), Json::Number(n)) => match (n.as_i64(), range) {
                (Some(n), Some((a, b))) if n < *a || n > *b => wrong(),
                (Some(_), _) => Ok(()),
                (None, _) => wrong(),
            },
            (Type::Optional(_), Json::Null)
            | (Type::Str | Type::Text, Json::String(_))
            | (Type::Bool, Json::Bool(_)) => Ok(()),
            (Type::Id(e), Json::String(_)) => match self.ids(e) {
                Some(all) if !all.contains(v) => wrong(),
                _ => Ok(()),
            },
            (Type::Record(r), Json::Object(all)) => {
                let fields = self.records.get(r).map_or(&[][..], Vec::as_slice);
                if let Some(extra) = all.keys().find(|k| !fields.iter().any(|(n, _)| n == *k)) {
                    return Err(format!("`{at}.{extra}` is no field of the record `{r}`"));
                }
                fields.iter().try_for_each(|(n, t)| {
                    self.fit(t, all.get(n).unwrap_or(&Json::Null), &format!("{at}.{n}"))
                })
            }
            (Type::Set(t) | Type::List(t), Json::Array(all)) => all
                .iter()
                .enumerate()
                .try_for_each(|(i, v)| self.fit(t, v, &format!("{at}[{i}]"))),
            (Type::Map(e), Json::Object(all)) => all.iter().try_for_each(|(k, v)| {
                self.fit(&Type::Id(e.clone()), &json!(k), &format!("{at} key"))?;
                self.fit(&Type::Record(e.clone()), v, &format!("{at}[{k}]"))
            }),
            _ => wrong(),
        }
    }

    pub fn conforms(&self, real: &State) -> Result<(), String> {
        for o in self.objects.iter().filter(|o| !o.state.is_empty()) {
            let Some(pairs) = real.get(&o.name) else {
                continue;
            };
            if let Some(extra) = pairs.keys().find(|k| !o.state.iter().any(|(n, _)| n == *k)) {
                return Err(format!(
                    "the runner reports `{}.{extra}`, and `{}` declares no `{extra}` in `state`",
                    o.name, o.name
                ));
            }
            for (k, t) in &o.state {
                let v = pairs
                    .get(k)
                    .ok_or_else(|| format!("the runner reports no `{}.{k}`", o.name))?;
                self.fit(t, v, &format!("{}.{k}", o.name))?;
            }
        }
        Ok(())
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

fn at(src: &str, n: &KdlNode) -> At {
    let before = &src[..n.span().offset()];
    let begun = before.rfind('\n').map_or(0, |i| i + 1);
    At {
        line: before.matches('\n').count() + 1,
        column: before[begun..].chars().count() + 1,
    }
}

const RUN: &str =
    "`run` takes `cases=<count>`, `steps=<count>` and `check=\"always\"` or `check=\"sampled\"`";

#[derive(Clone, Copy)]
struct Given {
    cases: Option<u64>,
    steps: Option<u64>,
    check: Option<Check>,
}

fn count(n: &KdlNode, key: &str) -> Result<Option<u64>, String> {
    n.get(key)
        .map(|v| match v {
            KdlValue::Integer(k) => u64::try_from(*k).ok().filter(|k| *k > 0).ok_or(RUN),
            _ => Err(RUN),
        })
        .transpose()
        .map_err(String::from)
}

fn check(n: &KdlNode) -> Result<Option<Check>, String> {
    n.get("check")
        .map(|v| match v.as_string() {
            Some("always") => Ok(Check::Always),
            Some("sampled") => Ok(Check::Sampled),
            _ => Err(format!(
                "`{}` takes `check=\"always\"` or `check=\"sampled\"`",
                name(n)
            )),
        })
        .transpose()
}

fn given(n: &KdlNode) -> Result<Given, String> {
    shape(n, 0, &["cases", "steps", "check"], false)?;
    match keys(n).is_empty() {
        true => Err(RUN.into()),
        false => Ok(Given {
            cases: count(n, "cases")?,
            steps: count(n, "steps")?,
            check: check(n)?,
        }),
    }
}

fn ran(doc: &KdlDocument) -> Result<Option<&KdlNode>, String> {
    match doc
        .nodes()
        .iter()
        .filter(|n| name(n) == "run")
        .collect::<Vec<_>>()
        .as_slice()
    {
        [] => Ok(None),
        [one] => Ok(Some(one)),
        _ => Err("`run` is set twice".into()),
    }
}

impl Run {
    fn over(self, given: Given) -> Run {
        Run {
            cases: given.cases.unwrap_or(self.cases),
            steps: given.steps.unwrap_or(self.steps),
            check: given.check.unwrap_or(self.check),
        }
    }
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

const INVARIANT: &str = "`invariant { <condition> }` has one condition on the declared state; a condition is a Quint builtin `<op> <term>… { <op line>… }`: `and`, `or`, `not`, `implies`, `iff`, `eq`, `neq`, `in`, `ilt`, `ilte`, `igt`, `igte`, `iadd`, `isub`, `imul`, `idiv`, `imod`, `ipow` or `iuminus`, or `forall` and `exists` as `forall \"<name>\" in=\"<set>\" { <condition> }` over each item of a set, a list or a map; a term is `\"<object>.<key>\"` of a `state` line, `(start)\"<object>.<key>\"`, `\"<name>.<field>\"` of a bound name, `\"<map>[<id>].<field>\"`, or a value, and each comparison names at least one of them";

const INPUT: &str = "`input` is `input \"<name>\" \"<type>\"`, `input \"<name>\" <field>=\"<type>\"…`, or `input \"<name>\" from=\"<pool>\"` with `take=\"reuse\"`, `take=\"consume\"` or `take=\"maybe\"`";

const RECORD_KEYS: [&str; 3] = ["from", "take", "of"];

struct Scope<'a> {
    d: &'a Diagram,
    states: bool,
    bound: Vec<(String, Type)>,
    src: &'a str,
}

impl Scope<'_> {
    fn bound(&self, v: &str) -> Option<&Type> {
        self.bound
            .iter()
            .rev()
            .find(|(n, _)| n == v)
            .map(|(_, t)| t)
    }

    fn with(&self, v: &str, t: Type) -> Scope<'_> {
        Scope {
            d: self.d,
            states: self.states,
            bound: [self.bound.clone(), vec![(v.to_string(), t)]].concat(),
            src: self.src,
        }
    }
}

fn listed(all: &[(String, Type)]) -> String {
    all.iter()
        .map(|(n, _)| format!("`{n}`"))
        .collect::<Vec<_>>()
        .join(", ")
}

fn path(s: &str, scope: &Scope) -> Result<(Now, Type), String> {
    let parts: Vec<String> = s.split('.').map(String::from).collect();
    let head = &parts[0];
    if let Some(t) = scope.bound(head) {
        let t = scope
            .d
            .walk(t.clone(), &parts[1..])
            .map_err(|e| format!("`{s}`: {e}"))?;
        return Ok((Now::Var(head.clone(), parts[1..].to_vec()), t));
    }
    match (scope.states, scope.d.object(head)) {
        (true, Some(o)) => {
            let [_, k] = parts.as_slice() else {
                return Err(format!(
                    "`{s}` reads into a key of `{head}`; read the items of a key with `forall`, `exists` or `[<id>]`"
                ));
            };
            match o.state.iter().find(|(n, _)| n == k) {
                Some((_, t)) => Ok((Now::Key(head.clone(), k.clone()), t.clone())),
                None if o.state.is_empty() => Err(format!(
                    "`{s}` is no declared state of `{head}`, and `{head}` has no `state` block"
                )),
                None => Err(format!(
                    "`{s}` is no declared state of `{head}`; the `state` of `{head}` has {}",
                    listed(&o.state)
                )),
            }
        }
        (false, Some(_)) => Err(format!(
            "`{s}` reads the state of `{head}`, and a `where` reads only the inputs of its action"
        )),
        (_, None) if parts.len() > 1 => Err(format!(
            "`{s}` names no object of `setup` and no bound name"
        )),
        _ => Ok((Now::Value(json!(s)), Type::Text)),
    }
}

fn reference(s: &str, scope: &Scope) -> Result<(Now, Type), String> {
    let Some((base, rest)) = s.split_once('[') else {
        return path(s, scope);
    };
    let (key, after) = rest
        .split_once(']')
        .ok_or_else(|| format!("`{s}` opens `[` and closes no `]`"))?;
    let (b, bt) = path(base, scope)?;
    let Type::Map(e) = bt.plain().clone() else {
        return Err(format!("`{base}` is {bt}, and `[<id>]` reads a map"));
    };
    let (k, kt) = path(key, scope)?;
    if !kt.fits(&Type::Id(e.clone())) {
        return Err(format!(
            "`{key}` is {kt}, and the keys of `{base}` are the ids of {e}"
        ));
    }
    let fields: Vec<String> = match after {
        "" => Vec::new(),
        more => more
            .strip_prefix('.')
            .ok_or_else(|| format!("`{s}` has `.<field>` after `]`"))?
            .split('.')
            .map(String::from)
            .collect(),
    };
    let t = scope
        .d
        .walk(Type::Record(e), &fields)
        .map_err(|err| format!("`{s}`: {err}"))?;
    Ok((Now::Index(Box::new(b), Box::new(k), fields), t))
}

fn term(e: &KdlEntry, scope: &Scope) -> Result<(Now, Type), String> {
    match (e.name(), e.ty().map(kdl::KdlIdentifier::value), e.value()) {
        (None, None, KdlValue::String(s)) => reference(s, scope),
        (None, Some("start"), KdlValue::String(s)) => match reference(s, scope)? {
            (Now::Key(o, k), t) => Ok((Now::Start(o, k), t)),
            _ => Err(format!(
                "`(start)` takes `\"<object>.<key>\"`, not `{s}`: {INVARIANT}"
            )),
        },
        (None, None, v @ KdlValue::Integer(_)) => Ok((Now::Value(value(v)), Type::Int(None))),
        _ => Err(format!(
            "`{}` is not a term: {INVARIANT}",
            e.to_string().trim()
        )),
    }
}

fn each(n: &KdlNode, q: Quant, scope: &Scope) -> Result<(Now, Type), String> {
    let wrong = || {
        format!(
            "`{}` is `{} \"<name>\" in=\"<set>\" {{ <condition> }}`: {INVARIANT}",
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
    let set = n.get("in").and_then(KdlValue::as_string);
    match (named.as_slice(), set, lines(n), n.entries().len()) {
        ([v], Some(set), [one], 2) => {
            if scope.d.object(v).is_some() || scope.bound(v).is_some() {
                return Err(format!(
                    "`{} \"{v}\"` binds a name that an object or an outer name has",
                    q.name()
                ));
            }
            let (over, t) = reference(set, scope)?;
            let item = t.element().ok_or_else(|| {
                format!(
                    "`{}` reads `{set}`, which is {t}, and it reads a set, a list or a map",
                    q.name()
                )
            })?;
            let (body, bt) = condition(one, &scope.with(v, item))?;
            match bt {
                Type::Bool => Ok((
                    Now::Each(
                        q,
                        v.clone(),
                        Box::new(over),
                        Box::new(body),
                        at(scope.src, n),
                    ),
                    Type::Bool,
                )),
                _ => Err(wrong()),
            }
        }
        _ => Err(wrong()),
    }
}

fn int(t: &Type) -> bool {
    matches!(t.plain(), Type::Int(_))
}

fn condition(n: &KdlNode, scope: &Scope) -> Result<(Now, Type), String> {
    match name(n) {
        "forall" => return each(n, Quant::Forall, scope),
        "exists" => return each(n, Quant::Exists, scope),
        _ => {}
    }
    let op =
        Op::parse(name(n)).ok_or_else(|| format!("`{}` is not a builtin: {INVARIANT}", name(n)))?;
    let typed = n
        .entries()
        .iter()
        .map(|e| term(e, scope))
        .chain(lines(n).iter().map(|c| condition(c, scope)))
        .collect::<Result<Vec<_>, _>>()?;
    let (min, max) = op.arity();
    if typed.len() < min || typed.len() > max {
        return Err(format!("`{}` takes {}: {INVARIANT}", op.name(), op.takes()));
    }
    let (args, types): (Vec<Now>, Vec<Type>) = typed.into_iter().unzip();
    let shown = Now::App(op, args.clone(), at(scope.src, n)).quint();
    let op = match op {
        Op::And | Op::Or | Op::Not | Op::Implies | Op::Iff
            if types.iter().all(|t| *t == Type::Bool) =>
        {
            op
        }
        Op::And | Op::Or | Op::Not | Op::Implies | Op::Iff => {
            return Err(format!("`{shown}` takes conditions"));
        }
        Op::Eq | Op::Neq if types[0].fits(&types[1]) => op,
        Op::Eq | Op::Neq => {
            return Err(format!("`{shown}` compares {} with {}", types[0], types[1]));
        }
        Op::In => match (types.len(), types[1].element()) {
            (2, Some(item)) => {
                let item = match types[1].plain() {
                    Type::Map(e) => Type::Id(e.clone()),
                    _ => item,
                };
                match types[0].fits(&item) {
                    true => Op::Contains,
                    false => {
                        return Err(format!("`{shown}` looks for {} in {}", types[0], types[1]));
                    }
                }
            }
            _ if types[1..].iter().all(|t| types[0].fits(t)) => op,
            _ => {
                return Err(format!(
                    "`{shown}` looks for {} in values of other types",
                    types[0]
                ));
            }
        },
        _ if types.iter().all(int) => op,
        _ => return Err(format!("`{shown}` takes integers")),
    };
    let app = Now::App(op, args, at(scope.src, n));
    if op.compares() && !app.refers() {
        return Err(format!(
            "`{}` names at least one `\"<object>.<key>\"`: {INVARIANT}",
            app.quint()
        ));
    }
    let t = match op.boolean() {
        true => Type::Bool,
        false => Type::Int(None),
    };
    Ok((app, t))
}

fn invariant(n: &KdlNode, scope: &Scope, run: Run) -> Result<Invariant, String> {
    if !args(n).is_empty() {
        return Err(NAMELESS.into());
    }
    shape(n, 0, &["check"], true)?;
    let (body, t) = match lines(n) {
        [one] => condition(one, scope)?,
        _ => return Err(INVARIANT.into()),
    };
    if t != Type::Bool {
        return Err(format!(
            "`invariant` has a condition, not `{}`: {INVARIANT}",
            body.quint()
        ));
    }
    Ok(Invariant {
        body,
        at: at(scope.src, n),
        check: check(n)?.unwrap_or(run.check),
    })
}

fn declared(c: &KdlNode, d: &Diagram) -> Result<(String, Type), String> {
    let given = shape(c, 1, &["of"], false)?;
    let t = d
        .typed(&given[0], c.get("of").and_then(KdlValue::as_string))
        .map_err(|e| format!("`{}`: {e}", name(c)))?;
    Ok((name(c).to_string(), t))
}

fn unique(all: &[(String, Type)], what: &str) -> Result<(), String> {
    match all
        .iter()
        .enumerate()
        .find(|(i, (n, _))| all[..*i].iter().any(|(m, _)| m == n))
    {
        Some((_, (n, _))) => Err(format!("{what} declares `{n}` twice")),
        None => Ok(()),
    }
}

fn record_fields(c: &KdlNode) -> Vec<(String, String)> {
    c.entries()
        .iter()
        .filter_map(|e| e.name().map(|k| (k.value().to_string(), text(e.value()))))
        .filter(|(k, _)| !RECORD_KEYS.contains(&k.as_str()))
        .collect()
}

fn typed_line(c: &KdlNode, d: &Diagram) -> Result<(String, Type), String> {
    let named = args(c);
    let label = named.first().cloned().unwrap_or_default();
    match (named.len(), record_fields(c).is_empty()) {
        (2, true) => {
            shape(c, 2, &["of"], false)?;
            let t = d.typed(&named[1], c.get("of").and_then(KdlValue::as_string))?;
            Ok((label, t))
        }
        (1, false) => {
            shape(c, 1, &keys(c), false)?;
            Ok((label.clone(), Type::Record(label)))
        }
        _ => Err(INPUT.into()),
    }
}

fn input(c: &KdlNode, d: &Diagram) -> Result<Input, String> {
    let label = args(c).first().cloned().ok_or(INPUT)?;
    if let Some(pool) = c.get("from").map(text) {
        shape(c, 1, &["from", "take"], false)?;
        d.pool(&pool).ok_or_else(|| {
            format!(
                "`input \"{label}\"` draws from `{pool}`, and no object declares `pool \"{pool}\"`"
            )
        })?;
        let take = match c.get("take").map(text).as_deref() {
            None | Some("reuse") => Take::Reuse,
            Some("consume") => Take::Consume,
            Some("maybe") => Take::Maybe,
            Some(other) => {
                return Err(format!(
                    "`take` is `\"reuse\"`, `\"consume\"` or `\"maybe\"`, not `\"{other}\"`"
                ));
            }
        };
        return Ok(Input {
            name: label,
            source: Source::Pool(pool, take),
        });
    }
    let (name, t) = typed_line(c, d)?;
    if !d.drawable(&t) {
        return Err(format!(
            "`input \"{name}\"` draws {t}; weft draws an int range, a bool, a string, an entity with `ids`, and a record or an optional of them"
        ));
    }
    Ok(Input {
        name,
        source: Source::Drawn(t),
    })
}

impl Diagram {
    pub fn invariant(&self, text: &str) -> Result<Invariant, Error> {
        let doc = document(text)?;
        let scope = Scope {
            d: self,
            states: true,
            bound: Vec::new(),
            src: text,
        };
        match doc.nodes() {
            [one] if name(one) == "invariant" => invariant(one, &scope, self.run).map_err(Error),
            _ => Err(Error(INVARIANT.into())),
        }
    }

    pub fn input_type(&self, i: &Input) -> Type {
        match &i.source {
            Source::Drawn(t) => t.clone(),
            Source::Pool(p, take) => {
                let of = self.pool(p).map_or(Type::Text, |p| p.of.clone());
                match take {
                    Take::Maybe => Type::Optional(Box::new(of)),
                    _ => of,
                }
            }
        }
    }

    fn written(&self, p: &str) -> Result<Vec<String>, String> {
        let parts: Vec<String> = p.split('.').map(String::from).collect();
        let o = self
            .object(&parts[0])
            .ok_or_else(|| format!("`writes \"{p}\"` names no object of `setup`"))?;
        let key = parts
            .get(1)
            .ok_or_else(|| format!("`writes` names `\"<object>.<key>\"`, not `{p}`"))?;
        let mut t = o
            .state
            .iter()
            .find(|(n, _)| n == key)
            .map(|(_, t)| t.clone())
            .ok_or_else(|| format!("`writes \"{p}\"` names no declared state of `{}`", o.name))?;
        for f in &parts[2..] {
            let inner = t.element().unwrap_or(t);
            t = self
                .field(&inner, f)
                .map_err(|e| format!("`writes \"{p}\"`: {e}"))?;
        }
        Ok(parts)
    }
}

fn action(n: &KdlNode, object: &str, d: &Diagram, src: &str) -> Result<Action, String> {
    let verb = shape(n, 1, &[], true)?.remove(0);
    let fail = |e: String| format!("action {verb}: {e}");
    let mut a = Action {
        verb: verb.clone(),
        object: object.to_string(),
        line: at(src, n).line,
        inputs: Vec::new(),
        returns: None,
        when: None,
        adds: Vec::new(),
        writes: Vec::new(),
    };
    let mut wheres = Vec::new();
    for c in lines(n) {
        match name(c) {
            "input" => {
                let found = input(c, d).map_err(fail)?;
                if a.inputs.iter().any(|i| i.name == found.name) {
                    return Err(fail(format!("`input \"{}\"` is set twice", found.name)));
                }
                a.inputs.push(found);
            }
            "returns" if a.returns.is_some() => return Err(fail("has one `returns`".into())),
            "returns" => a.returns = Some(typed_line(c, d).map_err(fail)?),
            "where" => wheres.push(c),
            "adds" => {
                let mut given = shape(c, 2, &[], false).map_err(fail)?;
                let from = given.remove(1);
                a.adds.push((given.remove(0), from));
            }
            "writes" => {
                let given = shape(c, 1, &[], false).map_err(fail)?;
                a.writes.push(d.written(&given[0]).map_err(fail)?);
            }
            other => {
                return Err(fail(format!(
                    "has `input`, `returns`, `where`, `adds` and `writes` lines, not `{other}`"
                )));
            }
        }
    }
    let bound: Vec<(String, Type)> = a
        .inputs
        .iter()
        .map(|i| (i.name.clone(), d.input_type(i)))
        .collect();
    match wheres.as_slice() {
        [] => {}
        [w] => {
            shape(w, 0, &[], true).map_err(fail)?;
            let scope = Scope {
                d,
                states: false,
                bound: bound.clone(),
                src,
            };
            let [one] = lines(w) else {
                return Err(fail("`where { <condition> }` has one condition".into()));
            };
            match condition(one, &scope).map_err(fail)? {
                (now, Type::Bool) => a.when = Some(now),
                (now, _) => {
                    return Err(fail(format!(
                        "`where` has a condition, not `{}`",
                        now.quint()
                    )));
                }
            }
        }
        _ => return Err(fail("has one `where`".into())),
    }
    for (pool, from) in &a.adds {
        let p = d
            .pool(pool)
            .ok_or_else(|| fail(format!("`adds \"{pool}\"` names no `pool`")))?;
        let given = bound
            .iter()
            .chain(a.returns.iter())
            .find(|(n, _)| n == from)
            .map(|(_, t)| t.clone())
            .ok_or_else(|| {
                fail(format!(
                    "`adds \"{pool}\" \"{from}\"` names no `input` and no `returns`"
                ))
            })?;
        if given != p.of {
            return Err(fail(format!(
                "`adds \"{pool}\" \"{from}\"` adds {given} to a pool of {}",
                p.of
            )));
        }
    }
    Ok(a)
}

fn implemented(n: &KdlNode) -> Result<Impl, String> {
    let mut found = Impl {
        adapter: shape(n, 1, &[], true)?.remove(0),
        modules: Vec::new(),
    };
    for c in lines(n) {
        if name(c) != "module" {
            return Err(format!(
                "`impl \"{}\"` has `module \"<object>\" \"<path>\" {{ … }}` lines, not `{}`",
                found.adapter,
                name(c)
            ));
        }
        let mut given = shape(c, 2, &[], true)?;
        let (place, object) = (given.remove(1), given.remove(0));
        if found.modules.iter().any(|m| m.object == object) {
            return Err(format!("`module \"{object}\"` is set twice"));
        }
        if place.starts_with('.') && !place.starts_with("../") {
            return Err(format!(
                "`module \"{object}\"` is a file in the directory of the diagram, and a module is code of the app"
            ));
        }
        let mut m = Module {
            object,
            path: place,
            reset: None,
            state: None,
            url: None,
            actions: Vec::new(),
        };
        let lined = |m: &Module, what: &str| {
            format!(
                "`module \"{}\"` has `reset`, `state`, `url` and `action \"<verb>\" \"<function>\"` lines, {what}",
                m.object
            )
        };
        for l in lines(c) {
            match name(l) {
                part @ ("reset" | "state" | "url") => {
                    let id = shape(l, 1, &[], false)?.remove(0);
                    let slot = match part {
                        "reset" => &mut m.reset,
                        "state" => &mut m.state,
                        _ => &mut m.url,
                    };
                    if slot.replace(id).is_some() {
                        return Err(format!("`module \"{}\"` sets `{part}` twice", m.object));
                    }
                }
                "action" => {
                    let mut g = shape(l, 2, &[], false)?;
                    let id = g.remove(1);
                    let verb = g.remove(0);
                    if m.actions.iter().any(|(v, _)| *v == verb) {
                        return Err(format!(
                            "`module \"{}\"` maps `action \"{verb}\"` twice",
                            m.object
                        ));
                    }
                    m.actions.push((verb, id));
                }
                other => return Err(lined(&m, &format!("not `{other}`"))),
            }
        }
        if m.reset.is_none() && m.state.is_none() && m.url.is_none() && m.actions.is_empty() {
            return Err(lined(&m, "and names at least one function"));
        }
        found.modules.push(m);
    }
    match !found.modules.is_empty() {
        true => Ok(found),
        false => Err(format!("`impl \"{}\"` has no `module` line", found.adapter)),
    }
}

fn object(n: &KdlNode, config: &Config, d: &Diagram) -> Result<(Object, Vec<Pool>), String> {
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
    let fail = |e: String| format!("{kind} {named}: {e}");
    let mut o = Object {
        name: named.clone(),
        kind: kind.clone(),
        icon,
        line: 0,
        start: pairs(n).map_err(fail)?,
        state: Vec::new(),
        invariants: Vec::new(),
        draws: Vec::new(),
    };
    let mut pools = Vec::new();
    let mut stated = false;
    for c in n.iter_children() {
        match name(c) {
            "state" if stated => return Err(fail("has one `state`".into())),
            "state" => {
                stated = true;
                shape(c, 0, &[], true).map_err(fail)?;
                o.state = lines(c)
                    .iter()
                    .map(|l| declared(l, d))
                    .collect::<Result<_, _>>()
                    .map_err(fail)?;
                unique(&o.state, "`state`").map_err(fail)?;
            }
            "pool" => {
                let pool = shape(c, 1, &["of"], false).map_err(fail)?.remove(0);
                let of = need(c, "of").map_err(fail)?;
                pools.push(Pool {
                    name: pool,
                    object: named.clone(),
                    of: d.word(&of).map_err(fail)?,
                });
            }
            "draw" if !o.draws.is_empty() => return Err(fail("has one `draw`".into())),
            "draw" => draw(c, &[], &mut o.draws).map_err(fail)?,
            "invariant" | "action" | "target" => {}
            other => {
                return Err(fail(format!(
                    "has `state`, `pool`, `action`, `target`, `draw` and `invariant`, not `{other}`"
                )));
            }
        }
    }
    if let Some((k, v)) = o
        .start
        .iter()
        .find(|(k, _)| !o.state.iter().any(|(n, _)| n == *k))
    {
        return Err(fail(format!(
            "`{k}={v}` sets the start of `{named}.{k}`, and `state` declares no `{k}`"
        )));
    }
    if let Some((k, v, t)) = o.start.iter().find_map(|(k, v)| {
        let t = &o.state.iter().find(|(n, _)| n == k)?.1;
        d.fit(t, v, k).is_err().then_some((k, v, t))
    }) {
        return Err(fail(format!("`{k}={v}` is no value of {t}")));
    }
    Ok((o, pools))
}

fn reachable(d: &Diagram) -> Result<(), String> {
    let mut filled: HashSet<&str> = HashSet::new();
    let mut on = vec![false; d.actions.len()];
    let needs = |a: &Action| -> Vec<String> {
        a.inputs
            .iter()
            .filter_map(|i| match &i.source {
                Source::Pool(p, Take::Reuse | Take::Consume) => Some(p.clone()),
                _ => None,
            })
            .collect()
    };
    loop {
        let mut changed = false;
        for (i, a) in d.actions.iter().enumerate() {
            if on[i] || !needs(a).iter().all(|p| filled.contains(p.as_str())) {
                continue;
            }
            on[i] = true;
            changed = true;
            filled.extend(a.adds.iter().map(|(p, _)| p.as_str()));
        }
        if !changed {
            break;
        }
    }
    if let Some((a, _)) = d.actions.iter().zip(&on).find(|(_, on)| !**on) {
        let pool = needs(a)
            .into_iter()
            .find(|p| !filled.contains(p.as_str()))
            .unwrap_or_default();
        return Err(format!(
            "action `{}` draws from the pool `{pool}`, and no reachable action adds to `{pool}`",
            a.verb
        ));
    }
    for i in d.invariants() {
        for (o, k) in i.body.keys() {
            let writes = d
                .actions
                .iter()
                .any(|a| a.writes.iter().any(|w| w[0] == *o && w[1] == *k));
            if !writes {
                return Err(format!(
                    "the invariant at line {} reads `{o}.{k}`, and no action writes it; the action that changes it has `writes \"{o}.{k}\"`",
                    i.at.line
                ));
            }
        }
    }
    Ok(())
}

fn mapped(d: &Diagram) -> Result<(), String> {
    let modules: Vec<&Module> = d.impls.iter().flat_map(|i| &i.modules).collect();
    for (i, m) in modules.iter().enumerate() {
        let o = d
            .object(&m.object)
            .ok_or_else(|| format!("`module \"{}\"` names no object of `setup`", m.object))?;
        if modules[..i].iter().any(|k| k.object == m.object) {
            return Err(format!("`module \"{}\"` is set twice", m.object));
        }
        if let Some((verb, _)) = m.actions.iter().find(|(v, _)| {
            !d.actions
                .iter()
                .any(|a| a.verb == *v && a.object == m.object)
        }) {
            return Err(format!(
                "`module \"{}\"` maps `action \"{verb}\"`, and `{}` declares no `action \"{verb}\"`",
                m.object, m.object
            ));
        }
        match (m.state.is_some(), o.state.is_empty()) {
            (true, true) => {
                return Err(format!(
                    "`module \"{}\"` maps `state`, and `{}` declares no `state`",
                    m.object, m.object
                ));
            }
            (false, false) => {
                return Err(format!(
                    "`{}` declares `state`, and `module \"{}\"` maps no `state` function",
                    m.object, m.object
                ));
            }
            _ => {}
        }
    }
    for a in &d.actions {
        let found = modules
            .iter()
            .any(|m| m.object == a.object && m.actions.iter().any(|(v, _)| *v == a.verb));
        if !found {
            return Err(format!(
                "`action \"{}\"` of `{}` has no function; `module \"{}\"` maps it with `action \"{}\" \"<function>\"`",
                a.verb, a.object, a.object, a.verb
            ));
        }
    }
    if let Some(o) = d
        .objects
        .iter()
        .find(|o| !o.state.is_empty() && !modules.iter().any(|m| m.object == o.name))
    {
        return Err(format!(
            "`{}` declares `state`, and no `module \"{}\"` reads it",
            o.name, o.name
        ));
    }
    if !modules.is_empty() && !modules.iter().any(|m| m.reset.is_some()) {
        return Err("no module maps a `reset` function".into());
    }
    Ok(())
}

fn build(doc: &KdlDocument, src: &str, config: &Config) -> Result<Diagram, String> {
    let run = match ran(doc)? {
        Some(n) => config.run.over(given(n)?),
        None => config.run,
    };
    let mut d = Diagram {
        objects: Vec::new(),
        entities: Vec::new(),
        records: BTreeMap::new(),
        pools: Vec::new(),
        actions: Vec::new(),
        targets: Vec::new(),
        impls: Vec::new(),
        run,
    };
    let mut setup: Vec<&KdlNode> = Vec::new();
    let mut seen = false;
    for n in doc.nodes() {
        match name(n) {
            "run" => {}
            "impl" => {
                let found = implemented(n)?;
                if d.impls.iter().any(|i| i.adapter == found.adapter) {
                    return Err(format!("`impl \"{}\"` is set twice", found.adapter));
                }
                d.impls.push(found);
            }
            "setup" if seen => return Err("`setup` is set twice".into()),
            "setup" => {
                shape(n, 0, &[], true)?;
                seen = true;
                setup.extend(lines(n));
            }
            other => return Err(format!("`{other}` is not run, setup or impl")),
        }
    }
    let (entities, rest): (Vec<&KdlNode>, Vec<&KdlNode>) =
        setup.into_iter().partition(|c| name(c) == "entity");
    let (records, objects): (Vec<&KdlNode>, Vec<&KdlNode>) =
        rest.into_iter().partition(|c| name(c) == "record");
    for e in &entities {
        let named = shape(e, 1, &["ids"], true)?.remove(0);
        let ids = match e.get("ids") {
            Some(KdlValue::Integer(n)) if *n > 0 => Some(*n as u64),
            None => None,
            Some(_) => return Err(format!("entity {named}: `ids` is a count above 0")),
        };
        if d.entity(&named).is_some() {
            return Err(format!("`entity \"{named}\"` is declared twice"));
        }
        d.entities.push(Entity {
            name: named,
            ids,
            fields: Vec::new(),
        });
    }
    for (at, e) in entities.iter().enumerate() {
        let named = d.entities[at].name.clone();
        let fields = lines(e)
            .iter()
            .map(|c| declared(c, &d))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|err| format!("entity {named}: {err}"))?;
        unique(&fields, &format!("entity {named}"))?;
        d.entities[at].fields = fields.clone();
        d.records.insert(named, fields);
    }
    for r in &records {
        let named = shape(r, 1, &[], true)?.remove(0);
        if d.entity(&named).is_some() || d.records.contains_key(&named) {
            return Err(format!(
                "`record \"{named}\"` has the name of an entity or another record"
            ));
        }
        d.records.insert(named.clone(), Vec::new());
    }
    for r in &records {
        let named = args(r).remove(0);
        let fields = lines(r)
            .iter()
            .map(|c| declared(c, &d))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|err| format!("record {named}: {err}"))?;
        unique(&fields, &format!("record {named}"))?;
        d.records.insert(named, fields);
    }
    for o in &objects {
        for a in lines(o).iter().filter(|c| name(c) == "action") {
            for c in lines(a)
                .iter()
                .filter(|c| matches!(name(c), "input" | "returns"))
            {
                let fields = record_fields(c);
                if fields.is_empty() {
                    continue;
                }
                let rec = args(c).first().cloned().unwrap_or_default();
                if d.entity(&rec).is_some() || d.records.contains_key(&rec) {
                    return Err(format!(
                        "the record `{rec}` of `{} \"{rec}\"` has the name of an entity or another record",
                        name(c)
                    ));
                }
                let typed = fields
                    .iter()
                    .map(|(k, v)| Ok((k.clone(), d.word(v)?)))
                    .collect::<Result<Vec<_>, String>>()
                    .map_err(|e| format!("`{} \"{rec}\"`: {e}", name(c)))?;
                d.records.insert(rec, typed);
            }
        }
    }
    let mut found = Vec::new();
    for n in &objects {
        let (mut o, pools) = object(n, config, &d)?;
        o.line = at(src, n).line;
        for p in pools {
            if d.pool(&p.name).is_some() {
                return Err(format!("`pool \"{}\"` is declared twice", p.name));
            }
            d.pools.push(p);
        }
        found.push(o);
    }
    d.objects = found;
    if d.objects.is_empty() {
        return Err("no objects".into());
    }
    let mut known = HashSet::new();
    if let Some(o) = d.objects.iter().find(|o| !known.insert(o.name.as_str())) {
        return Err(format!("`{}` is declared twice", o.name));
    }
    for (at, n) in objects.iter().enumerate() {
        let (kind, o) = (d.objects[at].kind.clone(), d.objects[at].name.clone());
        for c in lines(n) {
            let fail = |e: String| format!("{kind} {o}: {e}");
            match name(c) {
                "action" => {
                    let a = action(c, &o, &d, src).map_err(fail)?;
                    if d.action(&a.verb).is_some() {
                        return Err(format!("`action \"{}\"` is declared twice", a.verb));
                    }
                    d.actions.push(a);
                }
                "target" => {
                    let given = shape(c, 1, &[], false).map_err(fail)?;
                    let scope = Scope {
                        d: &d,
                        states: true,
                        bound: Vec::new(),
                        src,
                    };
                    match reference(&given[0], &scope).map_err(fail)? {
                        (Now::Key(to, k), t) if int(&t) || t.element().is_some() => {
                            d.targets.push((to, k));
                        }
                        _ => {
                            return Err(fail(format!(
                                "`target \"{}\"` names `\"<object>.<key>\"` of an int, a set, a list or a map",
                                given[0]
                            )));
                        }
                    }
                }
                "invariant" => {
                    let scope = Scope {
                        d: &d,
                        states: true,
                        bound: Vec::new(),
                        src,
                    };
                    let i = invariant(c, &scope, run).map_err(fail)?;
                    d.objects[at].invariants.push(i);
                }
                _ => {}
            }
        }
    }
    mapped(&d)?;
    reachable(&d)?;
    let invariants = d.invariants();
    if let Some((i, j)) = invariants.iter().enumerate().find_map(|(at, i)| {
        invariants[..at]
            .iter()
            .find(|j| j.body == i.body)
            .map(|j| (i, j))
    }) {
        return Err(format!(
            "the invariant at line {} has the condition of the invariant at line {}",
            i.at.line, j.at.line
        ));
    }
    Ok(d)
}

const NAMELESS: &str =
    "`invariant` takes no name: write `invariant { <condition> }`, and the condition is the check";

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
    pub run: Run,
    pub runners: Vec<(String, Start)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Start {
    Command(String),
    Bun,
}

pub fn config(src: &str) -> Result<Config, Error> {
    let doc = document(src)?;
    let found = ran(&doc)
        .map_err(Error)?
        .map(given)
        .transpose()
        .map_err(Error)?;
    let run = match found.map(|g| (g.cases, g.steps, g.check)) {
        Some((Some(cases), Some(steps), Some(check))) => Run {
            cases,
            steps,
            check,
        },
        _ => return Err(Error(format!("needs {RUN}"))),
    };
    let mut c = Config {
        include: Vec::new(),
        styles: Vec::new(),
        adapter: Adapter::Default,
        transport: None,
        run,
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
                shape(n, 1, &[], false).map_err(Error)?;
                c.adapter = adapter;
                set = true;
            }
            "run" => {}
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
                    "`{other}` is not include, style, adapter, run, transport or runner"
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
run cases=100 steps=50 check="sampled"
style "person" icon="user"
style "api" icon="server"
style "browser" icon="app-window"
style "element" icon="square-dashed-mouse-pointer""#;

    const BASE: &str = r#"
setup {
  person "user"
  element "origin" value="San Francisco" {
    state {
      value "string"
    }
    action "type" {
      input "text" "string"
      writes "origin.value"
    }
    invariant {
      neq "origin.value" ""
    }
  }
}
impl "bun" {
  module "origin" "../src/origin.ts" {
    reset "reset"
    state "origin"
    action "type" "type"
  }
}
"#;

    const POOL: &str = r#"
setup {
  person "user"
  browser "page" count=0 opened=0 {
    state {
      count "int"
      opened "int"
      snapshot "string"
    }
    action "open" {
      writes "page.opened"
    }
    action "send" {
      input "n" "0..1000"
      writes "page.count"
    }
    invariant {
      implies {
        eq "page.opened" 0
        eq "page.count" (start)"page.count"
      }
    }
  }
}
impl "bun" {
  module "page" "../src/page.ts" {
    reset "reset"
    state "page"
    action "open" "open"
    action "send" "send"
  }
}
"#;

    const TENANCY: &str = r#"
setup {
  person "account_owner"
  entity "user" ids=3
  entity "account" ids=3 {
    members "set" of="user"
  }
  entity "conversation" ids=3 {
    account "account"
    readers "set" of="user"
  }
  api "curry" {
    state {
      accounts "map" of="account"
      conversations "map" of="conversation"
    }
    pool "seats" of="seat"
    pool "conversations" of="conversation"
    action "join_account" {
      input "seat" user="user" account="account"
      adds "seats" "seat"
      writes "curry.accounts.members"
    }
    action "start_conversation" {
      input "id" "conversation"
      input "owner" from="seats"
      input "parent" from="conversations" take="maybe"
      where {
        neq "parent" "id"
      }
      adds "conversations" "id"
      writes "curry.conversations"
    }
    invariant {
      forall "c" in="curry.conversations" {
        forall "u" in="c.readers" {
          in "u" "curry.accounts[c.account].members"
        }
      }
    }
  }
}
impl "go" {
  module "curry" "../curry.go" {
    reset "weftmodel.Reset"
    state "weftmodel.Observe"
    action "join_account" "weftmodel.JoinAccount"
    action "start_conversation" "weftmodel.StartConversation"
  }
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
    fn the_config_sets_each_value_of_a_run() {
        let run = |line: &str| {
            config(&CONFIG.replace(r#"run cases=100 steps=50 check="sampled""#, line))
                .map(|c| c.run)
                .map_err(|e| e.0)
        };
        assert_eq!(
            run(r#"run cases=5 steps=20 check="always""#),
            Ok(Run {
                cases: 5,
                steps: 20,
                check: Check::Always,
            })
        );
        assert_eq!(run(""), Err(format!("needs {RUN}")));
        assert_eq!(run("run cases=5 steps=20"), Err(format!("needs {RUN}")));
        assert_eq!(
            run(r#"run cases=5 steps=0 check="always""#),
            Err(RUN.to_string())
        );
        assert_eq!(
            run(r#"run cases=5 steps=20 check="often""#),
            Err("`run` takes `check=\"always\"` or `check=\"sampled\"`".to_string())
        );
        assert_eq!(
            run("run cases=5 steps=20 check=\"always\"\nrun cases=5 steps=20 check=\"always\""),
            Err("`run` is set twice".to_string())
        );
    }

    #[test]
    fn a_test_and_an_invariant_change_the_values_of_the_config() -> Result<(), String> {
        let config = config(CONFIG).map_err(|e| e.0)?;
        let d = parse(POOL, &config).map_err(|e| e.0)?;
        assert_eq!(d.run, config.run);
        assert_eq!(d.invariants()[0].check, Check::Sampled);
        let changed = POOL
            .replace("setup {", "run steps=5\nsetup {")
            .replace("invariant {", "invariant check=\"always\" {");
        let d = parse(&changed, &config).map_err(|e| e.0)?;
        assert_eq!(
            d.run,
            Run {
                cases: 100,
                steps: 5,
                check: Check::Sampled,
            }
        );
        assert_eq!(d.invariants()[0].check, Check::Always);
        assert_eq!(built(POOL, "setup {", "run\nsetup {"), Err(RUN.to_string()));
        Ok(())
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
                Err("`rules` is not run, setup or impl"),
            ),
            (
                "    action \"send\" \"send\"\n",
                "    action \"send\" \"send\"\n    run \"page.open\"\n",
                Err(
                    "`module \"page\"` has `reset`, `state`, `url` and `action \"<verb>\" \"<function>\"` lines, not `run`",
                ),
            ),
            (
                "module \"page\"",
                "module \"tab\"",
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
            r#"invariant { implies(eq(page.opened, 0), eq(page.count, start(page.count))) }"#
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
            Some(Failure {
                assert: Assert {
                    op: "eq",
                    at: At {
                        line: 20,
                        column: 9
                    },
                },
                evidence: vec![
                    closed.source(),
                    "eq(0, 0) = true".to_string(),
                    "eq(page.count, start(page.count)) = eq(6, 0) = false".to_string(),
                ],
            })
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
                "    invariant {\n",
                "    invariant {\n      eq \"page.count\" 0\n",
                at(INVARIANT.to_string()),
            ),
            (
                "      implies {\n        eq \"page.opened\" 0\n        eq \"page.count\" (start)\"page.count\"\n      }\n",
                "      iadd \"page.count\" 1\n",
                at(format!(
                    "`invariant` has a condition, not `iadd(page.count, 1)`: {INVARIANT}"
                )),
            ),
            (
                "    invariant {\n",
                "    invariant \"closed\" {\n",
                at(NAMELESS.to_string()),
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
        let d = parse(POOL, &config(CONFIG).unwrap()).unwrap();
        for (src, start, now, passes) in cases {
            let doc = document(src).unwrap();
            let scope = Scope {
                d: &d,
                states: true,
                bound: Vec::new(),
                src,
            };
            let body = condition(&doc.nodes()[0], &scope).unwrap().0;
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

    fn tenancy(accounts: Json, conversations: Json) -> State {
        state(
            "curry",
            &[("accounts", accounts), ("conversations", conversations)],
        )
    }

    #[test]
    fn an_invariant_reads_the_declared_state_and_names_the_witness() {
        let d = parse(TENANCY, &config(CONFIG).unwrap()).unwrap();
        let i = d.invariants()[0];
        let accounts = json!({
            "account1": { "members": ["user1"] },
            "account2": { "members": ["user2"] },
        });
        let kept = json!({
            "conversation1": { "account": "account1", "readers": ["user1"] },
            "conversation2": { "account": "account2", "readers": ["user2"] },
        });
        let leaked = json!({
            "conversation1": { "account": "account1", "readers": ["user1"] },
            "conversation2": { "account": "account2", "readers": ["user1", "user2"] },
        });
        assert_eq!(
            d.conforms(&tenancy(accounts.clone(), leaked.clone())),
            Ok(())
        );
        assert_eq!(
            i.judge(&State::new(), &tenancy(accounts.clone(), kept)),
            None
        );
        let failed = i.judge(&State::new(), &tenancy(accounts, leaked)).unwrap();
        assert_eq!(failed.assert.op, "in");
        assert_eq!(
            failed.evidence[1..],
            [
                "in(curry.conversations[conversation2].readers[user1], curry.accounts[curry.conversations[conversation2].account].members) = in(\"user1\", [\"user2\"]) = false"
                    .to_string()
            ]
        );
    }

    #[test]
    fn the_runner_reports_values_of_the_declared_types() {
        let d = parse(TENANCY, &config(CONFIG).unwrap()).unwrap();
        let conversations = json!({});
        let cases = [
            (
                json!({ "account1": { "members": [7] } }),
                Err(
                    "`curry.accounts[account1].members[0]` is `7`, and its type is user"
                        .to_string(),
                ),
            ),
            (
                json!({ "account9": { "members": [] } }),
                Err("`curry.accounts key` is `\"account9\"`, and its type is account".to_string()),
            ),
            (
                json!({ "account1": { "members": [], "admins": [] } }),
                Err(
                    "`curry.accounts[account1].admins` is no field of the record `account`"
                        .to_string(),
                ),
            ),
        ];
        for (accounts, want) in cases {
            assert_eq!(d.conforms(&tenancy(accounts, conversations.clone())), want);
        }
        let mut extra = tenancy(json!({}), json!({}));
        extra
            .get_mut("curry")
            .unwrap()
            .insert("grants".into(), json!([]));
        assert_eq!(
            d.conforms(&extra),
            Err(
                "the runner reports `curry.grants`, and `curry` declares no `grants` in `state`"
                    .to_string()
            )
        );
    }

    #[test]
    fn an_action_draws_its_inputs_from_its_types_and_its_pools() {
        let d = parse(TENANCY, &config(CONFIG).unwrap()).unwrap();
        let join = d.action("join_account").unwrap();
        assert_eq!(
            d.drawn(join),
            json!({
                "type": "object",
                "properties": { "seat": {
                    "type": "object",
                    "properties": {
                        "user": { "enum": ["user1", "user2", "user3"] },
                        "account": { "enum": ["account1", "account2", "account3"] },
                    },
                    "required": ["user", "account"],
                    "additionalProperties": false,
                } },
                "required": ["seat"],
                "additionalProperties": false,
            })
        );
        let start = d.action("start_conversation").unwrap();
        assert_eq!(
            start.inputs[1].source,
            Source::Pool("seats".into(), Take::Reuse)
        );
        assert_eq!(
            d.input_type(&start.inputs[2]),
            Type::Optional(Box::new(Type::Id("conversation".into())))
        );
        assert!(d.admits(
            start,
            &json!({ "id": "conversation2", "parent": "conversation1" })
        ));
        assert!(d.admits(start, &json!({ "id": "conversation2", "parent": null })));
        assert!(!d.admits(
            start,
            &json!({ "id": "conversation2", "parent": "conversation2" })
        ));
    }

    #[test]
    fn the_parse_fails_on_each_term_type_and_function_that_the_test_declares_nowhere() {
        let curry = |e: &str| Err(format!("api curry: {e}"));
        let cases = [
            (
                "in \"u\" \"curry.accounts[c.account].members\"",
                "eq \"curry.grants\" \"curry.grants_inside_account\"",
                curry("`curry.grants` is no declared state of `curry`; the `state` of `curry` has `accounts`, `conversations`"),
            ),
            (
                "in=\"c.readers\"",
                "in=\"c.viewers\"",
                curry("`c.viewers`: `conversation` has the fields `account`, `readers`, and no `viewers`"),
            ),
            (
                "in \"u\" \"curry.accounts[c.account].members\"",
                "eq \"u\" \"c.account\"",
                curry("`eq(u, c.account)` compares user with account"),
            ),
            (
                "curry.accounts[c.account]",
                "curry.accounts[u]",
                curry("`u` is user, and the keys of `curry.accounts` are the ids of account"),
            ),
            (
                "in \"u\" \"curry.accounts[c.account].members\"",
                "in \"u\" \"tenancy.grants\"",
                curry("`tenancy.grants` names no object of `setup` and no bound name"),
            ),
            (
                "        neq \"parent\" \"id\"",
                "        neq \"parent\" \"curry.conversations\"",
                curry("action start_conversation: `curry.conversations` reads the state of `curry`, and a `where` reads only the inputs of its action"),
            ),
            (
                "      input \"id\" \"conversation\"",
                "      input \"id\" \"set\" of=\"user\"",
                curry("action start_conversation: `input \"id\"` draws set of user; weft draws an int range, a bool, a string, an entity with `ids`, and a record or an optional of them"),
            ),
            (
                "adds \"conversations\" \"id\"",
                "adds \"conversations\" \"owner\"",
                curry("action start_conversation: `adds \"conversations\" \"owner\"` adds seat to a pool of conversation"),
            ),
            (
                "writes \"curry.conversations\"",
                "writes \"curry.conversation\"",
                curry("action start_conversation: `writes \"curry.conversation\"` names no declared state of `curry`"),
            ),
            (
                "      adds \"seats\" \"seat\"\n",
                "",
                Err("action `start_conversation` draws from the pool `seats`, and no reachable action adds to `seats`".to_string()),
            ),
            (
                "      writes \"curry.conversations\"\n",
                "",
                Err("the invariant at line 33 reads `curry.conversations`, and no action writes it; the action that changes it has `writes \"curry.conversations\"`".to_string()),
            ),
            (
                "    action \"start_conversation\" \"weftmodel.StartConversation\"\n",
                "",
                Err("`action \"start_conversation\"` of `curry` has no function; `module \"curry\"` maps it with `action \"start_conversation\" \"<function>\"`".to_string()),
            ),
            (
                "    action \"join_account\" \"weftmodel.JoinAccount\"\n",
                "    action \"join_account\" \"weftmodel.JoinAccount\"\n    action \"leave_account\" \"weftmodel.LeaveAccount\"\n",
                Err("`module \"curry\"` maps `action \"leave_account\"`, and `curry` declares no `action \"leave_account\"`".to_string()),
            ),
            (
                "    state \"weftmodel.Observe\"\n",
                "",
                Err("`curry` declares `state`, and `module \"curry\"` maps no `state` function".to_string()),
            ),
            (
                "  entity \"user\" ids=3\n",
                "",
                Err("entity account: `members`: `user` is no type; a type is `int`, `<low>..<high>`, `string`, `bool`, an `entity`, a record of an `input` or `returns`, or `set`, `list`, `map` or `optional` with `of=\"<type>\"`".to_string()),
            ),
        ];
        for (from, to, want) in cases {
            assert_eq!(built(TENANCY, from, to), want, "{to}");
        }
    }
}
