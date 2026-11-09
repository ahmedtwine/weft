use std::collections::{BTreeMap, HashSet, VecDeque};
use std::fmt;

use kdl::KdlValue;
pub use kdl::{KdlDocument, KdlEntry, KdlNode};
use miette::SourceCode;

#[derive(Debug, Clone, Default)]
pub struct Diagram {
    pub objects: Vec<Object>,
    pub init: Vec<(String, String)>,
    pub steps: Vec<Step>,
    pub check: Option<Check>,
}

#[derive(Debug, Clone)]
pub struct Node {
    pub name: String,
    pub parent: Option<usize>,
    pub initial: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Move {
    pub actor: String,
    pub verb: String,
    pub target: String,
    pub from: Option<String>,
    pub to: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Object {
    pub name: String,
    pub kind: String,
    pub label: String,
    pub value: Option<String>,
    pub initial: Option<String>,
    pub nodes: Vec<Node>,
    pub moves: Vec<Move>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Data {
    Text(String),
    List(Vec<Data>),
    Record(Vec<(String, Data)>),
}

#[derive(Debug, Clone)]
pub struct Class {
    pub name: String,
    pub fields: Vec<(String, Data)>,
}

#[derive(Debug, Clone)]
pub struct Step {
    pub from: String,
    pub to: String,
    pub send: Class,
    pub shows: Vec<(String, String)>,
    pub set: Vec<(String, String)>,
}

#[derive(Debug, Clone)]
pub struct Check {
    pub body: Cond,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Term {
    Name(String),
    Next(String),
    Step,
}

#[derive(Debug, Clone)]
pub enum Cond {
    Eq(Term, Term),
    In(Term, Vec<Term>),
    And(Vec<Cond>),
    Implies(Box<Cond>, Box<Cond>),
    Iff(Box<Cond>, Box<Cond>),
    Forall(String, Vec<String>, Box<Cond>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mark {
    pub object: String,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    Holds,
    Fails {
        step: usize,
        evidence: Vec<String>,
        marks: Vec<Mark>,
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

type Env = Vec<(String, String)>;

type Event<'a> = (&'a str, &'a str, &'a str);

struct Ctx<'a> {
    cur: &'a State,
    nxt: &'a State,
    verb: Option<&'a str>,
}

struct Val {
    object: Option<String>,
    text: String,
}

enum Side<'a> {
    Object(&'a Object),
    Verb,
    Text(&'a str),
}

fn bound<'a>(env: &'a Env, o: &'a str) -> &'a str {
    env.iter()
        .rev()
        .find(|(v, _)| v == o)
        .map_or(o, |(_, x)| x.as_str())
}

fn bind(env: &Env, var: &str, object: &str) -> Env {
    let mut out = env.clone();
    out.push((var.to_string(), object.to_string()));
    out
}

fn same(d: &Diagram, a: &Option<Val>, b: &Option<Val>) -> bool {
    match (a, b) {
        (Some(x), Some(y)) => match (&x.object, &y.object) {
            (Some(o), None) => d.object(o).is_some_and(|ob| ob.within(&x.text, &y.text)),
            (None, Some(o)) => d.object(o).is_some_and(|ob| ob.within(&y.text, &x.text)),
            _ => x.text == y.text,
        },
        (None, None) => true,
        _ => false,
    }
}

fn list(names: &[String]) -> String {
    match names.split_last() {
        Some((last, rest)) if !rest.is_empty() => format!("{} and {last}", rest.join(", ")),
        _ => names.concat(),
    }
}

fn show(v: &Option<Val>) -> &str {
    v.as_ref().map_or("unset", |v| v.text.as_str())
}

impl Check {
    pub fn statement(&self, d: &Diagram) -> String {
        let words = self.body.words(d, &Vec::new());
        let mut chars = words.chars();
        chars
            .next()
            .map_or(String::new(), |c| c.to_uppercase().chain(chars).collect())
    }

    pub fn source(&self, d: &Diagram) -> String {
        let body = self.body.source(d, &Vec::new());
        match self.body.action() {
            true => format!("always({body})"),
            false => body,
        }
    }
}

fn third(verb: &str) -> String {
    match ["s", "sh", "ch", "x"].iter().any(|e| verb.ends_with(e)) {
        true => format!("{verb}es"),
        false => format!("{verb}s"),
    }
}

fn kind(d: &Diagram, objects: &[String]) -> String {
    let kinds: HashSet<&str> = objects
        .iter()
        .filter_map(|o| d.object(o))
        .map(|o| o.kind.as_str())
        .collect();
    match kinds.into_iter().collect::<Vec<_>>().as_slice() {
        [one] => format!("{one} "),
        _ => String::new(),
    }
}

impl Move {
    fn takes(&self, (actor, verb, target): Event) -> bool {
        self.actor == actor && self.verb == verb && self.target == target
    }

    fn event(&self) -> String {
        format!("on {} {} {}", self.actor, self.verb, self.target)
    }
}

type State = BTreeMap<String, String>;

impl Diagram {
    pub fn unstyled(&self, styles: &[(String, String)]) -> Option<&Object> {
        self.objects
            .iter()
            .find(|o| !styles.iter().any(|(k, _)| *k == o.kind))
    }

    fn object(&self, name: &str) -> Option<&Object> {
        self.objects.iter().find(|o| o.name == name)
    }

    pub fn initial(&self, name: &str) -> Option<&str> {
        self.init
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }

    pub fn verdict(&self) -> Option<Verdict> {
        let c = self.check.as_ref()?;
        let action = c.body.action();
        let mut now: State = self.init.iter().cloned().collect();
        for (step, s) in self.steps.iter().enumerate() {
            let mut next = now.clone();
            next.extend(s.set.iter().cloned());
            let ctx = match action {
                true => Ctx {
                    cur: &now,
                    nxt: &next,
                    verb: Some(&s.send.name),
                },
                false => Ctx {
                    cur: &next,
                    nxt: &next,
                    verb: None,
                },
            };
            if !c.body.holds(self, &Vec::new(), &ctx) {
                let (mut evidence, mut marks) = (Vec::new(), Vec::new());
                c.body.explain(self, &Vec::new(), &ctx, &mut evidence);
                c.body.marks(self, &Vec::new(), &ctx, &mut marks);
                return Some(Verdict::Fails {
                    step,
                    evidence,
                    marks,
                });
            }
            now = next;
        }
        Some(Verdict::Holds)
    }
}

impl Object {
    fn node(&self, name: &str) -> Option<usize> {
        self.nodes.iter().position(|n| n.name == name)
    }

    fn path(&self, name: &str) -> Vec<&Node> {
        let mut out = Vec::new();
        let mut at = self.node(name);
        while let Some(i) = at {
            out.push(&self.nodes[i]);
            at = self.nodes[i].parent;
        }
        out
    }

    fn leaf(&self, name: &str) -> Result<String, String> {
        let i = self
            .node(name)
            .ok_or_else(|| format!("`{name}` is not a state of `{}`", self.name))?;
        match (
            self.nodes.iter().any(|n| n.parent == Some(i)),
            &self.nodes[i].initial,
        ) {
            (false, _) => Ok(name.to_string()),
            (true, Some(inner)) => self.leaf(inner),
            (true, None) => Err(format!(
                "the state `{name}` of `{}` holds states and no `initial`",
                self.name
            )),
        }
    }

    fn values(&self) -> Vec<String> {
        self.nodes
            .iter()
            .enumerate()
            .filter(|(i, _)| !self.nodes.iter().any(|n| n.parent == Some(*i)))
            .map(|(_, n)| n.name.clone())
            .collect()
    }

    fn within(&self, leaf: &str, name: &str) -> bool {
        self.path(leaf).iter().any(|n| n.name == name)
    }

    fn overlap(&self, a: &Option<String>, b: &Option<String>) -> bool {
        match (a, b) {
            (Some(a), Some(b)) => self.within(a, b) || self.within(b, a),
            _ => true,
        }
    }

    fn validate(&self, objects: &[Object]) -> Result<(), String> {
        let o = &self.name;
        let mut names = HashSet::new();
        for n in &self.nodes {
            if !names.insert(n.name.as_str()) {
                return Err(format!("`{o}` declares the state `{}` twice", n.name));
            }
        }
        for (i, n) in self.nodes.iter().enumerate() {
            if let Some(inner) = &n.initial
                && self.node(inner).and_then(|j| self.nodes[j].parent) != Some(i)
            {
                return Err(format!(
                    "the `initial` of the state `{}` of `{o}` is not one of its states",
                    n.name
                ));
            }
            self.leaf(&n.name)?;
        }
        match (&self.initial, self.nodes.is_empty()) {
            (None, false) => return Err(format!("`{o}` has states and no `initial`")),
            (Some(_), true) => return Err(format!("`{o}` has `initial` and no states")),
            (Some(i), false) if self.node(i).is_none_or(|j| self.nodes[j].parent.is_some()) => {
                return Err(format!(
                    "the `initial` of `{o}` is not one of its top states"
                ));
            }
            _ => {}
        }
        if !self.moves.is_empty() && self.nodes.is_empty() {
            return Err(format!("`{o}` has `on` lines and no states"));
        }
        for (k, m) in self.moves.iter().enumerate() {
            for x in [&m.actor, &m.target] {
                if !objects.iter().any(|y| &y.name == x) {
                    return Err(format!(
                        "`{o}` has `{}` with unknown object `{x}`",
                        m.event()
                    ));
                }
            }
            let copies = |s: &String| objects.iter().any(|y| &y.name == s && y.value.is_some());
            for s in m.from.iter().chain(m.to.iter().filter(|s| !copies(s))) {
                if self.node(s).is_none() {
                    return Err(format!(
                        "`{o}` has `{}` with `{s}`, which is not a state of `{o}`",
                        m.event()
                    ));
                }
            }
            if self.moves[..k]
                .iter()
                .any(|p| p.event() == m.event() && self.overlap(&p.from, &m.from))
            {
                return Err(format!(
                    "`{o}` has two `{}` lines that start from the same state",
                    m.event()
                ));
            }
        }
        Ok(())
    }
}

fn note(o: &str, text: String) -> Mark {
    Mark {
        object: o.to_string(),
        text,
    }
}

impl Term {
    fn action(&self) -> bool {
        !matches!(self, Term::Name(_))
    }

    fn source(&self, d: &Diagram, vars: &[String]) -> String {
        match self {
            Term::Step => "step".into(),
            Term::Next(o) => format!("next({o})"),
            Term::Name(n) if vars.contains(n) || d.object(n).is_some() => n.clone(),
            Term::Name(n) => format!("\"{n}\""),
        }
    }

    fn noun(&self, d: &Diagram, vars: &[String]) -> String {
        match self {
            Term::Step => "the step".into(),
            Term::Next(o) => format!("the {o}"),
            Term::Name(n) if vars.contains(n) => format!("the {n}"),
            Term::Name(n) => match (d.object(n), d.steps.iter().find(|s| &s.to == n)) {
                (Some(_), Some(s)) => {
                    format!("the {n} that the {} {}", s.from, third(&s.send.name))
                }
                (Some(_), None) => format!("the {n}"),
                _ => n.clone(),
            },
        }
    }

    fn value(&self, d: &Diagram, env: &Env, c: &Ctx) -> Option<Val> {
        let leaf = |o: &str, state: &State| {
            state.get(o).map(|l| Val {
                object: Some(o.to_string()),
                text: l.clone(),
            })
        };
        match self {
            Term::Step => c.verb.map(|v| Val {
                object: None,
                text: v.to_string(),
            }),
            Term::Next(o) => leaf(bound(env, o), c.nxt),
            Term::Name(n) => match d.object(bound(env, n)) {
                Some(_) => leaf(bound(env, n), c.cur),
                None => Some(Val {
                    object: None,
                    text: n.clone(),
                }),
            },
        }
    }

    fn side<'a>(&'a self, d: &'a Diagram, env: &'a Env) -> Result<Side<'a>, String> {
        let object = |o: &str| match d.object(bound(env, o)) {
            Some(x) if x.nodes.is_empty() => Err(format!("`{}` has no states", x.name)),
            found => Ok(found.map(Side::Object)),
        };
        match self {
            Term::Step => Ok(Side::Verb),
            Term::Next(o) => object(o)?.ok_or_else(|| format!("`(next){o}` names no object")),
            Term::Name(n) => Ok(object(n)?.unwrap_or(Side::Text(n))),
        }
    }
}

fn compared(d: &Diagram, env: &Env, a: &Term, b: &Term) -> Result<(), String> {
    match (a.side(d, env)?, b.side(d, env)?) {
        (Side::Object(x), Side::Object(y)) => {
            match [x, y].into_iter().find(|o| o.value.is_none()) {
                Some(o) => Err(format!("`{}` needs `value=` to be compared", o.name)),
                None => Ok(()),
            }
        }
        (Side::Object(o), Side::Text(s)) | (Side::Text(s), Side::Object(o)) => match o.node(s) {
            Some(_) => Ok(()),
            None => Err(format!("`{s}` is not a state of `{}`", o.name)),
        },
        (Side::Verb, Side::Text(s)) | (Side::Text(s), Side::Verb) => {
            match d.steps.iter().any(|x| x.send.name == s)
                || d.objects.iter().flat_map(|o| &o.moves).any(|m| m.verb == s)
            {
                true => Ok(()),
                false => Err(format!("`{s}` is not a step")),
            }
        }
        _ => Err(
            "`eq` and `in` compare an object with an object or a state, or `step` with a step"
                .into(),
        ),
    }
}

impl Cond {
    fn action(&self) -> bool {
        match self {
            Cond::Eq(a, b) => a.action() || b.action(),
            Cond::In(a, all) => a.action() || all.iter().any(Term::action),
            Cond::And(all) => all.iter().any(Cond::action),
            Cond::Forall(_, _, a) => a.action(),
            Cond::Implies(a, b) | Cond::Iff(a, b) => a.action() || b.action(),
        }
    }

    fn source(&self, d: &Diagram, vars: &[String]) -> String {
        let pair = |op: &str, a: &Term, b: &Term| {
            format!("{op}({}, {})", a.source(d, vars), b.source(d, vars))
        };
        match self {
            Cond::Eq(a, b) => pair("eq", a, b),
            Cond::In(a, all) => format!(
                "in({}, Set({}))",
                a.source(d, vars),
                all.iter()
                    .map(|x| x.source(d, vars))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Cond::And(all) => format!(
                "and {{ {} }}",
                all.iter()
                    .map(|x| x.source(d, vars))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Cond::Implies(a, b) => format!("implies({}, {})", a.source(d, vars), b.source(d, vars)),
            Cond::Iff(a, b) => format!("iff({}, {})", a.source(d, vars), b.source(d, vars)),
            Cond::Forall(v, objects, body) => {
                let inner = [vars, std::slice::from_ref(v)].concat();
                format!(
                    "forall(Set({}), {v} => {})",
                    objects.join(", "),
                    body.source(d, &inner)
                )
            }
        }
    }

    fn steps(&self, d: &Diagram) -> Option<String> {
        let verb = |t: &Term| match t {
            Term::Name(n) => Some(n.clone()),
            _ => None,
        };
        match self {
            Cond::Eq(Term::Step, v) => verb(v).map(|v| format!("each {v}")),
            Cond::In(Term::Step, all) => {
                let verbs: Vec<String> = all.iter().filter_map(verb).collect();
                let taken: Vec<&Step> = d
                    .steps
                    .iter()
                    .filter(|s| verbs.contains(&s.send.name))
                    .collect();
                let place = match taken.split_first() {
                    Some((s, rest)) if rest.iter().all(|r| r.from == s.from && r.to == s.to) => {
                        format!(" by the {} on the {}", s.from, s.to)
                    }
                    _ => String::new(),
                };
                Some(format!("any combination of {}{place}", list(&verbs)))
            }
            _ => None,
        }
    }

    fn words(&self, d: &Diagram, env: &Env) -> String {
        let vars: Vec<String> = env.iter().map(|(v, _)| v.clone()).collect();
        let vars = vars.as_slice();
        let noun = |t: &Term| t.noun(d, vars);
        let owned = |t: &Term| match t {
            Term::Name(n) | Term::Next(n) => {
                match d.object(bound(env, n)).and_then(|o| o.value.as_ref()) {
                    Some(v) => format!("the {v} of {}", noun(t)),
                    None => noun(t),
                }
            }
            Term::Step => noun(t),
        };
        let literal =
            |t: &Term| matches!(t, Term::Name(n) if !vars.contains(n) && d.object(n).is_none());
        match self {
            Cond::Eq(Term::Next(x), Term::Name(y)) | Cond::Eq(Term::Name(y), Term::Next(x))
                if x == y =>
            {
                format!("{} persists", owned(&Term::Name(y.clone())))
            }
            Cond::Eq(a, b) if literal(b) => format!("{} is {}", noun(a), noun(b)),
            Cond::Eq(a, b) => format!("{} equals {}", owned(a), owned(b)),
            Cond::In(a, all) => format!(
                "{} is one of {}",
                noun(a),
                list(&all.iter().map(noun).collect::<Vec<_>>())
            ),
            Cond::And(all) => list(&all.iter().map(|x| x.words(d, env)).collect::<Vec<_>>()),
            Cond::Iff(a, b) => format!("{} exactly when {}", a.words(d, env), b.words(d, env)),
            Cond::Implies(a, b) => match (a.steps(d), b.as_ref()) {
                (Some(s), Cond::Eq(Term::Next(x), Term::Name(y))) if x == y => {
                    format!("{s} persists {}", owned(&Term::Name(y.clone())))
                }
                (Some(s), _) => format!("after {s}, {}", b.words(d, env)),
                (None, _) => format!("when {}, {}", a.words(d, env), b.words(d, env)),
            },
            Cond::Forall(v, objects, body) => {
                let k = kind(d, objects);
                let named = match k.trim_end() == v {
                    true => v.clone(),
                    false => format!("{k}{v}"),
                };
                format!(
                    "for any {named}, {}",
                    body.words(d, &bind(env, v, &objects[0]))
                )
            }
        }
    }

    fn holds(&self, d: &Diagram, env: &Env, c: &Ctx) -> bool {
        match self {
            Cond::Eq(a, b) => same(d, &a.value(d, env, c), &b.value(d, env, c)),
            Cond::In(a, all) => all
                .iter()
                .any(|x| same(d, &a.value(d, env, c), &x.value(d, env, c))),
            Cond::And(all) => all.iter().all(|x| x.holds(d, env, c)),
            Cond::Implies(a, b) => !a.holds(d, env, c) || b.holds(d, env, c),
            Cond::Iff(a, b) => a.holds(d, env, c) == b.holds(d, env, c),
            Cond::Forall(v, objects, body) => {
                objects.iter().all(|o| body.holds(d, &bind(env, v, o), c))
            }
        }
    }

    fn explain(&self, d: &Diagram, env: &Env, c: &Ctx, out: &mut Vec<String>) {
        match self {
            Cond::Eq(a, b) => {
                let vars: Vec<String> = env.iter().map(|(v, _)| v.clone()).collect();
                out.push(format!(
                    "{} = {} · {} · {}",
                    self.source(d, &vars),
                    self.holds(d, env, c),
                    show(&a.value(d, env, c)),
                    show(&b.value(d, env, c))
                ));
            }
            Cond::In(a, _) => {
                let vars: Vec<String> = env.iter().map(|(v, _)| v.clone()).collect();
                out.push(format!(
                    "{} = {} · {}",
                    self.source(d, &vars),
                    self.holds(d, env, c),
                    show(&a.value(d, env, c))
                ));
            }
            Cond::And(all) => all.iter().for_each(|x| x.explain(d, env, c, out)),
            Cond::Implies(a, b) | Cond::Iff(a, b) => {
                a.explain(d, env, c, out);
                b.explain(d, env, c, out);
            }
            Cond::Forall(v, objects, body) => {
                for o in objects {
                    let inner = bind(env, v, o);
                    if !body.holds(d, &inner, c) {
                        out.push(format!("{v} = {o}"));
                        body.explain(d, &inner, c, out);
                    }
                }
            }
        }
    }

    fn notes(&self, d: &Diagram, env: &Env, c: &Ctx, out: &mut Vec<Mark>, failed: bool) {
        match self {
            Cond::Eq(a, b) => {
                let (x, y) = (a.value(d, env, c), b.value(d, env, c));
                let object = x
                    .as_ref()
                    .and_then(|v| v.object.clone())
                    .or_else(|| y.as_ref().and_then(|v| v.object.clone()));
                let sign = match failed {
                    true => "≠",
                    false => "=",
                };
                if let Some(o) = object {
                    out.push(note(&o, format!("{} {sign} {}", show(&x), show(&y))));
                }
            }
            Cond::In(a, all) => {
                let x = a.value(d, env, c);
                let sign = match failed {
                    true => "∉",
                    false => "∈",
                };
                let names: Vec<String> = all
                    .iter()
                    .map(|t| show(&t.value(d, env, c)).to_string())
                    .collect();
                if let Some(o) = x.as_ref().and_then(|v| v.object.clone()) {
                    out.push(note(
                        &o,
                        format!("{} {sign} [{}]", show(&x), names.join(", ")),
                    ));
                }
            }
            Cond::And(all) => all
                .iter()
                .filter(|x| x.holds(d, env, c) != failed)
                .for_each(|x| x.notes(d, env, c, out, failed)),
            Cond::Implies(a, b) => match failed {
                true => b.notes(d, env, c, out, true),
                false => a.notes(d, env, c, out, true),
            },
            Cond::Iff(a, b) => {
                a.notes(d, env, c, out, !a.holds(d, env, c));
                b.notes(d, env, c, out, !b.holds(d, env, c));
            }
            Cond::Forall(v, objects, body) => {
                for o in objects {
                    let inner = bind(env, v, o);
                    if body.holds(d, &inner, c) != failed {
                        body.notes(d, &inner, c, out, failed);
                    }
                }
            }
        }
    }

    fn marks(&self, d: &Diagram, env: &Env, c: &Ctx, out: &mut Vec<Mark>) {
        if !self.holds(d, env, c) {
            self.notes(d, env, c, out, true);
        }
    }

    fn typed(&self, d: &Diagram, env: &Env) -> Result<(), String> {
        match self {
            Cond::Eq(a, b) => compared(d, env, a, b),
            Cond::In(a, all) => all.iter().try_for_each(|x| compared(d, env, a, x)),
            Cond::And(all) => all.iter().try_for_each(|x| x.typed(d, env)),
            Cond::Implies(a, b) | Cond::Iff(a, b) => a.typed(d, env).and_then(|_| b.typed(d, env)),
            Cond::Forall(v, objects, body) => {
                if d.object(v).is_some() || env.iter().any(|(x, _)| x == v) {
                    return Err(format!("`forall {v}` names an object or a bound name"));
                }
                for (i, o) in objects.iter().enumerate() {
                    if d.object(o).is_none() {
                        return Err(format!("unknown object `{o}`"));
                    }
                    if objects[..i].contains(o) {
                        return Err(format!("`forall {v}` names `{o}` twice"));
                    }
                    body.typed(d, &bind(env, v, o))?;
                }
                Ok(())
            }
        }
    }
}

fn name(n: &KdlNode) -> &str {
    n.name().value()
}

fn text(v: &KdlValue) -> String {
    match v {
        KdlValue::String(s) => s.clone(),
        KdlValue::Bool(b) => b.to_string(),
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

fn shape(n: &KdlNode, count: usize, allowed: &[&str], block: bool) -> Result<Vec<String>, String> {
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
        return Err(format!("`{}` holds no lines", name(n)));
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

fn first(n: &KdlNode) -> Result<String, String> {
    shape(n, 1, &[], false).map(|mut a| a.remove(0))
}

fn initial(slot: &mut Option<String>, n: &KdlNode) -> Result<(), String> {
    match slot.replace(first(n)?) {
        Some(_) => Err("`initial` is set twice".into()),
        None => Ok(()),
    }
}

fn state(n: &KdlNode, parent: Option<usize>, nodes: &mut Vec<Node>) -> Result<(), String> {
    let at = nodes.len();
    nodes.push(Node {
        name: shape(n, 1, &[], true)?.remove(0),
        parent,
        initial: None,
    });
    for c in n.iter_children() {
        match name(c) {
            "state" => state(c, Some(at), nodes)?,
            "initial" => initial(&mut nodes[at].initial, c)?,
            other => {
                return Err(format!(
                    "the state `{}` holds `state` and `initial`, not `{other}`",
                    nodes[at].name
                ));
            }
        }
    }
    Ok(())
}

fn on(n: &KdlNode) -> Result<Move, String> {
    let verb = shape(n, 1, &["from", "to", "state", "next"], false)?.remove(0);
    Ok(Move {
        actor: need(n, "from")?,
        verb,
        target: need(n, "to")?,
        from: n.get("state").map(text),
        to: n.get("next").map(text),
    })
}

fn object(n: &KdlNode) -> Result<Object, String> {
    let kind = name(n);
    let at = |e: String| format!("{kind}: {e}");
    let [object_name, label]: [String; 2] = shape(n, 2, &["value"], true)
        .map_err(at)?
        .try_into()
        .map_err(|_| at("takes a name and a label".into()))?;
    let at = |e: String| format!("{kind} {object_name}: {e}");
    let mut o = Object {
        name: object_name.clone(),
        kind: kind.to_string(),
        label,
        value: n.get("value").map(text),
        initial: None,
        nodes: Vec::new(),
        moves: Vec::new(),
    };
    for c in n.iter_children() {
        match name(c) {
            "state" => state(c, None, &mut o.nodes),
            "initial" => initial(&mut o.initial, c),
            "on" => on(c).map(|m| o.moves.push(m)),
            other => Err(format!("holds `state`, `initial` and `on`, not `{other}`")),
        }
        .map_err(at)?;
    }
    Ok(o)
}

fn data(n: &KdlNode) -> Result<Data, String> {
    let (given, named, held) = (args(n), keys(n), lines(n));
    match (given.as_slice(), named.is_empty(), held.is_empty()) {
        ([one], true, true) => Ok(Data::Text(one.clone())),
        ([_, _, ..], true, true) => Ok(Data::List(given.into_iter().map(Data::Text).collect())),
        ([], false, true) => Ok(Data::Record(
            named
                .into_iter()
                .map(|k| {
                    (
                        k.to_string(),
                        Data::Text(n.get(k).map(text).unwrap_or_default()),
                    )
                })
                .collect(),
        )),
        ([], true, false) if held.iter().all(|c| name(c) == "-") => held
            .iter()
            .map(data)
            .collect::<Result<_, _>>()
            .map(Data::List),
        ([], true, false) => record(held).map(Data::Record),
        _ => Err(format!(
            "`{}` holds one value, a list of values, fields, or lines",
            name(n)
        )),
    }
}

fn record(held: &[KdlNode]) -> Result<Vec<(String, Data)>, String> {
    let mut out: Vec<(String, Data)> = Vec::new();
    for c in held {
        if out.iter().any(|(k, _)| k == name(c)) {
            return Err(format!("`{}` is set twice", name(c)));
        }
        out.push((name(c).to_string(), data(c)?));
    }
    Ok(out)
}

fn step(n: &KdlNode) -> Result<Step, String> {
    let shows = keys(n)
        .into_iter()
        .filter(|k| !["from", "to"].contains(k))
        .map(|k| (k.to_string(), need(n, k).unwrap_or_default()))
        .collect();
    let allowed: Vec<&str> = keys(n);
    shape(n, 0, &allowed, true)?;
    Ok(Step {
        from: need(n, "from")?,
        to: need(n, "to")?,
        send: Class {
            name: name(n).to_string(),
            fields: record(lines(n))?,
        },
        shows,
        set: Vec::new(),
    })
}

fn term(e: &KdlEntry) -> Result<Term, String> {
    let v = text(e.value());
    match (e.ty().map(|t| t.value()), v.as_str()) {
        (None, "step") => Ok(Term::Step),
        (None, _) => Ok(Term::Name(v)),
        (Some("next"), _) => Ok(Term::Next(v)),
        (Some(t), _) => Err(format!("`({t})` is not `(next)`")),
    }
}

fn pair(n: &KdlNode) -> Result<(Term, Term), String> {
    shape(n, 2, &[], false)?;
    let terms = n
        .entries()
        .iter()
        .filter(|e| e.name().is_none())
        .map(term)
        .collect::<Result<Vec<_>, _>>()?;
    let [a, b]: [Term; 2] = terms
        .try_into()
        .map_err(|_| format!("`{}` takes two values", name(n)))?;
    Ok((a, b))
}

fn two(n: &KdlNode) -> Result<(Box<Cond>, Box<Cond>), String> {
    shape(n, 0, &[], true)?;
    match lines(n) {
        [a, b] => Ok((Box::new(cond(a)?), Box::new(cond(b)?))),
        _ => Err(format!("`{}` holds two conditions", name(n))),
    }
}

fn cond(n: &KdlNode) -> Result<Cond, String> {
    match name(n) {
        "eq" => pair(n).map(|(a, b)| Cond::Eq(a, b)),
        "in" => {
            let terms = n
                .entries()
                .iter()
                .filter(|e| e.name().is_none())
                .map(term)
                .collect::<Result<Vec<_>, _>>()?;
            shape(n, terms.len(), &[], false)?;
            match terms.split_first() {
                Some((a, all)) if all.len() > 1 => Ok(Cond::In(a.clone(), all.to_vec())),
                _ => Err("`in` takes a value and two or more values".into()),
            }
        }
        "and" => {
            shape(n, 0, &[], true)?;
            match lines(n) {
                held if held.len() > 1 => held
                    .iter()
                    .map(cond)
                    .collect::<Result<_, _>>()
                    .map(Cond::And),
                _ => Err("`and` holds two or more conditions".into()),
            }
        }
        "implies" => two(n).map(|(a, b)| Cond::Implies(a, b)),
        "iff" => two(n).map(|(a, b)| Cond::Iff(a, b)),
        "forall" => {
            let given = args(n);
            shape(n, given.len(), &[], true)?;
            match (given.as_slice(), lines(n)) {
                ([var, objects @ ..], [body]) if !objects.is_empty() => Ok(Cond::Forall(
                    var.clone(),
                    objects.to_vec(),
                    Box::new(cond(body)?),
                )),
                _ => Err(
                    "`forall` takes a name and one or more objects, and holds one condition".into(),
                ),
            }
        }
        other => Err(format!(
            "`{other}` is not eq, in, and, implies, iff or forall"
        )),
    }
}

fn invariant(n: &KdlNode) -> Result<Check, String> {
    shape(n, 0, &[], true)?;
    match lines(n) {
        [body] => Ok(Check { body: cond(body)? }),
        _ => Err("`invariant` holds one condition".into()),
    }
}

fn typed(d: &Diagram, c: &Check) -> Result<(), String> {
    c.body.typed(d, &Vec::new())?;
    let init: State = d.init.iter().cloned().collect();
    let ctx = Ctx {
        cur: &init,
        nxt: &init,
        verb: None,
    };
    match !c.body.action() && !c.body.holds(d, &Vec::new(), &ctx) {
        true => Err(format!("`{}` fails in the initial state", c.statement(d))),
        false => Ok(()),
    }
}

fn build(doc: &KdlDocument) -> Result<Diagram, String> {
    let mut d = Diagram::default();
    let mut seen = HashSet::new();
    for n in doc.nodes() {
        let parent = name(n);
        let holds = match parent {
            "setup" => "objects",
            "action" => "steps",
            "check" => "one `invariant`",
            other => return Err(format!("`{other}` is not setup, action or check")),
        };
        shape(n, 0, &[], true)?;
        if !seen.insert(parent) {
            return Err(format!("`{parent}` is set twice"));
        }
        for c in n.iter_children() {
            match (parent, name(c)) {
                ("setup", _) => d.objects.push(object(c)?),
                ("action", _) => d.steps.push(step(c)?),
                ("check", "invariant") if d.check.is_none() => d.check = Some(invariant(c)?),
                ("check", "invariant") => return Err("`check` holds one `invariant`".into()),
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
        if let Some(same) = d
            .objects
            .iter()
            .find(|x| x.name != o.name && x.kind == o.kind && x.label == o.label)
        {
            return Err(format!(
                "`{}` and `{}` are the same {}",
                o.name, same.name, o.kind
            ));
        }
        o.validate(&d.objects)?;
        if let Some(i) = &o.initial {
            d.init.push((o.name.clone(), o.leaf(i)?));
        }
    }
    run(&d.objects, &d.init, &mut d.steps)?;
    let c = d.check.as_ref().ok_or("needs `check`")?;
    typed(&d, c)?;
    explore(&d, c)?;
    let last = d.steps.len();
    match d.verdict() {
        Some(Verdict::Fails { step, .. }) if step + 1 == last => Ok(d),
        Some(Verdict::Fails { step, .. }) => Err(format!(
            "the run breaks `{}` at step {}, before its last step",
            c.statement(&d),
            step + 1
        )),
        _ => Err(format!("the run keeps `{}` at each step", c.statement(&d))),
    }
}

fn choices(
    objects: &[Object],
    now: &State,
    e: Event,
) -> Result<Vec<(String, Vec<String>)>, String> {
    let mut out = Vec::new();
    for o in objects {
        let lines: Vec<&Move> = o.moves.iter().filter(|m| m.takes(e)).collect();
        let Some(first) = lines.first() else {
            continue;
        };
        let at = now.get(&o.name).cloned().unwrap_or_default();
        let m = lines
            .iter()
            .find(|m| m.from.as_ref().is_none_or(|f| o.within(&at, f)))
            .ok_or_else(|| {
                format!(
                    "`{}` is `{at}`, and no `{}` line starts from `{at}`",
                    o.name,
                    first.event()
                )
            })?;
        let copied = m.to.as_ref().and_then(|t| now.get(t));
        match (&m.to, copied, &o.value) {
            (Some(_), Some(v), _) => out.push((o.name.clone(), vec![o.leaf(v)?])),
            (Some(t), None, _) => out.push((o.name.clone(), vec![o.leaf(t)?])),
            (None, _, Some(_)) => out.push((o.name.clone(), o.values())),
            (None, _, None) => {}
        }
    }
    Ok(out)
}

fn run(objects: &[Object], init: &[(String, String)], steps: &mut [Step]) -> Result<(), String> {
    let mut now: State = init.iter().cloned().collect();
    let last = steps.len();
    for (k, s) in steps.iter_mut().enumerate() {
        let at = |e: String| format!("step {}: {e}", k + 1);
        for x in [&s.from, &s.to]
            .into_iter()
            .chain(s.shows.iter().map(|(o, _)| o))
        {
            if !objects.iter().any(|o| &o.name == x) {
                return Err(at(format!("unknown object `{x}`")));
            }
        }
        let given = choices(objects, &now, (&s.from, &s.send.name, &s.to)).map_err(at)?;
        let mut set = Vec::new();
        for (name_of, options) in &given {
            let o = objects
                .iter()
                .find(|o| &o.name == name_of)
                .ok_or("unknown object")?;
            match (
                s.shows.iter().find(|(x, _)| x == name_of),
                options.as_slice(),
            ) {
                (Some((_, v)), _) => {
                    let shown = o.leaf(v).map_err(at)?;
                    if !options.contains(&shown) && k + 1 < last {
                        return Err(at(format!(
                            "`{name_of}` shows `{shown}`, and the setup gives it only {}",
                            list(options)
                        )));
                    }
                    set.push((name_of.clone(), shown));
                }
                (None, [one]) => set.push((name_of.clone(), one.clone())),
                (None, _) => {
                    return Err(at(format!(
                        "`{name_of}` takes any of {}, so the step names one with `{name_of}=`",
                        list(options)
                    )));
                }
            }
        }
        for (name_of, v) in &s.shows {
            if given.iter().any(|(x, _)| x == name_of) {
                continue;
            }
            if k + 1 < last {
                return Err(at(format!(
                    "`{name_of}` has no `{}` line, so only the last step changes it",
                    s.send.name
                )));
            }
            let o = objects
                .iter()
                .find(|o| &o.name == name_of)
                .ok_or("unknown object")?;
            set.push((name_of.clone(), o.leaf(v).map_err(at)?));
        }
        now.extend(set.iter().cloned());
        s.set = set;
    }
    Ok(())
}

fn explore(d: &Diagram, c: &Check) -> Result<(), String> {
    let mut events: Vec<Event> = Vec::new();
    for m in d.objects.iter().flat_map(|o| &o.moves) {
        let e = (m.actor.as_str(), m.verb.as_str(), m.target.as_str());
        if !events.contains(&e) {
            events.push(e);
        }
    }
    let start: State = d.init.iter().cloned().collect();
    let mut seen = HashSet::from([start.clone()]);
    let mut queue = VecDeque::from([(start, Vec::<String>::new())]);
    while let Some((now, trace)) = queue.pop_front() {
        for e in &events {
            let Ok(given) = choices(&d.objects, &now, *e) else {
                continue;
            };
            let mut nexts = vec![now.clone()];
            for (o, options) in &given {
                nexts = nexts
                    .iter()
                    .flat_map(|n| {
                        options.iter().map(move |v| {
                            let mut x = n.clone();
                            x.insert(o.clone(), v.clone());
                            x
                        })
                    })
                    .collect();
            }
            for next in nexts {
                let ctx = match c.body.action() {
                    true => Ctx {
                        cur: &now,
                        nxt: &next,
                        verb: Some(e.1),
                    },
                    false => Ctx {
                        cur: &next,
                        nxt: &next,
                        verb: None,
                    },
                };
                let shown: Vec<String> = given
                    .iter()
                    .filter_map(|(o, _)| next.get(o).map(|v| format!("{o}={v}")))
                    .collect();
                let path = [
                    trace.clone(),
                    vec![format!("{} {} {} ({})", e.0, e.1, e.2, shown.join(", "))],
                ]
                .concat();
                if !c.body.holds(d, &Vec::new(), &ctx) {
                    return Err(format!(
                        "`{}` fails in the setup after {}",
                        c.statement(d),
                        path.join(", ")
                    ));
                }
                if seen.insert(next.clone()) {
                    queue.push_back((next, path));
                }
            }
        }
    }
    Ok(())
}

fn document(src: &str) -> Result<KdlDocument, Error> {
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

pub struct Config {
    pub include: Vec<String>,
    pub theme: String,
    pub styles: Vec<(String, String)>,
}

pub fn config(src: &str) -> Result<Config, Error> {
    let doc = document(src)?;
    let mut c = Config {
        include: Vec::new(),
        theme: String::new(),
        styles: Vec::new(),
    };
    for n in doc.nodes() {
        match name(n) {
            "include" => c.include.extend(shape(n, 1, &[], false).map_err(Error)?),
            "theme" => c.theme = shape(n, 1, &[], false).map_err(Error)?.remove(0),
            "style" => {
                let kind = shape(n, 1, &["icon"], false).map_err(Error)?.remove(0);
                if c.styles.iter().any(|(k, _)| *k == kind) {
                    return Err(Error(format!("`style {kind}` is set twice")));
                }
                c.styles.push((kind, need(n, "icon").map_err(Error)?));
            }
            other => {
                return Err(Error(format!("`{other}` is not include, theme or style")));
            }
        }
    }
    match (c.include.is_empty(), c.theme.is_empty()) {
        (true, _) => Err(Error("needs `include \"<glob>\"`".into())),
        (_, true) => Err(Error("needs `theme light` or `theme dark`".into())),
        _ => Ok(c),
    }
}

pub fn parse(src: &str) -> Result<Diagram, Error> {
    build(&document(src)?).map_err(Error)
}
