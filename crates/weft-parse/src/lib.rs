use std::collections::{BTreeMap, HashSet, VecDeque};
use std::fmt;

use kdl::KdlValue;
pub use kdl::{KdlDocument, KdlEntry, KdlNode};
use miette::SourceCode;

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Value {
    Text(String),
    Int(i128),
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Value::Text(s) if s.is_empty() => f.write_str("\"\""),
            Value::Text(s) => f.write_str(s),
            Value::Int(n) => write!(f, "{n}"),
        }
    }
}

impl Value {
    fn quint(&self) -> String {
        match self {
            Value::Text(s) => format!("{s:?}"),
            Value::Int(n) => n.to_string(),
        }
    }
}

pub type Pairs = BTreeMap<String, Value>;

pub type State = BTreeMap<String, Pairs>;

#[derive(Debug, Clone, Default)]
pub struct Diagram {
    pub objects: Vec<Object>,
    pub init: State,
    pub steps: Vec<Step>,
    pub check: Option<Check>,
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
    pub values: Vec<Value>,
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

#[derive(Debug, Clone)]
pub struct Move {
    pub verb: String,
    pub from: Pairs,
    pub to: Pairs,
    pub line: usize,
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
    pub initial: Pairs,
    pub states: Vec<Pairs>,
    pub moves: Vec<Move>,
    pub draws: Vec<Draw>,
    pub pools: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct Step {
    pub from: String,
    pub to: String,
    pub verb: String,
    pub shows: Vec<(String, String, Value)>,
    pub set: Vec<(String, Pairs)>,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum At {
    Now,
    Next,
    Start,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Field(String, At),
    Value(Value),
    Step,
    App(Op, Vec<Expr>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ty {
    Bool,
    Int,
    Text,
}

#[derive(Debug, Clone, PartialEq)]
enum Lit {
    Bool(bool),
    Value(Value),
}

#[derive(Debug, Clone)]
pub struct Check {
    pub kind: Kind,
    pub objects: Vec<String>,
    pub body: Expr,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mark {
    pub object: String,
    pub text: String,
    pub passed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    Passes,
    Fails { step: usize, evidence: Vec<String> },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error(pub String);

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

#[derive(Clone, Copy)]
struct Ctx<'a> {
    start: &'a State,
    cur: &'a State,
    nxt: &'a State,
    verb: &'a str,
}

fn list(names: &[String], word: &str) -> String {
    match names.split_last() {
        Some((last, rest)) if !rest.is_empty() => format!("{} {word} {last}", rest.join(", ")),
        _ => names.concat(),
    }
}

impl fmt::Display for Ty {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(match self {
            Ty::Bool => "boolean",
            Ty::Int => "integer",
            Ty::Text => "string",
        })
    }
}

impl Ty {
    fn of(v: &Value) -> Ty {
        match v {
            Value::Text(_) => Ty::Text,
            Value::Int(_) => Ty::Int,
        }
    }
}

impl Lit {
    fn quint(&self) -> String {
        match self {
            Lit::Bool(b) => b.to_string(),
            Lit::Value(v) => v.quint(),
        }
    }

    fn int(n: Option<i128>) -> Option<Lit> {
        n.map(|n| Lit::Value(Value::Int(n)))
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

    fn word(self) -> &'static str {
        match self {
            Op::And => "and",
            Op::Or => "or",
            Op::Not => "not",
            Op::Implies => "implies",
            Op::Iff => "if and only if",
            Op::Eq => "equals",
            Op::Neq => "does not equal",
            Op::In => "is",
            Op::Ilt => "is less than",
            Op::Ilte => "is at most",
            Op::Igt => "is more than",
            Op::Igte => "is at least",
            Op::Iadd => "+",
            Op::Isub => "-",
            Op::Imul => "*",
            Op::Idiv => "/",
            Op::Imod => "mod",
            Op::Ipow => "^",
            Op::Iuminus => "-",
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

    fn ty(self, args: &[Ty]) -> Option<Ty> {
        let all = |t: Ty| args.iter().all(|a| *a == t);
        match (self, args) {
            (Op::Eq | Op::Neq | Op::In, [t, rest @ ..]) if rest.iter().all(|r| r == t) => {
                Some(Ty::Bool)
            }
            (Op::Ilt | Op::Ilte | Op::Igt | Op::Igte, _) if all(Ty::Int) => Some(Ty::Bool),
            (op, _) if op.boolean() && !op.compares() && all(Ty::Bool) => Some(Ty::Bool),
            (op, _) if !op.boolean() && all(Ty::Int) => Some(Ty::Int),
            _ => None,
        }
    }

    fn apply(self, args: &[Lit]) -> Option<Lit> {
        let ints: Option<Vec<i128>> = args
            .iter()
            .map(|a| match a {
                Lit::Value(Value::Int(n)) => Some(*n),
                _ => None,
            })
            .collect();
        match (self, args) {
            (Op::Not, [Lit::Bool(a)]) => Some(Lit::Bool(!a)),
            (Op::Iff, [Lit::Bool(a), Lit::Bool(b)]) => Some(Lit::Bool(a == b)),
            (Op::Eq, [a, b]) => Some(Lit::Bool(a == b)),
            (Op::Neq, [a, b]) => Some(Lit::Bool(a != b)),
            (Op::In, [a, rest @ ..]) => Some(Lit::Bool(rest.contains(a))),
            _ => self.compute(&ints?),
        }
    }

    fn compute(self, n: &[i128]) -> Option<Lit> {
        match (self, n) {
            (Op::Ilt, [a, b]) => Some(Lit::Bool(a < b)),
            (Op::Ilte, [a, b]) => Some(Lit::Bool(a <= b)),
            (Op::Igt, [a, b]) => Some(Lit::Bool(a > b)),
            (Op::Igte, [a, b]) => Some(Lit::Bool(a >= b)),
            (Op::Iadd, [a, b]) => Lit::int(a.checked_add(*b)),
            (Op::Isub, [a, b]) => Lit::int(a.checked_sub(*b)),
            (Op::Imul, [a, b]) => Lit::int(a.checked_mul(*b)),
            (Op::Idiv, [a, b]) => Lit::int(a.checked_div(*b)),
            (Op::Imod, [a, b]) => Lit::int(a.checked_rem(*b)),
            (Op::Ipow, [a, b]) if (*a, *b) != (0, 0) => {
                Lit::int(u32::try_from(*b).ok().and_then(|e| a.checked_pow(e)))
            }
            (Op::Iuminus, [a]) => Lit::int(a.checked_neg()),
            _ => None,
        }
    }
}

impl Expr {
    fn leaf(&self, o: &str) -> Option<String> {
        match self {
            Expr::Field(k, At::Now) => Some(format!("{o}.{k}")),
            Expr::Field(k, At::Next) => Some(format!("next({o}.{k})")),
            Expr::Field(k, At::Start) => Some(format!("start({o}.{k})")),
            Expr::Value(v) => Some(v.quint()),
            Expr::Step => Some("step".into()),
            Expr::App(..) => None,
        }
    }

    fn show(&self, part: &dyn Fn(&Expr) -> Option<String>) -> String {
        let each = |args: &[Expr]| {
            args.iter()
                .map(|e| e.show(part))
                .collect::<Vec<_>>()
                .join(", ")
        };
        match (part(self), self) {
            (Some(s), _) => s,
            (None, Expr::App(Op::In, args)) => {
                format!("in({}, Set({}))", args[0].show(part), each(&args[1..]))
            }
            (None, Expr::App(op, args)) => format!("{}({})", op.name(), each(args)),
            (None, _) => String::new(),
        }
    }

    fn quint(&self, o: &str) -> String {
        self.show(&|e| e.leaf(o))
    }

    fn valued(&self, c: &Ctx, o: &str) -> String {
        self.show(&|e| match e {
            Expr::App(op, _) if op.boolean() => None,
            e => Some(e.eval(c, o).map_or(String::new(), |v| v.quint())),
        })
    }

    fn english(&self, o: &str) -> String {
        let each = |args: &[Expr], by: &str| {
            args.iter()
                .map(|e| e.english(o))
                .collect::<Vec<_>>()
                .join(by)
        };
        let member = |all: &[Expr], word: &str| {
            let set: Vec<String> = all[1..]
                .iter()
                .map(|e| match e {
                    Expr::Value(v) => v.to_string(),
                    e => e.english(o),
                })
                .collect();
            format!("{} {word} {}", all[0].english(o), list(&set, "or"))
        };
        match self {
            Expr::Field(k, At::Next) => format!("the next {o}.{k}"),
            Expr::Field(k, At::Start) => format!("the start {o}.{k}"),
            Expr::App(op @ (Op::And | Op::Or), all) => each(all, &format!(", {} ", op.word())),
            Expr::App(Op::Implies, all) => format!("if {}", each(all, ", then ")),
            Expr::App(Op::In, all) => member(all, "is"),
            Expr::App(Op::Not, all) => match &all[0] {
                Expr::App(Op::In, inner) => member(inner, "is not"),
                Expr::App(Op::Eq, inner) => each(inner, " does not equal "),
                e => format!("not {}", e.english(o)),
            },
            Expr::App(Op::Eq, all) => match all.as_slice() {
                [Expr::Field(k, At::Next), Expr::Field(j, At::Now)] if k == j => {
                    format!("{o}.{k} stays unchanged")
                }
                [Expr::Field(k, At::Next), b] => format!("{o}.{k} becomes {}", b.english(o)),
                _ => each(all, " equals "),
            },
            Expr::App(op, all) if all.len() == 1 => format!("{}{}", op.word(), all[0].english(o)),
            Expr::App(op, all) => each(all, &format!(" {} ", op.word())),
            e => e.leaf(o).unwrap_or_default(),
        }
    }

    fn eval(&self, c: &Ctx, o: &str) -> Option<Lit> {
        let first = |all: &[Expr], stop: bool| {
            all.iter()
                .map(|e| e.truth(c, o))
                .find(|t| *t != Some(!stop))
                .unwrap_or(Some(!stop))
                .map(Lit::Bool)
        };
        match self {
            Expr::Field(k, at) => match at {
                At::Now => c.cur,
                At::Next => c.nxt,
                At::Start => c.start,
            }
            .get(o)?
            .get(k)
            .cloned()
            .map(Lit::Value),
            Expr::Value(v) => Some(Lit::Value(v.clone())),
            Expr::Step => Some(Lit::Value(Value::Text(c.verb.to_string()))),
            Expr::App(Op::And, all) => first(all, false),
            Expr::App(Op::Or, all) => first(all, true),
            Expr::App(Op::Implies, all) => match all[0].truth(c, o)? {
                false => Some(Lit::Bool(true)),
                true => all[1].eval(c, o),
            },
            Expr::App(op, all) => op.apply(
                &all.iter()
                    .map(|e| e.eval(c, o))
                    .collect::<Option<Vec<_>>>()?,
            ),
        }
    }

    fn truth(&self, c: &Ctx, o: &str) -> Option<bool> {
        match self.eval(c, o)? {
            Lit::Bool(b) => Some(b),
            Lit::Value(_) => None,
        }
    }

    fn passes(&self, c: &Ctx, o: &str) -> bool {
        self.truth(c, o) == Some(true)
    }

    fn guarded(&self, c: &Ctx, o: &str) -> bool {
        match self {
            Expr::App(Op::And | Op::Or, all) => all.iter().any(|e| e.guarded(c, o)),
            Expr::App(Op::Implies, all) => all[0].passes(c, o),
            Expr::App(Op::In, all) if matches!(all[0], Expr::Step) => self.passes(c, o),
            _ => true,
        }
    }

    fn failed(&self, c: &Ctx, o: &str) -> &Expr {
        match self {
            Expr::App(Op::And, all) => all
                .iter()
                .find(|e| !e.passes(c, o))
                .map_or(self, |e| e.failed(c, o)),
            Expr::App(Op::Implies, all) => all[1].failed(c, o),
            e => e,
        }
    }

    fn evidence(&self, c: &Ctx, bound: &str, o: &str) -> Vec<String> {
        match self {
            Expr::App(Op::And, all) => all
                .iter()
                .find(|e| !e.passes(c, o))
                .map_or_else(Vec::new, |e| e.evidence(c, bound, o)),
            Expr::App(Op::Implies, all) => [
                vec![format!("{} = true", all[0].valued(c, o))],
                all[1].evidence(c, bound, o),
            ]
            .concat(),
            e => vec![format!("{} = {} = false", e.quint(bound), e.valued(c, o))],
        }
    }

    fn all(&self) -> Vec<&Expr> {
        match self {
            Expr::App(_, args) => [vec![self], args.iter().flat_map(Expr::all).collect()].concat(),
            e => vec![e],
        }
    }

    fn reads(&self) -> bool {
        self.all()
            .iter()
            .any(|e| matches!(e, Expr::Field(..) | Expr::Step))
    }

    fn steps(&self) -> Vec<&[Expr]> {
        self.all()
            .into_iter()
            .filter_map(|e| match e {
                Expr::App(Op::In, all) if matches!(all[0], Expr::Step) => Some(&all[1..]),
                _ => None,
            })
            .collect()
    }

    fn fields(&self) -> Vec<(&String, At)> {
        self.all()
            .into_iter()
            .filter_map(|e| match e {
                Expr::Field(k, at) => Some((k, *at)),
                _ => None,
            })
            .collect()
    }

    fn ty(&self, x: &Object, bound: &str) -> Result<Ty, String> {
        match self {
            Expr::Field(k, _) => x
                .initial
                .get(k)
                .map(Ty::of)
                .ok_or_else(|| format!("`{}` has no `{k}`", x.name)),
            Expr::Value(v) => Ok(Ty::of(v)),
            Expr::Step => Ok(Ty::Text),
            Expr::App(op, args) => {
                let tys = args
                    .iter()
                    .map(|e| e.ty(x, bound))
                    .collect::<Result<Vec<_>, _>>()?;
                op.ty(&tys).ok_or_else(|| {
                    let names: Vec<String> = tys.iter().map(Ty::to_string).collect();
                    format!(
                        "`{}` takes {}, not {}",
                        self.quint(bound),
                        op.takes(),
                        list(&names, "and")
                    )
                })
            }
        }
    }
}

fn joined(p: &Pairs, pair: impl Fn(&str, &Value) -> String, by: &str) -> String {
    p.iter()
        .map(|(k, v)| pair(k, v))
        .collect::<Vec<_>>()
        .join(by)
}

pub fn shown(p: &Pairs) -> String {
    joined(p, |k, v| format!("{k} = {v}"), ", ")
}

fn written(p: &Pairs) -> String {
    joined(p, |k, v| format!("{k}={}", v.quint()), " ")
}

impl Check {
    fn stepped(&self) -> bool {
        self.body.fields().iter().any(|(_, at)| *at == At::Next)
    }

    fn now<'a>(&self, c: &Ctx<'a>) -> Ctx<'a> {
        Ctx {
            cur: if self.stepped() { c.cur } else { c.nxt },
            ..*c
        }
    }

    fn broken(&self, c: &Ctx) -> Option<&String> {
        let now = self.now(c);
        self.objects.iter().find(|o| !self.body.passes(&now, o))
    }

    fn evidence(&self, c: &Ctx, o: &str) -> Vec<String> {
        [
            vec![format!("{} = {o}", self.kind)],
            self.body.evidence(&self.now(c), &self.kind.to_string(), o),
        ]
        .concat()
    }

    fn mark(&self, c: &Ctx, o: &str) -> Option<Mark> {
        let now = self.now(c);
        let passed = self.body.passes(&now, o);
        let state = c.nxt.get(o).map_or(String::new(), shown);
        self.body.guarded(&now, o).then(|| Mark {
            object: o.to_string(),
            text: match passed {
                true => state,
                false => format!(
                    "{state}\n{} = false",
                    self.body.failed(&now, o).valued(&now, o)
                ),
            },
            passed,
        })
    }

    pub fn judge(
        &self,
        start: &State,
        cur: &State,
        nxt: &State,
        verb: &str,
    ) -> Option<Vec<String>> {
        let c = Ctx {
            start,
            cur,
            nxt,
            verb,
        };
        self.broken(&c).map(|o| self.evidence(&c, o))
    }

    pub fn applies(&self, start: &State, cur: &State, nxt: &State, verb: &str) -> bool {
        let c = Ctx {
            start,
            cur,
            nxt,
            verb,
        };
        let now = self.now(&c);
        self.objects.iter().any(|o| self.body.guarded(&now, o))
    }

    pub fn reads(&self) -> Vec<(&String, &String)> {
        let mut out: Vec<(&String, &String)> = Vec::new();
        for o in &self.objects {
            for (k, _) in self.body.fields() {
                if !out.contains(&(o, k)) {
                    out.push((o, k));
                }
            }
        }
        out
    }

    pub fn source(&self) -> String {
        let k = self.kind.to_string();
        format!(
            "always(forall(Set({}), {k} => {}))",
            self.objects.join(", "),
            self.body.quint(&k)
        )
    }

    pub fn english(&self) -> String {
        let k = self.kind.to_string();
        format!(
            "Always, for each {k} in {}, {}",
            list(&self.objects, "and"),
            self.body.english(&k)
        )
    }
}

impl Diagram {
    fn object(&self, name: &str) -> Option<&Object> {
        self.objects.iter().find(|o| o.name == name)
    }

    pub fn enabled(&self, now: &State) -> Vec<(&Object, &str, Pairs)> {
        let mut out: Vec<(&Object, &str, Pairs)> = Vec::new();
        for o in &self.objects {
            for m in &o.moves {
                if out.iter().any(|(x, v, _)| x.name == o.name && *v == m.verb) {
                    continue;
                }
                if let Ok(p) = moved(o, now, &m.verb) {
                    out.push((o, &m.verb, p));
                }
            }
        }
        out
    }

    pub fn initial(&self, name: &str) -> Option<String> {
        self.init.get(name).map(shown)
    }

    fn walk(&self) -> Vec<(&Step, State, State)> {
        let mut now = self.init.clone();
        self.steps
            .iter()
            .map(|s| {
                let mut next = now.clone();
                next.extend(s.set.iter().cloned());
                (s, std::mem::replace(&mut now, next.clone()), next)
            })
            .collect()
    }

    pub fn checks(&self) -> Vec<(usize, Mark)> {
        let Some(c) = &self.check else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for (k, (s, cur, nxt)) in self.walk().iter().enumerate() {
            let ctx = Ctx {
                start: &self.init,
                cur,
                nxt,
                verb: &s.verb,
            };
            out.extend(
                c.objects
                    .iter()
                    .filter_map(|o| c.mark(&ctx, o))
                    .map(|m| (k, m)),
            );
        }
        out
    }

    pub fn verdict(&self) -> Option<Verdict> {
        let c = self.check.as_ref()?;
        for (step, (s, cur, nxt)) in self.walk().iter().enumerate() {
            let ctx = Ctx {
                start: &self.init,
                cur,
                nxt,
                verb: &s.verb,
            };
            if let Some(o) = c.broken(&ctx) {
                return Some(Verdict::Fails {
                    step,
                    evidence: c.evidence(&ctx, o),
                });
            }
        }
        Some(Verdict::Passes)
    }
}

impl Object {
    fn overlay(&self, base: &Pairs, given: &Pairs) -> Result<Pairs, String> {
        let mut out = base.clone();
        for (k, v) in given {
            match out.get_mut(k) {
                Some(slot) if std::mem::discriminant(slot) == std::mem::discriminant(v) => {
                    v.clone_into(slot)
                }
                Some(slot) => {
                    return Err(format!(
                        "`{k}={}` of `{}` is not of the type of `{k}={}`",
                        v.quint(),
                        self.name,
                        slot.quint()
                    ));
                }
                None => return Err(format!("`{}` has no `{k}`", self.name)),
            }
        }
        Ok(out)
    }

    fn validate(&self) -> Result<(), String> {
        let o = &self.name;
        for (i, p) in self.states.iter().enumerate() {
            if self.states[..i].contains(p) {
                return Err(format!("`{o}` declares the state `{}` twice", written(p)));
            }
        }
        if !self.initial.is_empty() && !self.states.contains(&self.initial) {
            return Err(format!(
                "the start state `{}` of `{o}` is not one of its states",
                written(&self.initial)
            ));
        }
        for (k, m) in self.moves.iter().enumerate() {
            if !self.states.contains(&m.to) {
                return Err(format!(
                    "the state `{}` of `{o}` has `on {}`, and `{}` is not a state of `{o}`",
                    written(&m.from),
                    m.verb,
                    written(&m.to)
                ));
            }
            if self.moves[..k]
                .iter()
                .any(|p| p.verb == m.verb && p.from == m.from)
            {
                return Err(format!(
                    "`{o}` has two `on {}` lines in the state `{}`",
                    m.verb,
                    written(&m.from)
                ));
            }
        }
        Ok(())
    }

    fn reached(&self, init: &State, steps: &[Step]) -> Result<(), String> {
        let o = &self.name;
        let sets: Vec<&Pairs> = steps
            .iter()
            .flat_map(|s| &s.set)
            .filter(|(x, _)| x == o)
            .map(|(_, p)| p)
            .collect();
        if sets.is_empty() && !steps.iter().any(|s| &s.from == o) {
            return Err(format!("no `action` line reaches `{o}`"));
        }
        match self
            .states
            .iter()
            .find(|p| init.get(o) != Some(p) && !sets.contains(p))
        {
            Some(p) => Err(format!(
                "no `action` line reaches the state `{}` of `{o}`",
                written(p)
            )),
            None => Ok(()),
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

fn value(v: &KdlValue) -> Value {
    match v {
        KdlValue::Integer(n) => Value::Int(*n),
        other => Value::Text(text(other)),
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

fn state(n: &KdlNode, o: &mut Object, src: &str) -> Result<(), String> {
    shape(n, 0, &keys(n), true)?;
    let full = pairs(n)?;
    if let Some(k) = o.initial.keys().find(|k| !full.contains_key(*k)) {
        return Err(format!("`state {}` sets no `{k}`", written(&full)));
    }
    o.overlay(&o.initial, &full)?;
    for c in n.iter_children() {
        match name(c) {
            "on" => {
                let m = on(c, o, &full, src)?;
                o.moves.push(m);
            }
            other => {
                return Err(format!(
                    "`state {}` has `on`, not `{other}`",
                    written(&full)
                ));
            }
        }
    }
    o.states.push(full);
    Ok(())
}

fn on(n: &KdlNode, o: &Object, from: &Pairs, src: &str) -> Result<Move, String> {
    let verb = shape(n, 1, &keys(n), false)?.remove(0);
    let given = pairs(n)?;
    if given.is_empty() {
        return Err(format!("`on {verb}` takes `<key>=<value>`"));
    }
    Ok(Move {
        to: o.overlay(from, &given)?,
        verb,
        from: from.clone(),
        line: line(src, n),
    })
}

fn object(n: &KdlNode, config: &Config, src: &str) -> Result<Object, String> {
    let kind = Kind::parse(name(n))?;
    let icon = config
        .styles
        .iter()
        .find(|(k, _)| *k == kind)
        .map(|(_, i)| i.clone())
        .ok_or_else(|| format!("`{kind}` has no `style {kind}` in the config"))?;
    let named = shape(n, 1, &keys(n), true)
        .map_err(|e| format!("{kind}: {e}"))?
        .remove(0);
    let at = |e: String| format!("{kind} {named}: {e}");
    let mut o = Object {
        name: named.clone(),
        kind,
        icon,
        line: line(src, n),
        initial: pairs(n).map_err(at)?,
        states: Vec::new(),
        moves: Vec::new(),
        draws: Vec::new(),
        pools: Vec::new(),
    };
    let stateless = !n.iter_children().any(|c| name(c) == "state");
    match (kind, o.initial.is_empty(), stateless) {
        (_, true, true) => {}
        (Kind::Person, _, _) => {
            return Err(format!("`{named}` is a person, and a person has no states"));
        }
        (_, true, false) => {
            return Err(format!(
                "`{named}` has states and no `<key>=<value>` on its line"
            ));
        }
        (_, false, true) => {
            return Err(format!(
                "`{named}` sets `{}` and has no states",
                written(&o.initial)
            ));
        }
        (_, false, false) => {}
    }
    for c in n.iter_children() {
        match name(c) {
            "state" => state(c, &mut o, src),
            "draw" if !o.draws.is_empty() => Err("has one `draw`".into()),
            "draw" => draw(c, &[], &mut o.draws),
            "pool" => shape(c, 1, &[], false).map(|mut given| o.pools.push(given.remove(0))),
            other => Err(format!("has `state`, `draw` and `pool`, not `{other}`")),
        }
        .map_err(at)?;
    }
    if let Some(pool) = o
        .pools
        .iter()
        .find(|pool| matches!(o.initial.get(*pool), Some(Value::Text(_))))
    {
        return Err(at(format!(
            "`{pool}` is the count of the values of `pool {pool}`, and a count is an integer"
        )));
    }
    Ok(o)
}

fn step(n: &KdlNode) -> Result<Step, String> {
    shape(n, 0, &keys(n), false)?;
    let mut shows = Vec::new();
    for (k, v) in pairs(n)?
        .into_iter()
        .filter(|(k, _)| k != "from" && k != "to")
    {
        let (o, key) = k
            .rsplit_once('.')
            .ok_or_else(|| format!("`{k}={}` is not `<object>.<key>=<value>`", v.quint()))?;
        shows.push((o.to_string(), key.to_string(), v));
    }
    Ok(Step {
        from: need(n, "from")?,
        to: need(n, "to")?,
        verb: name(n).to_string(),
        shows,
        set: Vec::new(),
    })
}

const DRAW: &str = "`draw` has `<key> <value>…` lines; a line with `{ }` has the lines of the keys of its part, and no values";

fn draw(n: &KdlNode, path: &[String], out: &mut Vec<Draw>) -> Result<(), String> {
    if n.entries()
        .iter()
        .any(|e| e.name().is_some() || e.ty().is_some())
    {
        return Err(DRAW.into());
    }
    let values: Vec<Value> = n.entries().iter().map(|e| value(e.value())).collect();
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
            value.quint()
        ));
    }
    match (path.is_empty(), values.is_empty(), keys.is_empty()) {
        (true, true, false) => {}
        (false, true, false) => out.push(Draw {
            path: path.to_vec(),
            values: keys.iter().cloned().map(Value::Text).collect(),
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
            "`{}` takes `<module>.<export>`, not `{text}`",
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
                    return Err(format!("`module {module}` is set twice"));
                }
                if place.starts_with('.') && !place.starts_with("../") {
                    return Err(format!(
                        "`module {module}` is a file in the directory of the diagram, and a module is code of the app"
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
            "`{unknown}` names `{}`, and `impl {}` has no `module \"{}\"`",
            unknown.module, found.adapter, unknown.module
        ));
    }
    if let Some((module, _)) = found
        .modules
        .iter()
        .find(|(module, _)| !names.iter().any(|n| n.module == *module))
    {
        return Err(format!(
            "no line of `impl {}` names `module \"{module}\"`",
            found.adapter
        ));
    }
    match found.uses.iter().any(|u| u.role != Role::Start) {
        true => Ok(found),
        false => Err(format!(
            "`impl {}` has no `read` line and no `run` line",
            found.adapter
        )),
    }
}

const CHECK: &str = "`invariant` has `forall \"<kind>\" \"<object>\"… { <condition> }`; a condition is a Quint builtin `<op> <term>… { <op line>… }`: `and`, `or`, `not`, `implies`, `iff`, `eq`, `neq`, `in`, `ilt`, `ilte`, `igt`, `igte`, `iadd`, `isub`, `imul`, `idiv`, `imod`, `ipow` or `iuminus`; a term is `\"<kind>.<key>\"`, `(next)\"<kind>.<key>\"`, `(start)\"<kind>.<key>\"`, `\"step\"`, or a value, and each comparison names at least one `\"<kind>.<key>\"` or `\"step\"`";

fn only<'a>(n: &'a KdlNode, node: &str) -> Result<&'a KdlNode, String> {
    match lines(n) {
        [one] if name(one) == node => Ok(one),
        _ => Err(format!("`{}` has one `{node}`: {CHECK}", name(n))),
    }
}

fn plain(n: &KdlNode) -> Result<Vec<String>, String> {
    n.entries()
        .iter()
        .map(|e| match (e.name(), e.ty(), e.value()) {
            (None, None, KdlValue::String(s)) => Ok(s.clone()),
            _ => Err(format!(
                "`{}` takes plain names, not `{}`: {CHECK}",
                name(n),
                e.to_string().trim()
            )),
        })
        .collect()
}

fn term(e: &KdlEntry, bound: &str) -> Result<Expr, String> {
    let field = |s: &str| {
        s.strip_prefix(bound)?
            .strip_prefix('.')
            .filter(|k| !k.is_empty() && !k.contains('.'))
            .map(String::from)
    };
    match (e.name(), e.ty().map(|t| t.value()), e.value()) {
        (None, None, KdlValue::String(s)) if s == "step" => Ok(Expr::Step),
        (None, None, KdlValue::String(s)) => Ok(field(s).map_or_else(
            || Expr::Value(Value::Text(s.clone())),
            |k| Expr::Field(k, At::Now),
        )),
        (None, Some(at @ ("next" | "start")), KdlValue::String(s)) => field(s)
            .map(|k| {
                Expr::Field(
                    k,
                    match at {
                        "next" => At::Next,
                        _ => At::Start,
                    },
                )
            })
            .ok_or_else(|| format!("`({at})` takes `{bound}.<key>`, not `{s}`: {CHECK}")),
        (None, None, KdlValue::Integer(n)) => Ok(Expr::Value(Value::Int(*n))),
        _ => Err(format!("`{}` is not a term: {CHECK}", e.to_string().trim())),
    }
}

fn expr(n: &KdlNode, bound: &str) -> Result<Expr, String> {
    let op =
        Op::parse(name(n)).ok_or_else(|| format!("`{}` is not a builtin: {CHECK}", name(n)))?;
    let args = n
        .entries()
        .iter()
        .map(|e| term(e, bound))
        .chain(lines(n).iter().map(|c| expr(c, bound)))
        .collect::<Result<Vec<_>, _>>()?;
    let (min, max) = op.arity();
    let app = Expr::App(op, args);
    match &app {
        Expr::App(_, args) if args.len() < min || args.len() > max => {
            Err(format!("`{}` takes {}: {CHECK}", op.name(), op.takes()))
        }
        _ if op.compares() && !app.reads() => Err(format!(
            "`{}` names at least one `{bound}.<key>` or `step`: {CHECK}",
            app.quint(bound)
        )),
        _ => Ok(app),
    }
}

fn invariant(n: &KdlNode) -> Result<Check, String> {
    shape(n, 0, &[], true)?;
    let forall = only(n, "forall")?;
    let (bound, objects) = match plain(forall)?.split_first() {
        Some((v, all)) if !all.is_empty() => (v.clone(), all.to_vec()),
        _ => {
            return Err(format!(
                "`forall` takes a name and one or more objects: {CHECK}"
            ));
        }
    };
    let body = match lines(forall) {
        [one] => expr(one, &bound)?,
        _ => return Err(format!("`forall` has one condition: {CHECK}")),
    };
    Ok(Check {
        kind: Kind::parse(&bound)?,
        objects,
        body,
    })
}

fn checked(d: &Diagram, c: &Check) -> Result<(), String> {
    let verbs: Vec<&str> = d
        .objects
        .iter()
        .flat_map(|o| &o.moves)
        .map(|m| m.verb.as_str())
        .collect();
    for steps in c.body.steps() {
        for (i, e) in steps.iter().enumerate() {
            let Expr::Value(Value::Text(s)) = e else {
                return Err(format!(
                    "`in step` takes verbs, not `{}`",
                    e.quint(&c.kind.to_string())
                ));
            };
            if steps[..i].contains(e) {
                return Err(format!("`in step` names `{s}` twice"));
            }
            if !verbs.contains(&s.as_str()) {
                return Err(format!("`{s}` is the verb of no `on` line"));
            }
        }
    }
    for (i, o) in c.objects.iter().enumerate() {
        let x = d
            .object(o)
            .ok_or_else(|| format!("`forall` names unknown object `{o}`"))?;
        if c.objects[..i].contains(o) {
            return Err(format!("`forall` names `{o}` twice"));
        }
        match (x.kind == c.kind, x.states.is_empty()) {
            (false, _) => {
                return Err(format!(
                    "`forall {}` takes the kind of its objects, and `{o}` is of kind `{}`",
                    c.kind, x.kind
                ));
            }
            (true, true) => return Err(format!("`{o}` has no states")),
            (true, false) => {}
        }
        let bound = c.kind.to_string();
        let ty = c.body.ty(x, &bound)?;
        if ty != Ty::Bool {
            return Err(format!(
                "`forall` has a condition, not `{}` of type {ty}",
                c.body.quint(&bound)
            ));
        }
        for e in c.body.all() {
            let Expr::App(Op::Eq | Op::Neq | Op::In, args) = e else {
                continue;
            };
            for a in args {
                for b in args {
                    let (Expr::Field(k, _), Expr::Value(v)) = (a, b) else {
                        continue;
                    };
                    if !x.states.iter().any(|s| s.get(k) == Some(v)) {
                        return Err(format!("`{k}={}` is in no state of `{o}`", v.quint()));
                    }
                }
            }
        }
    }
    Ok(())
}

fn build(doc: &KdlDocument, src: &str, config: &Config) -> Result<Diagram, String> {
    let mut d = Diagram::default();
    let mut seen = HashSet::new();
    for n in doc.nodes() {
        let parent = name(n);
        if parent == "impl" {
            let found = implemented(n)?;
            if d.impls.iter().any(|i| i.adapter == found.adapter) {
                return Err(format!("`impl {}` is set twice", found.adapter));
            }
            d.impls.push(found);
            continue;
        }
        let lines = match parent {
            "setup" => "objects",
            "action" => "`<verb> from=\"<object>\" to=\"<object>\"` lines",
            "check" => "one `invariant`",
            other => return Err(format!("`{other}` is not setup, action, check or impl")),
        };
        shape(n, 0, &[], true)?;
        if !seen.insert(parent) {
            return Err(format!("`{parent}` is set twice"));
        }
        for c in n.iter_children() {
            match (parent, name(c)) {
                ("setup", _) => d.objects.push(object(c, config, src)?),
                ("action", _) => d.steps.push(step(c)?),
                ("check", "invariant") if d.check.is_none() => d.check = Some(invariant(c)?),
                ("check", "invariant") => return Err("`check` has one `invariant`".into()),
                _ => return Err(format!("`{parent}` has {lines}")),
            }
        }
    }
    if d.objects.is_empty() {
        return Err("no objects".into());
    }
    let pools: Vec<&String> = d.objects.iter().flat_map(|o| &o.pools).collect();
    if let Some(pool) = pools
        .iter()
        .enumerate()
        .find_map(|(at, pool)| pools[..at].contains(pool).then_some(pool))
    {
        return Err(format!("`pool {pool}` is set twice"));
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
    let verbs: Vec<&String> = d
        .objects
        .iter()
        .flat_map(|o| &o.moves)
        .map(|m| &m.verb)
        .collect();
    if let Some((u, verb)) = d.impls.iter().flat_map(|i| &i.uses).find_map(|u| {
        [u.tag.as_ref(), u.verb.as_ref()]
            .into_iter()
            .flatten()
            .find(|verb| !verbs.contains(verb))
            .map(|verb| (u, verb))
    }) {
        return Err(format!(
            "{} names the verb `{verb}`, and no object of `setup` has `on {verb}`",
            u.line()
        ));
    }
    let mut known = HashSet::new();
    for o in &d.objects {
        if !known.insert(o.name.as_str()) {
            return Err(format!("`{}` is declared twice", o.name));
        }
        o.validate()?;
        if !o.initial.is_empty() {
            d.init.insert(o.name.clone(), o.initial.clone());
        }
    }
    let c = d.check.clone().ok_or("needs `check`")?;
    checked(&d, &c)?;
    run(&d.objects, &d.init, &mut d.steps, &c)?;
    for o in &d.objects {
        o.reached(&d.init, &d.steps)?;
    }
    explore(&d, &c)?;
    let last = d.steps.len();
    match d.verdict() {
        Some(Verdict::Fails { step, .. }) if step + 1 == last => Ok(d),
        Some(Verdict::Fails { step, .. }) => Err(format!(
            "the run breaks `{}` at `action` line {}, before its last line",
            c.source(),
            step + 1
        )),
        _ => Err(format!(
            "the run passes `{}` at each `action` line",
            c.source()
        )),
    }
}

fn moved(o: &Object, now: &State, verb: &str) -> Result<Pairs, String> {
    let at = now.get(&o.name).cloned().unwrap_or_default();
    match o.moves.iter().find(|m| m.verb == verb && m.from == at) {
        Some(m) => Ok(m.to.clone()),
        None if o.moves.iter().any(|m| m.verb == verb) => Err(format!(
            "`{}` is `{}`, and no `on {verb}` line starts from it",
            o.name,
            written(&at)
        )),
        None => Err(format!("`{}` has no `on {verb}` line", o.name)),
    }
}

fn run(objects: &[Object], init: &State, steps: &mut [Step], c: &Check) -> Result<(), String> {
    let mut now = init.clone();
    let last = steps.len();
    for (k, s) in steps.iter_mut().enumerate() {
        let at = |e: String| format!("`action` line {}: {e}", k + 1);
        let find = |x: &str| {
            objects
                .iter()
                .find(|o| o.name == x)
                .ok_or_else(|| at(format!("unknown object `{x}`")))
        };
        find(&s.from)?;
        let target = find(&s.to)?;
        let mut set = vec![(
            target.name.clone(),
            moved(target, &now, &s.verb).map_err(at)?,
        )];
        for (x, key, v) in &s.shows {
            let o = find(x)?;
            match (k + 1 == last, c.objects.contains(x)) {
                (false, _) => {
                    return Err(at(format!(
                        "`{x}.{key}={}` is on a line before the last, and only the last line sets a state",
                        v.quint()
                    )));
                }
                (true, false) => {
                    return Err(at(format!(
                        "`{x}` is not an object of `forall`, and the last line sets only their states"
                    )));
                }
                (true, true) => {}
            }
            let i = match set.iter().position(|(n, _)| n == x) {
                Some(i) => i,
                None => {
                    set.push((x.clone(), now.get(x).cloned().unwrap_or_default()));
                    set.len() - 1
                }
            };
            set[i].1 = o
                .overlay(&set[i].1, &Pairs::from([(key.clone(), v.clone())]))
                .map_err(at)?;
        }
        now.extend(set.iter().cloned());
        s.set = set;
    }
    Ok(())
}

fn explore(d: &Diagram, c: &Check) -> Result<(), String> {
    let start = d.init.clone();
    let first = Ctx {
        start: &start,
        cur: &start,
        nxt: &start,
        verb: "",
    };
    if c.broken(&first).is_some() {
        return Err(format!("`{}` fails in the start state", c.source()));
    }
    let mut seen = HashSet::from([start.clone()]);
    let mut queue = VecDeque::from([(start, Vec::<String>::new())]);
    while let Some((now, trace)) = queue.pop_front() {
        for (o, verb, v) in d.enabled(&now) {
            let mut next = now.clone();
            let path = [
                trace.clone(),
                vec![format!("{verb} to={} ({})", o.name, written(&v))],
            ]
            .concat();
            next.insert(o.name.clone(), v);
            if c.judge(&d.init, &now, &next, verb).is_some() {
                return Err(format!(
                    "`{}` fails in the setup after {}",
                    c.source(),
                    path.join(", ")
                ));
            }
            if seen.insert(next.clone()) {
                queue.push_back((next, path));
            }
        }
    }
    Ok(())
}

const FORM: &str = "a file of weft has the KDL that versions 1 and 2 share: the name of a line and a key are words, a value is a string in `\"` or an integer, a line ends at the end of its text line, and `\\` at the end of a text line continues the line";

fn form(doc: &KdlDocument) -> Result<(), String> {
    let word = |repr: Option<&str>| !repr.is_some_and(|r| r.starts_with(['"', '#']));
    for n in doc.nodes() {
        if n.ty().is_some()
            || !word(n.name().repr())
            || n.format().is_some_and(|f| f.terminator.starts_with(';'))
        {
            return Err(format!("`{}` is not a line of weft: {FORM}", name(n)));
        }
        for e in n.entries() {
            let repr = e.format().map_or("", |f| f.value_repr.as_str());
            let fits = match e.value() {
                KdlValue::String(_) => repr.starts_with('"') && !repr.starts_with("\"\"\""),
                KdlValue::Integer(_) => repr
                    .strip_prefix('-')
                    .unwrap_or(repr)
                    .bytes()
                    .all(|b| b.is_ascii_digit()),
                _ => false,
            };
            if !fits || !e.name().is_none_or(|k| word(k.repr())) {
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
                    return Err(Error(format!("`style {kind}` is set twice")));
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
style "api" icon="server"
style "browser" icon="app-window"
style "element" icon="square-dashed-mouse-pointer""#;

    const BASE: &str = r#"
setup {
  person "user"
  browser "history" snapshot="snap_0" {
    state snapshot="snap_0" {
      on "leave" snapshot="snap_1"
    }
    state snapshot="snap_1" {
      on "back" snapshot="snap_0"
    }
  }
  element "origin" value="San Francisco" {
    state value="San Francisco" {
      on "type" value="Lahore"
    }
    state value="Lahore"
  }
}
action {
  type from="user" to="origin"
  leave from="user" to="history"
  back from="user" to="history" origin.value="San Francisco"
}
check {
  invariant {
    forall "element" "origin" {
      implies {
        in "step" "back"
        eq (next)"element.value" "element.value"
      }
    }
  }
}
"#;

    const SUBMIT: &str = r#"
setup {
  person "user"
  element "input" value="" {
    state value="" {
      on "type" value="Sent"
    }
    state value="Sent"
  }
  browser "page" snapshot="snap_0" count=0 {
    state snapshot="snap_0" count=0 {
      on "send" snapshot="snap_0" count=1
    }
    state snapshot="snap_0" count=1 {
      on "reload" snapshot="snap_0" count=1
    }
  }
}
action {
  type from="user" to="input"
  send from="input" to="page"
  reload from="user" to="page" page.count=2
}
check {
  invariant {
    forall "browser" "page" {
      and {
        implies {
          in "step" "send"
          eq (next)"browser.count" {
            iadd "browser.count" 1
          }
        }
        implies {
          in "step" "reload"
          eq (next)"browser.count" "browser.count"
        }
      }
    }
  }
}
"#;

    const POOL: &str = r#"
setup {
  person "user"
  browser "page" count=0 opened=0 {
    pool "opened"
    state count=0 opened=0 {
      on "open" count=1 opened=1
    }
    state count=1 opened=1 {
      on "open" count=1 opened=1
      on "close" count=0 opened=0
    }
  }
}
action {
  open from="user" to="page"
  close from="user" to="page" page.count=1
}
check {
  invariant {
    forall "browser" "page" {
      implies {
        eq (next)"browser.opened" 0
        eq (next)"browser.count" (start)"browser.count"
      }
    }
  }
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

    fn each(cases: &[(&str, &str, Result<(), &str>)]) {
        for (from, to, want) in cases {
            assert_eq!(built(BASE, from, to), want.map_err(String::from), "{to}");
        }
    }

    fn mark(text: &str, passed: bool) -> Mark {
        Mark {
            object: "page".into(),
            text: text.into(),
            passed,
        }
    }

    #[test]
    fn each_send_adds_one_and_a_reload_adds_none() {
        let d = parse(SUBMIT, &config(CONFIG).unwrap()).unwrap();
        let c = d.check.as_ref().unwrap();
        assert_eq!(
            c.source(),
            r#"always(forall(Set(page), browser => and(implies(in(step, Set("send")), eq(next(browser.count), iadd(browser.count, 1))), implies(in(step, Set("reload")), eq(next(browser.count), browser.count)))))"#
        );
        assert_eq!(
            c.english(),
            "Always, for each browser in page, if step is send, then browser.count becomes browser.count + 1, and if step is reload, then browser.count stays unchanged"
        );
        assert_eq!(
            d.verdict(),
            Some(Verdict::Fails {
                step: 2,
                evidence: vec![
                    "browser = page".into(),
                    r#"in("reload", Set("reload")) = true"#.into(),
                    "eq(next(browser.count), browser.count) = eq(2, 1) = false".into()
                ],
            })
        );
        assert_eq!(
            d.checks(),
            [
                (1, mark("count = 1, snapshot = snap_0", true)),
                (
                    2,
                    mark("count = 2, snapshot = snap_0\neq(2, 1) = false", false)
                )
            ]
        );
    }

    #[test]
    fn explore_breaks_the_check_from_the_setup_alone() {
        let cases = [
            (
                "      on \"reload\" snapshot=\"snap_0\" count=1\n    }\n",
                "      on \"reload\" snapshot=\"snap_0\" count=2\n    }\n    state snapshot=\"snap_0\" count=2\n",
                Err(
                    r#"`always(forall(Set(page), browser => and(implies(in(step, Set("send")), eq(next(browser.count), iadd(browser.count, 1))), implies(in(step, Set("reload")), eq(next(browser.count), browser.count)))))` fails in the setup after send to=page (count=1 snapshot="snap_0"), reload to=page (count=2 snapshot="snap_0")"#,
                ),
            ),
            (
                "      on \"send\" snapshot=\"snap_0\" count=1\n    }\n    state snapshot=\"snap_0\" count=1 {\n      on \"reload\" snapshot=\"snap_0\" count=1\n",
                "      on \"send\" snapshot=\"snap_0\" count=2\n    }\n    state snapshot=\"snap_0\" count=2 {\n      on \"reload\" snapshot=\"snap_0\" count=2\n",
                Err(
                    r#"`always(forall(Set(page), browser => and(implies(in(step, Set("send")), eq(next(browser.count), iadd(browser.count, 1))), implies(in(step, Set("reload")), eq(next(browser.count), browser.count)))))` fails in the setup after send to=page (count=2 snapshot="snap_0")"#,
                ),
            ),
            (
                "        implies {\n          in \"step\" \"reload\"\n          eq (next)\"browser.count\" \"browser.count\"\n        }\n",
                "        eq \"browser.count\" 1\n",
                Err(
                    r#"`always(forall(Set(page), browser => and(implies(in(step, Set("send")), eq(next(browser.count), iadd(browser.count, 1))), eq(browser.count, 1))))` fails in the start state"#,
                ),
            ),
        ];
        for (from, to, want) in cases {
            assert_eq!(built(SUBMIT, from, to), want.map_err(String::from), "{to}");
        }
    }

    #[test]
    fn each_term_is_typed() {
        let cases = [
            (
                "      on \"send\" snapshot=\"snap_0\" count=1\n",
                "      on \"send\" snapshot=\"snap_0\" count=\"1\"\n",
                Err("browser page: `count=\"1\"` of `page` is not of the type of `count=0`"),
            ),
            (
                "browser \"page\" snapshot=\"snap_0\" count=0 {",
                "browser \"page\" snapshot=\"snap_0\" count=0.5 {",
                Err(&format!(
                    "`count=0.5` of `browser` is not a value of weft: {FORM}"
                )),
            ),
            (
                "iadd \"browser.count\" 1",
                "iadd \"browser.count\" \"1\"",
                Err("`iadd(browser.count, \"1\")` takes two integers, not integer and string"),
            ),
            (
                "eq (next)\"browser.count\" \"browser.count\"",
                "eq (next)\"browser.count\" \"browser.snapshot\"",
                Err(
                    "`eq(next(browser.count), browser.snapshot)` takes two terms of one type, not integer and string",
                ),
            ),
            (
                "iadd \"browser.count\" 1",
                "isum \"browser.count\" 1",
                Err(&format!("`isum` is not a builtin: {CHECK}")),
            ),
            (
                "iadd \"browser.count\" 1",
                "iadd \"browser.count\"",
                Err(&format!("`iadd` takes two integers: {CHECK}")),
            ),
            (
                "in \"step\" \"reload\"",
                "ilte \"browser.count\" \"browser.snapshot\"",
                Err(
                    "`ilte(browser.count, browser.snapshot)` takes two integers, not integer and string",
                ),
            ),
            (
                "in \"step\" \"reload\"",
                "iadd \"browser.count\" 1",
                Err(
                    "`implies(iadd(browser.count, 1), eq(next(browser.count), browser.count))` takes two conditions, not integer and boolean",
                ),
            ),
            (
                "in \"step\" \"reload\"",
                "ilte 1 2",
                Err(&format!(
                    "`ilte(1, 2)` names at least one `browser.<key>` or `step`: {CHECK}"
                )),
            ),
            (
                "in \"step\" \"reload\"",
                "in \"step\" \"browser.snapshot\"",
                Err("`in step` takes verbs, not `browser.snapshot`"),
            ),
        ];
        for (from, to, want) in cases {
            assert_eq!(built(SUBMIT, from, to), want.map_err(String::from), "{to}");
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
        let c = d.check.as_ref().unwrap();
        assert_eq!(
            c.source(),
            "always(forall(Set(page), browser => implies(eq(next(browser.opened), 0), eq(next(browser.count), start(browser.count)))))"
        );
        assert_eq!(
            c.english(),
            "Always, for each browser in page, if browser.opened becomes 0, then browser.count becomes the start browser.count"
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
                "    pool \"opened\"\n",
                "    pool \"opened\"\n    pool \"opened\"\n",
                Err("`pool opened` is set twice"),
            ),
            (
                "run \"opened\" consume=\"opened\" verb=\"close\"",
                "run \"page.close\" from=\"opened\" verb=\"close\"",
                Err(
                    r#"`run "page.close"` draws from the pool `opened`, and takes `"opened"` or `"opened.<method>"`"#,
                ),
            ),
            (
                "run \"opened\" consume=\"opened\" verb=\"close\"",
                "run \"opened.close\" from=\"opened\" verb=\"close\"",
                Ok(()),
            ),
            (
                "run \"opened\" consume=\"opened\" verb=\"close\"",
                "run \"opened\" consume=\"opened\"",
                Err("`run` needs `input` or `verb`"),
            ),
            (
                "run \"opened\" consume=\"opened\" verb=\"close\"",
                "run \"opened\" from=\"opened\" consume=\"opened\" verb=\"close\"",
                Err("`run` has `from` and `consume`, and a line takes one of them"),
            ),
            (
                "run \"page.open\" input=\"page.openSchema\" pool=\"opened\"",
                "run \"page.open\" input=\"page.openSchema\" pool=#true",
                Err(&format!(
                    "`pool=#true` of `run` is not a value of weft: {FORM}"
                )),
            ),
            (
                "(start)\"browser.count\"",
                "(start)\"browser\"",
                Err(&format!(
                    "`(start)` takes `browser.<key>`, not `browser`: {CHECK}"
                )),
            ),
        ];
        for (from, to, want) in cases {
            assert_eq!(built(POOL, from, to), want.map_err(String::from), "{to}");
        }
    }

    fn condition(src: &str) -> Check {
        let doc = document(src).unwrap();
        Check {
            kind: Kind::Browser,
            objects: vec!["page".into()],
            body: expr(&doc.nodes()[0], "browser").unwrap(),
        }
    }

    fn page(count: i128) -> State {
        State::from([(
            "page".into(),
            Pairs::from([
                ("count".into(), Value::Int(count)),
                ("snapshot".into(), Value::Text("snap_0".into())),
            ]),
        )])
    }

    #[test]
    fn each_builtin_gives_the_value_of_the_quint_evaluator() {
        let cases = [
            ("ilte (next)\"browser.count\" \"browser.count\"", 1, 1, true),
            (
                "ilte (next)\"browser.count\" \"browser.count\"",
                1,
                2,
                false,
            ),
            ("ilt (next)\"browser.count\" \"browser.count\"", 1, 0, true),
            ("ilt (next)\"browser.count\" \"browser.count\"", 1, 1, false),
            ("igte (next)\"browser.count\" \"browser.count\"", 1, 1, true),
            (
                "igte (next)\"browser.count\" \"browser.count\"",
                1,
                0,
                false,
            ),
            ("igt (next)\"browser.count\" \"browser.count\"", 1, 2, true),
            ("igt (next)\"browser.count\" \"browser.count\"", 1, 1, false),
            ("neq (next)\"browser.count\" \"browser.count\"", 1, 2, true),
            ("neq (next)\"browser.count\" \"browser.count\"", 1, 1, false),
            (
                "or {\neq \"browser.count\" 1\neq \"browser.count\" 2\n}",
                0,
                2,
                true,
            ),
            (
                "or {\neq \"browser.count\" 1\neq \"browser.count\" 2\n}",
                0,
                3,
                false,
            ),
            ("not {\neq \"browser.count\" 1\n}", 0, 2, true),
            ("not {\neq \"browser.count\" 1\n}", 0, 1, false),
            (
                "iff {\neq \"browser.count\" 1\nin \"step\" \"send\"\n}",
                0,
                1,
                true,
            ),
            (
                "iff {\neq \"browser.count\" 1\nin \"step\" \"send\"\n}",
                0,
                2,
                false,
            ),
            (
                "eq (next)\"browser.count\" {\nisub \"browser.count\" 1\n}",
                3,
                2,
                true,
            ),
            (
                "eq (next)\"browser.count\" {\nimul \"browser.count\" 2\n}",
                3,
                6,
                true,
            ),
            (
                "eq (next)\"browser.count\" {\nimul \"browser.count\" 2\n}",
                3,
                5,
                false,
            ),
            (
                "eq (next)\"browser.count\" {\nidiv \"browser.count\" 2\n}",
                7,
                3,
                true,
            ),
            (
                "eq (next)\"browser.count\" {\nidiv \"browser.count\" 2\n}",
                -7,
                -3,
                true,
            ),
            (
                "eq (next)\"browser.count\" {\nimod \"browser.count\" 2\n}",
                7,
                1,
                true,
            ),
            (
                "eq (next)\"browser.count\" {\nimod \"browser.count\" 2\n}",
                -7,
                -1,
                true,
            ),
            (
                "eq (next)\"browser.count\" {\nipow \"browser.count\" 3\n}",
                2,
                8,
                true,
            ),
            (
                "eq (next)\"browser.count\" {\niuminus \"browser.count\"\n}",
                2,
                -2,
                true,
            ),
            (
                "eq (next)\"browser.count\" {\nidiv 6 \"browser.count\"\n}",
                0,
                0,
                false,
            ),
            (
                "eq (next)\"browser.count\" {\nimod 6 \"browser.count\"\n}",
                0,
                0,
                false,
            ),
            (
                "eq (next)\"browser.count\" {\nipow \"browser.count\" 0\n}",
                0,
                1,
                false,
            ),
            (
                "eq (next)\"browser.count\" {\nipow 2 \"browser.count\"\n}",
                -1,
                0,
                false,
            ),
            (
                "eq (next)\"browser.count\" {\niadd \"browser.count\" 1\n}",
                i128::MAX,
                i128::MIN,
                false,
            ),
            (
                "implies {\nneq \"browser.count\" 0\neq 3 {\nidiv 6 \"browser.count\"\n}\n}",
                0,
                0,
                true,
            ),
            (
                "implies {\nneq \"browser.count\" 0\neq 3 {\nidiv 6 \"browser.count\"\n}\n}",
                2,
                2,
                true,
            ),
            (
                "implies {\nneq \"browser.count\" 0\neq 3 {\nidiv 6 \"browser.count\"\n}\n}",
                3,
                3,
                false,
            ),
            ("in \"browser.snapshot\" \"snap_1\" \"snap_0\"", 0, 0, true),
            ("in \"browser.snapshot\" \"snap_1\" \"snap_2\"", 0, 0, false),
            ("in \"step\" \"open\" \"send\"", 0, 0, true),
            ("in \"step\" \"open\" \"reload\"", 0, 0, false),
        ];
        for (src, cur, nxt, passes) in cases {
            let broken = condition(src).judge(&page(cur), &page(cur), &page(nxt), "send");
            assert_eq!(broken.is_none(), passes, "{src} from {cur} to {nxt}");
        }
    }

    #[test]
    fn each_builtin_reads_as_quint_and_as_english() {
        let cases = [
            (
                "ilte (next)\"browser.count\" \"browser.count\"",
                "ilte(next(browser.count), browser.count)",
                "the next browser.count is at most browser.count",
            ),
            (
                "or {\nigt \"browser.count\" 1\nnot {\neq \"browser.snapshot\" \"snap_0\"\n}\n}",
                r#"or(igt(browser.count, 1), not(eq(browser.snapshot, "snap_0")))"#,
                r#"browser.count is more than 1, or browser.snapshot does not equal "snap_0""#,
            ),
            (
                "implies {\nnot {\nin \"step\" \"send\" \"open\"\n}\neq (next)\"browser.count\" \"browser.count\"\n}",
                r#"implies(not(in(step, Set("send", "open"))), eq(next(browser.count), browser.count))"#,
                "if step is not send or open, then browser.count stays unchanged",
            ),
            (
                "not {\nilt \"browser.count\" 1\n}",
                "not(ilt(browser.count, 1))",
                "not browser.count is less than 1",
            ),
            (
                "iff {\nneq \"browser.count\" 0\nin \"browser.snapshot\" \"snap_0\" \"snap_1\"\n}",
                r#"iff(neq(browser.count, 0), in(browser.snapshot, Set("snap_0", "snap_1")))"#,
                "browser.count does not equal 0 if and only if browser.snapshot is snap_0 or snap_1",
            ),
            (
                "eq (next)\"browser.count\" {\nimod {\nipow \"browser.count\" 2\niuminus 3\n}\n}",
                "eq(next(browser.count), imod(ipow(browser.count, 2), iuminus(3)))",
                "browser.count becomes browser.count ^ 2 mod -3",
            ),
        ];
        for (src, quint, english) in cases {
            let c = condition(src);
            assert_eq!(
                (c.body.quint("browser"), c.body.english("browser")),
                (quint.to_string(), english.to_string())
            );
        }
    }

    #[test]
    fn the_evidence_shows_the_value_of_each_integer_builtin() {
        let c = condition("ilte (next)\"browser.count\" {\nisub \"browser.count\" 1\n}");
        assert_eq!(
            c.judge(&page(3), &page(3), &page(5), "send"),
            Some(vec![
                "browser = page".into(),
                "ilte(next(browser.count), isub(browser.count, 1)) = ilte(5, 2) = false".into()
            ])
        );
    }

    #[test]
    fn each_object_and_each_on_line_has_its_line_in_the_file() {
        let d = parse(BASE, &config(CONFIG).unwrap()).unwrap();
        let lines: Vec<String> = d
            .objects
            .iter()
            .flat_map(|o| {
                std::iter::once(format!("{} {}", o.name, o.line))
                    .chain(o.moves.iter().map(|m| format!("on {} {}", m.verb, m.line)))
            })
            .collect();
        assert_eq!(
            lines,
            [
                "user 3",
                "history 4",
                "on leave 6",
                "on back 9",
                "origin 12",
                "on type 14"
            ]
        );
    }

    #[test]
    fn the_run_breaks_the_check_at_its_last_line() {
        let d = parse(BASE, &config(CONFIG).unwrap()).unwrap();
        let Some(Verdict::Fails { step, .. }) = d.verdict() else {
            panic!("the run passes the check");
        };
        assert_eq!(step, 2);
        assert_eq!(
            d.checks(),
            [(
                2,
                Mark {
                    object: "origin".into(),
                    text: "value = San Francisco\neq(\"San Francisco\", \"Lahore\") = false".into(),
                    passed: false,
                }
            )]
        );
        assert_eq!(
            d.initial("origin").as_deref(),
            Some("value = San Francisco")
        );
    }

    #[test]
    fn each_object_and_each_state_is_reached_by_an_action_line() {
        each(&[
            (
                "  person \"user\"\n",
                "  person \"user\"\n  person \"admin\"\n",
                Err("no `action` line reaches `admin`"),
            ),
            (
                "  person \"user\"\n",
                "  person \"user\"\n  api \"correlation\"\n",
                Err("no `action` line reaches `correlation`"),
            ),
            (
                "  person \"user\"\n",
                "  person \"user\"\n  browser \"page\" snapshot=\"snap_0\" {\n    state snapshot=\"snap_0\"\n  }\n",
                Err("no `action` line reaches `page`"),
            ),
            (
                "    state value=\"Lahore\"\n",
                "    state value=\"Lahore\" {\n      on \"type\" value=\"Tokyo\"\n    }\n    state value=\"Tokyo\"\n",
                Err("no `action` line reaches the state `value=\"Tokyo\"` of `origin`"),
            ),
            (
                "      on \"back\" snapshot=\"snap_0\"\n",
                "      on \"back\" snapshot=\"snap_0\"\n      on \"leave\" snapshot=\"snap_2\"\n    }\n    state snapshot=\"snap_2\" {\n",
                Err("no `action` line reaches the state `snapshot=\"snap_2\"` of `history`"),
            ),
            (
                "      on \"leave\" snapshot=\"snap_1\"\n",
                "      on \"leave\" snapshot=\"snap_1\"\n      on \"forward\" snapshot=\"snap_1\"\n",
                Ok(()),
            ),
            (
                "  leave from=\"user\" to=\"history\"\n",
                "  forward from=\"user\" to=\"history\"\n",
                Err("`action` line 2: `history` has no `on forward` line"),
            ),
        ]);
    }

    #[test]
    fn each_start_state_and_each_state_is_key_value() {
        each(&[
            (
                "element \"origin\" value=\"San Francisco\" {",
                "element \"origin\" \"San Francisco\" {",
                Err("element: `element` takes 1 names, not 2"),
            ),
            (
                "    state value=\"Lahore\"\n",
                "    state \"Lahore\"\n",
                Err("element origin: `state` takes 0 names, not 1"),
            ),
            (
                "      on \"type\" value=\"Lahore\"\n",
                "      on \"type\" \"Lahore\"\n",
                Err("element origin: `on` takes 1 names, not 2"),
            ),
            (
                "element \"origin\" value=\"San Francisco\" {",
                "element \"origin\" {",
                Err("`origin` has states and no `<key>=<value>` on its line"),
            ),
            (
                "  person \"user\"\n",
                "  person \"user\"\n  api \"correlation\" value=\"ready\"\n",
                Err("`correlation` sets `value=\"ready\"` and has no states"),
            ),
            (
                "  person \"user\"\n",
                "  person \"user\" name=\"agent\"\n",
                Err("`user` is a person, and a person has no states"),
            ),
            (
                "    state value=\"Lahore\"\n",
                "    state city=\"Lahore\"\n",
                Err("element origin: `state city=\"Lahore\"` sets no `value`"),
            ),
            (
                "    state value=\"Lahore\"\n",
                "    state value=\"Lahore\" city=\"Lahore\"\n",
                Err("element origin: `origin` has no `city`"),
            ),
            (
                "    state value=\"Lahore\"\n",
                "    state value=\"Lahore\" value=\"Tokyo\"\n",
                Err("element origin: `state` sets `value` twice"),
            ),
            (
                "      on \"type\" value=\"Lahore\"\n",
                "      on \"type\" city=\"Lahore\"\n",
                Err("element origin: `origin` has no `city`"),
            ),
            (
                "      on \"type\" value=\"Lahore\"\n",
                "      on \"type\" value=\"Tokyo\"\n",
                Err(
                    "the state `value=\"San Francisco\"` of `origin` has `on type`, and `value=\"Tokyo\"` is not a state of `origin`",
                ),
            ),
            (
                "element \"origin\" value=\"San Francisco\" {",
                "element \"origin\" value=\"Tokyo\" {",
                Err("the start state `value=\"Tokyo\"` of `origin` is not one of its states"),
            ),
            (
                "eq (next)\"element.value\" \"element.value\"",
                "eq (next)\"element\" \"element\"",
                Err(&format!(
                    "`(next)` takes `element.<key>`, not `element`: {CHECK}"
                )),
            ),
            (
                "eq (next)\"element.value\" \"element.value\"",
                "eq (next)\"element.city\" \"element.city\"",
                Err("`origin` has no `city`"),
            ),
            (
                "eq (next)\"element.value\" \"element.value\"",
                "eq (next)\"element.value\" \"Tokyo\"",
                Err("`value=\"Tokyo\"` is in no state of `origin`"),
            ),
            (
                "origin.value=\"San Francisco\"",
                "origin=\"San Francisco\"",
                Err("`origin=\"San Francisco\"` is not `<object>.<key>=<value>`"),
            ),
            (
                "origin.value=\"San Francisco\"",
                "origin.value=\"Tokyo\"",
                Ok(()),
            ),
        ]);
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
                "person \"user\"",
                "\"person\" \"user\"",
                Err(line("person")),
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
        let d = parse(
            &POOL.replacen("    pool \"opened\"\n", block, 1),
            &config(CONFIG).unwrap(),
        )
        .unwrap();
        let draws: Vec<(String, Vec<String>, bool)> = d.objects[1]
            .draws
            .iter()
            .map(|d| {
                (
                    d.path.join("."),
                    d.values.iter().map(Value::quint).collect(),
                    d.keys,
                )
            })
            .collect();
        let draw = |path: &str, values: &[&str], keys: bool| {
            (
                path.to_string(),
                values.iter().map(|v| v.to_string()).collect::<Vec<_>>(),
                keys,
            )
        };
        assert_eq!(
            draws,
            [
                draw("id", &["1", "2"], false),
                draw("attributes", &["\"class\"", "\"title\""], true),
                draw("attributes.class", &["\"a\"", "\"b\""], false),
                draw("attributes.title", &[], false),
            ]
        );
        let at = |e: &str| Err(format!("browser page: {e}"));
        let cases = [
            ("      id 1 2\n", "      id 1 1\n", at("`id` of `draw` has `1` twice")),
            (
                "      id 1 2\n",
                "      id 1\n      id 2\n",
                at("`draw` sets `id` twice"),
            ),
            ("      attributes {\n", "      attributes \"a\" {\n", at(DRAW)),
            ("    draw {\n", "    draw \"id\" {\n", at(DRAW)),
            (
                "    draw {\n",
                "    draw {\n      n 1\n    }\n    draw {\n",
                at("has one `draw`"),
            ),
            (
                "verb=\"close\"",
                "verb=\"shut\"",
                Err(
                    r#"`run "opened" verb="shut" consume="opened"` names the verb `shut`, and no object of `setup` has `on shut`"#
                        .to_string(),
                ),
            ),
            (
                "  module \"page\" \"../src/page.ts\"\n",
                "  module \"page\" \"../src/page.ts\"\n  module \"other\" \"../src/other.ts\"\n",
                Err("no line of `impl bun` names `module \"other\"`".to_string()),
            ),
        ];
        let base = POOL.replacen("    pool \"opened\"\n", block, 1);
        for (from, to, want) in cases {
            assert_eq!(built(&base, from, to), want, "{to}");
        }
    }
}
