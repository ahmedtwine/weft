use std::collections::{HashMap, HashSet};
use std::fmt;

use hcl::expr::{BinaryOperator, Operation, TemplateExpr, UnaryOperator};
use hcl::{BlockLabel, Body, Expression, ObjectKey, Structure};
use serde::Deserialize;

#[derive(Debug, Clone, Default)]
pub struct Diagram {
    pub title: String,
    pub objects: Vec<Object>,
    pub init: Vec<(String, Data)>,
    pub steps: Vec<Step>,
    pub checks: Vec<Check>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub enum Method {
    #[serde(rename = "GET")]
    Get,
    #[serde(rename = "POST")]
    Post,
    #[serde(rename = "PUT")]
    Put,
    #[serde(rename = "PATCH")]
    Patch,
    #[serde(rename = "DELETE")]
    Delete,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub enum Store {
    S3,
    #[serde(rename = "disk")]
    Disk,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub enum Storage {
    #[serde(rename = "history.state")]
    HistoryState,
    #[serde(rename = "localStorage")]
    LocalStorage,
    #[serde(rename = "sessionStorage")]
    SessionStorage,
    #[serde(rename = "cookie")]
    Cookie,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Person {
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Api {
    pub method: Method,
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Db {
    pub table: String,
    pub key: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct File {
    pub store: Store,
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Page {
    pub element: String,
    pub page: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Code {
    pub symbol: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Browser {
    pub storage: Storage,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Type {
    Person(Person),
    Api(Api),
    Db(Db),
    File(File),
    Page(Page),
    Code(Code),
    Browser(Browser),
}

#[derive(Debug, Clone)]
pub struct Object {
    pub name: String,
    pub ty: Type,
    pub machine: Vec<(String, String)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Data {
    Text(String),
    List(Vec<Data>),
    Record(Vec<(String, Data)>),
}

#[derive(Debug, Clone)]
pub struct Step {
    pub from: String,
    pub to: String,
    pub send: Option<Data>,
    pub set: Vec<(String, Data)>,
}

#[derive(Debug, Clone)]
pub struct Check {
    pub name: String,
    pub when: Option<Cond>,
    pub then: Cond,
}

#[derive(Debug, Clone)]
pub enum Cond {
    From(String),
    Changed(String),
    Equal(Term, Term),
    Differ(Term, Term),
    And(Box<Cond>, Box<Cond>),
    Or(Box<Cond>, Box<Cond>),
    Not(Box<Cond>),
}

#[derive(Debug, Clone)]
pub enum Term {
    State(String),
    Text(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    Holds,
    Fails {
        step: usize,
        objects: Vec<String>,
        reason: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error(pub String);

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

impl Method {
    pub fn as_str(self) -> &'static str {
        match self {
            Method::Get => "GET",
            Method::Post => "POST",
            Method::Put => "PUT",
            Method::Patch => "PATCH",
            Method::Delete => "DELETE",
        }
    }
}

impl Store {
    pub fn as_str(self) -> &'static str {
        match self {
            Store::S3 => "S3",
            Store::Disk => "disk",
        }
    }
}

impl Storage {
    pub fn as_str(self) -> &'static str {
        match self {
            Storage::HistoryState => "history.state",
            Storage::LocalStorage => "localStorage",
            Storage::SessionStorage => "sessionStorage",
            Storage::Cookie => "cookie",
        }
    }
}

impl Type {
    pub fn kind(&self) -> &'static str {
        match self {
            Type::Person(_) => "person",
            Type::Api(_) => "api",
            Type::Db(_) => "db",
            Type::File(_) => "file",
            Type::Page(_) => "page",
            Type::Code(_) => "code",
            Type::Browser(_) => "browser",
        }
    }

    pub fn fields(&self) -> Vec<(&'static str, &str)> {
        match self {
            Type::Person(p) => vec![("name", &p.name)],
            Type::Api(a) => vec![("method", a.method.as_str()), ("path", &a.path)],
            Type::Db(d) => vec![("table", &d.table), ("key", &d.key)],
            Type::File(f) => vec![("store", f.store.as_str()), ("path", &f.path)],
            Type::Page(p) => vec![("element", &p.element), ("page", &p.page)],
            Type::Code(c) => vec![("symbol", &c.symbol)],
            Type::Browser(b) => vec![("storage", b.storage.as_str())],
        }
    }
}

impl Data {
    pub fn flat(&self) -> String {
        match self {
            Data::Text(t) => t.clone(),
            Data::List(items) => format!(
                "[{}]",
                items.iter().map(Data::flat).collect::<Vec<_>>().join(", ")
            ),
            Data::Record(fields) => {
                let inner: Vec<String> = fields
                    .iter()
                    .map(|(k, v)| format!("{k}: {}", v.flat()))
                    .collect();
                format!("{{{}}}", inner.join(", "))
            }
        }
    }
}

type State = HashMap<String, String>;

impl Diagram {
    pub fn verdict(&self) -> Option<Verdict> {
        let machines: Vec<&Object> = self
            .objects
            .iter()
            .filter(|o| !o.machine.is_empty())
            .collect();
        if self.checks.is_empty() && machines.is_empty() {
            return None;
        }
        let mut now: State = self
            .init
            .iter()
            .map(|(k, v)| (k.clone(), v.flat()))
            .collect();
        for (step, s) in self.steps.iter().enumerate() {
            let mut next = now.clone();
            next.extend(s.set.iter().map(|(k, v)| (k.clone(), v.flat())));
            for o in &machines {
                if let (Some(a), Some(b)) = (now.get(&o.name), next.get(&o.name))
                    && a != b
                    && !o.machine.iter().any(|(x, y)| x == a && y == b)
                {
                    let reason = format!(
                        "{} {} has no transition from {a} to {b}",
                        o.ty.kind(),
                        o.name
                    );
                    return Some(Verdict::Fails {
                        step,
                        objects: vec![s.from.clone(), o.name.clone()],
                        reason,
                    });
                }
            }
            for c in &self.checks {
                if c.when
                    .as_ref()
                    .is_none_or(|w| w.holds(&now, &next, &s.from))
                    && !c.then.holds(&now, &next, &s.from)
                {
                    let mut objects: Vec<String> =
                        c.objects().into_iter().map(String::from).collect();
                    objects.extend([s.from.clone(), s.to.clone()]);
                    return Some(Verdict::Fails {
                        step,
                        objects,
                        reason: c.name.clone(),
                    });
                }
            }
            now = next;
        }
        Some(Verdict::Holds)
    }
}

impl Check {
    pub fn objects(&self) -> Vec<&str> {
        let mut out = Vec::new();
        self.when
            .iter()
            .chain([&self.then])
            .for_each(|c| c.objects(&mut out));
        out
    }
}

impl Cond {
    fn holds(&self, now: &State, next: &State, from: &str) -> bool {
        match self {
            Cond::From(o) => o == from,
            Cond::Changed(o) => now.get(o) != next.get(o),
            Cond::Equal(a, b) => a.value(next) == b.value(next),
            Cond::Differ(a, b) => a.value(next) != b.value(next),
            Cond::And(a, b) => a.holds(now, next, from) && b.holds(now, next, from),
            Cond::Or(a, b) => a.holds(now, next, from) || b.holds(now, next, from),
            Cond::Not(a) => !a.holds(now, next, from),
        }
    }

    fn objects<'a>(&'a self, out: &mut Vec<&'a str>) {
        match self {
            Cond::From(o) | Cond::Changed(o) => out.push(o),
            Cond::Equal(a, b) | Cond::Differ(a, b) => {
                out.extend([a, b].into_iter().filter_map(Term::object))
            }
            Cond::And(a, b) | Cond::Or(a, b) => {
                a.objects(out);
                b.objects(out);
            }
            Cond::Not(a) => a.objects(out),
        }
    }
}

impl Term {
    fn value<'a>(&'a self, state: &'a State) -> &'a str {
        match self {
            Term::State(o) => state.get(o).map_or("", String::as_str),
            Term::Text(t) => t,
        }
    }

    fn object(&self) -> Option<&str> {
        match self {
            Term::State(o) => Some(o),
            Term::Text(_) => None,
        }
    }
}

fn text(e: &Expression) -> Result<String, String> {
    match e {
        Expression::String(s) => Ok(s.clone()),
        Expression::TemplateExpr(t) => match t.as_ref() {
            TemplateExpr::QuotedString(s) => Ok(s.clone()),
            TemplateExpr::Heredoc(h) => Ok(h.template.clone()),
        },
        Expression::Number(n) => Ok(n.to_string()),
        Expression::Bool(b) => Ok(b.to_string()),
        other => Err(format!("expected a string, got `{other}`")),
    }
}

fn name(e: &Expression) -> Result<String, String> {
    match e {
        Expression::Variable(v) => Ok(v.as_str().to_string()),
        other => Err(format!("expected an object name, got `{other}`")),
    }
}

fn key(k: &ObjectKey) -> Result<String, String> {
    match k {
        ObjectKey::Identifier(id) => Ok(id.as_str().to_string()),
        ObjectKey::Expression(e) => text(e),
        other => Err(format!("`{other:?}` is not an object key")),
    }
}

fn data(e: &Expression) -> Result<Data, String> {
    match e {
        Expression::Array(items) => items
            .iter()
            .map(data)
            .collect::<Result<_, _>>()
            .map(Data::List),
        Expression::Object(fields) => pairs(fields.iter()).map(Data::Record),
        other => text(other).map(Data::Text),
    }
}

fn pairs<'a>(
    fields: impl Iterator<Item = (&'a ObjectKey, &'a Expression)>,
) -> Result<Vec<(String, Data)>, String> {
    fields.map(|(k, v)| Ok((key(k)?, data(v)?))).collect()
}

fn object_pairs(e: &Expression) -> Result<Vec<(String, Data)>, String> {
    match e {
        Expression::Object(fields) => pairs(fields.iter()),
        other => Err(format!("expected an object, got `{other}`")),
    }
}

fn term(e: &Expression) -> Result<Term, String> {
    match e {
        Expression::Variable(v) => Ok(Term::State(v.as_str().to_string())),
        Expression::Parenthesis(inner) => term(inner),
        other => text(other).map(Term::Text),
    }
}

fn cond(e: &Expression) -> Result<Cond, String> {
    match e {
        Expression::Parenthesis(inner) => cond(inner),
        Expression::FuncCall(f) => match (f.name.name.as_str(), f.args.as_slice()) {
            ("from", [arg]) => name(arg).map(Cond::From),
            ("changed", [arg]) => name(arg).map(Cond::Changed),
            (other, _) => Err(format!(
                "`{other}` is not `from(object)` or `changed(object)`"
            )),
        },
        Expression::Operation(op) => match op.as_ref() {
            Operation::Unary(u) if u.operator == UnaryOperator::Not => {
                Ok(Cond::Not(Box::new(cond(&u.expr)?)))
            }
            Operation::Binary(b) => {
                let (l, r) = (&b.lhs_expr, &b.rhs_expr);
                match b.operator {
                    BinaryOperator::Eq => Ok(Cond::Equal(term(l)?, term(r)?)),
                    BinaryOperator::NotEq => Ok(Cond::Differ(term(l)?, term(r)?)),
                    BinaryOperator::And => Ok(Cond::And(Box::new(cond(l)?), Box::new(cond(r)?))),
                    BinaryOperator::Or => Ok(Cond::Or(Box::new(cond(l)?), Box::new(cond(r)?))),
                    other => Err(format!("`{other:?}` is not ==, !=, && or ||")),
                }
            }
            other => Err(format!("`{other:?}` is not a condition")),
        },
        other => Err(format!("`{other}` is not a condition")),
    }
}

fn attributes(body: &Body, allowed: &[&str]) -> Result<HashMap<String, Expression>, String> {
    let mut out = HashMap::new();
    for s in &body.0 {
        match s {
            Structure::Attribute(a) if allowed.contains(&a.key.as_str()) => {
                out.insert(a.key.as_str().to_string(), a.expr.clone());
            }
            Structure::Attribute(a) => {
                return Err(format!(
                    "`{}` is not one of {}",
                    a.key.as_str(),
                    allowed.join(", ")
                ));
            }
            Structure::Block(b) => {
                return Err(format!(
                    "the block `{}` is not allowed here",
                    b.identifier.as_str()
                ));
            }
        }
    }
    Ok(out)
}

fn label(labels: &[BlockLabel]) -> Result<String, String> {
    match labels {
        [l] => Ok(l.as_str().to_string()),
        _ => Err("a block needs exactly one label".into()),
    }
}

fn object(kind: &str, labels: &[BlockLabel], body: &Body) -> Result<Object, String> {
    let object_name = label(labels)?;
    let (states, rest): (Vec<&Structure>, Vec<&Structure>) = body
        .0
        .iter()
        .partition(|s| matches!(s, Structure::Attribute(a) if a.key.as_str() == "states"));
    let rest = Body(rest.into_iter().cloned().collect());
    let typed = |e: hcl::Error| format!("{kind} \"{object_name}\": {e}");
    let ty = match kind {
        "person" => Type::Person(hcl::from_body(rest).map_err(typed)?),
        "api" => Type::Api(hcl::from_body(rest).map_err(typed)?),
        "db" => Type::Db(hcl::from_body(rest).map_err(typed)?),
        "file" => Type::File(hcl::from_body(rest).map_err(typed)?),
        "page" => Type::Page(hcl::from_body(rest).map_err(typed)?),
        "code" => Type::Code(hcl::from_body(rest).map_err(typed)?),
        "browser" => Type::Browser(hcl::from_body(rest).map_err(typed)?),
        other => {
            return Err(format!(
                "`{other}` is not person, api, db, file, page, code, browser, step or check"
            ));
        }
    };
    let mut machine = Vec::new();
    for s in states {
        if let Structure::Attribute(a) = s {
            for (state, next) in object_pairs(&a.expr)? {
                match next {
                    Data::List(items) => {
                        machine.extend(items.iter().map(|b| (state.clone(), b.flat())))
                    }
                    other => machine.push((state, other.flat())),
                }
            }
        }
    }
    Ok(Object {
        name: object_name,
        ty,
        machine,
    })
}

fn step(body: &Body) -> Result<Step, String> {
    let attrs = attributes(body, &["from", "to", "send", "set"])?;
    let get = |k: &str| attrs.get(k).ok_or_else(|| format!("a step needs `{k}`"));
    Ok(Step {
        from: name(get("from")?)?,
        to: name(get("to")?)?,
        send: attrs.get("send").map(data).transpose()?,
        set: attrs
            .get("set")
            .map(object_pairs)
            .transpose()?
            .unwrap_or_default(),
    })
}

fn check(labels: &[BlockLabel], body: &Body) -> Result<Check, String> {
    let attrs = attributes(body, &["when", "then"])?;
    let then = attrs
        .get("then")
        .ok_or_else(|| "an invariant needs `then`".to_string())?;
    Ok(Check {
        name: label(labels)?,
        when: attrs.get("when").map(cond).transpose()?,
        then: cond(then)?,
    })
}

fn build(body: &Body) -> Result<Diagram, String> {
    let mut d = Diagram::default();
    let mut seen = HashSet::new();
    for s in &body.0 {
        let b = match s {
            Structure::Attribute(a) if a.key.as_str() == "title" => {
                d.title = text(&a.expr)?;
                continue;
            }
            Structure::Attribute(a) => return Err(format!("`{}` is not title", a.key)),
            Structure::Block(b) => b,
        };
        let parent = b.identifier.as_str();
        let holds = match parent {
            "setup" => "objects and `init`",
            "action" => "`step` blocks",
            "check" => "`invariant` blocks",
            other => return Err(format!("`{other}` is not setup, action or check")),
        };
        if !b.labels.is_empty() || !seen.insert(parent) {
            return Err(format!("`{parent}` is one block with no label"));
        }
        for c in &b.body.0 {
            match (parent, c) {
                ("setup", Structure::Attribute(a)) if a.key.as_str() == "init" => {
                    d.init = object_pairs(&a.expr)?
                }
                ("setup", Structure::Block(o)) => {
                    d.objects
                        .push(object(o.identifier.as_str(), &o.labels, &o.body)?)
                }
                ("action", Structure::Block(o)) if o.identifier.as_str() == "step" => {
                    d.steps.push(step(&o.body)?)
                }
                ("check", Structure::Block(o)) if o.identifier.as_str() == "invariant" => {
                    d.checks.push(check(&o.labels, &o.body)?)
                }
                _ => return Err(format!("`{parent}` holds {holds}")),
            }
        }
    }
    if d.objects.is_empty() {
        return Err("no objects".into());
    }
    let mut known = HashSet::new();
    for o in &d.objects {
        if !known.insert(o.name.as_str()) {
            return Err(format!("`{}` is declared twice", o.name));
        }
    }
    let steps = d.steps.iter().flat_map(|s| {
        [s.from.as_str(), s.to.as_str()]
            .into_iter()
            .chain(s.set.iter().map(|(k, _)| k.as_str()))
    });
    let used = d
        .init
        .iter()
        .map(|(k, _)| k.as_str())
        .chain(steps)
        .chain(d.checks.iter().flat_map(Check::objects));
    for n in used {
        if !known.contains(n) {
            return Err(format!("unknown object `{n}`"));
        }
    }
    Ok(d)
}

pub fn parse(src: &str) -> Result<Diagram, Error> {
    let body = hcl::parse(src).map_err(|e| Error(e.to_string()))?;
    build(&body).map_err(Error)
}
