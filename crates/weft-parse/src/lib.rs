use std::collections::{HashMap, HashSet};
use std::fmt;

use kdl::KdlValue;
pub use kdl::{KdlDocument, KdlNode};
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
    pub set: Vec<(String, String)>,
}

#[derive(Debug, Clone)]
pub struct Check {
    pub statement: String,
    pub when: Option<Cond>,
    pub then: Cond,
}

#[derive(Debug, Clone)]
pub struct Pattern {
    pub from: Option<String>,
    pub verb: Option<String>,
    pub to: Option<String>,
}

#[derive(Debug, Clone)]
pub enum Cond {
    Step(Pattern),
    Changed(String),
    Is(String, String),
    In(String, Vec<String>),
    And(Box<Cond>, Box<Cond>),
    Or(Box<Cond>, Box<Cond>),
    Not(Box<Cond>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mark {
    Note { object: String, text: String },
    Expect { from: String, to: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    Holds,
    Fails {
        statement: String,
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

impl Pattern {
    fn matches(&self, s: &Step) -> bool {
        let same = |p: &Option<String>, v: &str| p.as_ref().is_none_or(|p| p == v);
        same(&self.from, &s.from) && same(&self.verb, &s.send.name) && same(&self.to, &s.to)
    }

    fn words(&self) -> String {
        let parts: Vec<String> = [("from", &self.from), ("verb", &self.verb), ("to", &self.to)]
            .into_iter()
            .filter_map(|(k, v)| v.as_ref().map(|v| format!("{k}={v}")))
            .collect();
        format!("step {}", parts.join(" "))
    }
}

impl Move {
    fn takes(&self, s: &Step) -> bool {
        self.actor == s.from && self.verb == s.send.name && self.target == s.to
    }

    fn event(&self) -> String {
        format!("on {} {} {}", self.actor, self.verb, self.target)
    }
}

type State = HashMap<String, String>;

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
        let mut now: State = self.init.iter().cloned().collect();
        for (step, s) in self.steps.iter().enumerate() {
            let mut next = now.clone();
            next.extend(s.set.iter().cloned());
            if c.when
                .as_ref()
                .is_none_or(|w| w.holds(self, &now, &next, s))
                && !c.then.holds(self, &now, &next, s)
            {
                let mut evidence = Vec::new();
                c.when
                    .iter()
                    .chain([&c.then])
                    .for_each(|x| x.explain(self, &now, &next, s, &mut evidence));
                let mut marks = Vec::new();
                c.then.marks(self, &now, &next, s, &mut marks);
                return Some(Verdict::Fails {
                    statement: c.statement.clone(),
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
            for s in m.from.iter().chain(&m.to) {
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

fn is(d: &Diagram, state: &State, o: &str, name: &str) -> bool {
    match (d.object(o), state.get(o)) {
        (Some(x), Some(leaf)) => x.within(leaf, name),
        _ => false,
    }
}

fn note(o: &str, text: String) -> Mark {
    Mark::Note {
        object: o.to_string(),
        text,
    }
}

fn shown(v: Option<&String>) -> &str {
    v.map_or("unset", String::as_str)
}

impl Cond {
    fn words(&self) -> String {
        match self {
            Cond::Step(p) => p.words(),
            Cond::Changed(o) => format!("changed {o}"),
            Cond::Is(o, s) => format!("is {o} {s}"),
            Cond::In(o, all) => format!("is {o} {}", all.join(" ")),
            Cond::And(a, b) => format!("{} and {}", a.words(), b.words()),
            Cond::Or(a, b) => format!("{} or {}", a.words(), b.words()),
            Cond::Not(a) => format!("not {}", a.words()),
        }
    }

    fn holds(&self, d: &Diagram, now: &State, next: &State, s: &Step) -> bool {
        match self {
            Cond::Step(p) => p.matches(s),
            Cond::Changed(o) => now.get(o) != next.get(o),
            Cond::Is(o, st) => is(d, next, o, st),
            Cond::In(o, all) => all.iter().any(|st| is(d, next, o, st)),
            Cond::And(a, b) => a.holds(d, now, next, s) && b.holds(d, now, next, s),
            Cond::Or(a, b) => a.holds(d, now, next, s) || b.holds(d, now, next, s),
            Cond::Not(a) => !a.holds(d, now, next, s),
        }
    }

    fn explain(&self, d: &Diagram, now: &State, next: &State, s: &Step, out: &mut Vec<String>) {
        let holds = self.holds(d, now, next, s);
        let value = format!("{} = {holds}", self.words());
        match self {
            Cond::Step(_) => out.push(value),
            Cond::Changed(o) => out.push(format!(
                "{value} · {} → {}",
                shown(now.get(o)),
                shown(next.get(o))
            )),
            Cond::Is(o, _) | Cond::In(o, _) => out.push(match holds {
                true => value,
                false => format!("{value} · {o} is {}", shown(next.get(o))),
            }),
            Cond::And(a, b) | Cond::Or(a, b) => {
                a.explain(d, now, next, s, out);
                b.explain(d, now, next, s, out);
            }
            Cond::Not(a) => a.explain(d, now, next, s, out),
        }
    }

    fn marks(&self, d: &Diagram, now: &State, next: &State, s: &Step, out: &mut Vec<Mark>) {
        if self.holds(d, now, next, s) {
            return;
        }
        match self {
            Cond::Step(p) => out.push(Mark::Expect {
                from: p.from.clone().unwrap_or_else(|| s.from.clone()),
                to: p.to.clone().unwrap_or_else(|| s.to.clone()),
            }),
            Cond::Changed(o) => out.push(note(o, shown(next.get(o)).to_string())),
            Cond::Is(o, st) => out.push(note(o, format!("{} ≠ {st}", shown(next.get(o))))),
            Cond::In(o, all) => out.push(note(
                o,
                format!("{} ∉ [{}]", shown(next.get(o)), all.join(", ")),
            )),
            Cond::And(a, b) | Cond::Or(a, b) => {
                a.marks(d, now, next, s, out);
                b.marks(d, now, next, s, out);
            }
            Cond::Not(a) => a.held(d, next, out),
        }
    }

    fn held(&self, d: &Diagram, next: &State, out: &mut Vec<Mark>) {
        match self {
            Cond::Is(o, st) if is(d, next, o, st) => out.push(note(o, st.clone())),
            Cond::In(o, all) if all.iter().any(|st| is(d, next, o, st)) => {
                out.push(note(o, shown(next.get(o)).to_string()))
            }
            Cond::And(a, b) | Cond::Or(a, b) => {
                a.held(d, next, out);
                b.held(d, next, out);
            }
            _ => {}
        }
    }

    fn typed(&self, d: &Diagram) -> Result<(), String> {
        let known = |o: &str| d.object(o).ok_or_else(|| format!("unknown object `{o}`"));
        let state = |o: &str, s: &str| match known(o)? {
            x if x.node(s).is_some() => Ok(()),
            x => Err(format!("`{s}` is not a state of `{}`", x.name)),
        };
        match self {
            Cond::Step(p) => p
                .from
                .iter()
                .chain(&p.to)
                .try_for_each(|o| known(o).map(|_| ())),
            Cond::Changed(o) => match known(o)? {
                x if x.nodes.is_empty() => Err(format!("`{o}` has no states")),
                _ => Ok(()),
            },
            Cond::Is(o, s) => state(o, s),
            Cond::In(o, all) => all.iter().try_for_each(|s| state(o, s)),
            Cond::And(a, b) | Cond::Or(a, b) => a.typed(d).and_then(|_| b.typed(d)),
            Cond::Not(a) => a.typed(d),
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
    let [object_name, label]: [String; 2] = shape(n, 2, &[], true)
        .map_err(at)?
        .try_into()
        .map_err(|_| at("takes a name and a label".into()))?;
    let at = |e: String| format!("{kind} {object_name}: {e}");
    let mut o = Object {
        name: object_name.clone(),
        kind: kind.to_string(),
        label,
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
    shape(n, 0, &["from", "to"], true)?;
    Ok(Step {
        from: need(n, "from")?,
        to: need(n, "to")?,
        send: Class {
            name: name(n).to_string(),
            fields: record(lines(n))?,
        },
        set: Vec::new(),
    })
}

fn all(
    held: &[KdlNode],
    what: &str,
    join: fn(Box<Cond>, Box<Cond>) -> Cond,
) -> Result<Cond, String> {
    held.iter()
        .map(cond)
        .reduce(|a, b| Ok(join(Box::new(a?), Box::new(b?))))
        .unwrap_or_else(|| Err(format!("`{what}` holds no condition")))
}

fn cond(n: &KdlNode) -> Result<Cond, String> {
    match name(n) {
        "step" => {
            shape(n, 0, &["from", "verb", "to"], false)?;
            let get = |k: &str| n.get(k).map(text);
            match keys(n).is_empty() {
                true => Err("`step` needs from=, verb= or to=".into()),
                false => Ok(Cond::Step(Pattern {
                    from: get("from"),
                    verb: get("verb"),
                    to: get("to"),
                })),
            }
        }
        "is" => {
            let given = args(n);
            shape(n, given.len(), &[], false)?;
            match given.as_slice() {
                [o, s] => Ok(Cond::Is(o.clone(), s.clone())),
                [o, rest @ ..] if rest.len() > 1 => Ok(Cond::In(o.clone(), rest.to_vec())),
                _ => Err("`is` takes an object and one or more states".into()),
            }
        }
        "changed" => first(n).map(Cond::Changed),
        "not" => {
            shape(n, 0, &[], true)?;
            match lines(n) {
                [c] => cond(c).map(|c| Cond::Not(Box::new(c))),
                _ => Err("`not` holds one condition".into()),
            }
        }
        "any" => shape(n, 0, &[], true).and_then(|_| all(lines(n), "any", Cond::Or)),
        "all" => shape(n, 0, &[], true).and_then(|_| all(lines(n), "all", Cond::And)),
        other => Err(format!(
            "`{other}` is not step, is, changed, not, any or all"
        )),
    }
}

fn invariant(n: &KdlNode) -> Result<Check, String> {
    let statement = shape(n, 1, &[], true)?.remove(0);
    let (mut when, mut then) = (None, None);
    for c in n.iter_children() {
        shape(c, 0, &[], true)?;
        let slot = match name(c) {
            "when" => &mut when,
            "then" => &mut then,
            other => {
                return Err(format!(
                    "`invariant` holds `when` and `then`, not `{other}`"
                ));
            }
        };
        if slot.replace(all(lines(c), name(c), Cond::And)?).is_some() {
            return Err(format!("`{}` is set twice", name(c)));
        }
    }
    Ok(Check {
        statement,
        when,
        then: then.ok_or("an invariant needs `then`")?,
    })
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
    if let Some(c) = &d.check {
        c.when
            .iter()
            .chain([&c.then])
            .try_for_each(|x| x.typed(&d))?;
    }
    Ok(d)
}

fn run(objects: &[Object], init: &[(String, String)], steps: &mut [Step]) -> Result<(), String> {
    let mut now: State = init.iter().cloned().collect();
    for (k, s) in steps.iter_mut().enumerate() {
        for x in [&s.from, &s.to] {
            if !objects.iter().any(|o| &o.name == x) {
                return Err(format!("step {}: unknown object `{x}`", k + 1));
            }
        }
        let mut moves = Vec::new();
        for o in objects {
            let lines: Vec<&Move> = o.moves.iter().filter(|m| m.takes(s)).collect();
            let Some(first) = lines.first() else {
                continue;
            };
            let at = now.get(&o.name).cloned().unwrap_or_default();
            let m = lines
                .iter()
                .find(|m| m.from.as_ref().is_none_or(|f| o.within(&at, f)))
                .ok_or_else(|| {
                    format!(
                        "step {}: `{}` is `{at}`, and no `{}` line starts from `{at}`",
                        k + 1,
                        o.name,
                        first.event()
                    )
                })?;
            if let Some(t) = &m.to {
                moves.push((o.name.clone(), o.leaf(t)?));
            }
        }
        now.extend(moves.iter().cloned());
        s.set = moves;
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
