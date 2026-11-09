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
                list(&Kind::ALL.map(|k| format!("`{k}`")))
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
    pub name: String,
    pub objects: Vec<String>,
    pub steps: Vec<String>,
    pub expected: Expected,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expected {
    Before,
    State(String),
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

type Event<'a> = (&'a str, &'a str, &'a str);

struct Ctx<'a> {
    cur: &'a State,
    nxt: &'a State,
    verb: &'a str,
}

type Broken = (String, String, String);

fn list(names: &[String]) -> String {
    match names.split_last() {
        Some((last, rest)) if !rest.is_empty() => format!("{} and {last}", rest.join(", ")),
        _ => names.concat(),
    }
}

impl Check {
    fn taken(&self, verb: &str) -> bool {
        self.steps.iter().any(|s| s == verb)
    }

    fn right(&self) -> String {
        match &self.expected {
            Expected::Before => self.name.clone(),
            Expected::State(s) => format!("\"{s}\""),
        }
    }

    fn guard(&self) -> String {
        let verbs: Vec<String> = self.steps.iter().map(|s| format!("\"{s}\"")).collect();
        format!("in(step, Set({}))", verbs.join(", "))
    }

    fn compare(&self) -> String {
        format!("eq(next({}), {})", self.name, self.right())
    }

    fn broken(&self, d: &Diagram, c: &Ctx) -> Vec<Broken> {
        let read = |state: &State, o: &str| state.get(o).cloned().unwrap_or_default();
        match self.taken(c.verb) {
            false => Vec::new(),
            true => self
                .objects
                .iter()
                .filter_map(|o| {
                    let after = read(c.nxt, o);
                    let (kept, want) = match &self.expected {
                        Expected::Before => {
                            let before = read(c.cur, o);
                            (after == before, before)
                        }
                        Expected::State(s) => {
                            (d.object(o).is_some_and(|x| x.within(&after, s)), s.clone())
                        }
                    };
                    (!kept).then(|| (o.clone(), after, want))
                })
                .collect(),
        }
    }

    fn evidence(&self, broken: &[Broken], verb: &str) -> Vec<String> {
        broken
            .iter()
            .flat_map(|(o, after, want)| {
                [
                    format!("{} = {o}", self.name),
                    format!("{} = true · {verb}", self.guard()),
                    format!("{} = false · {after} · {want}", self.compare()),
                ]
            })
            .collect()
    }

    pub fn statement(&self, d: &Diagram) -> String {
        let k = kind(d, &self.objects);
        let named = match k.trim_end() == self.name {
            true => self.name.clone(),
            false => format!("{k}{}", self.name),
        };
        let moves: Vec<&Move> = d
            .objects
            .iter()
            .flat_map(|o| &o.moves)
            .filter(|m| self.taken(&m.verb))
            .collect();
        let place = match moves.split_first() {
            Some((m, rest))
                if rest
                    .iter()
                    .all(|r| r.actor == m.actor && r.target == m.target) =>
            {
                format!(" by the {} on the {}", m.actor, m.target)
            }
            _ => String::new(),
        };
        let steps = match self.steps.as_slice() {
            [one] => format!("each {one}{place}"),
            all => format!("any combination of {}{place}", list(all)),
        };
        let value = self
            .objects
            .first()
            .and_then(|o| d.object(o))
            .and_then(|o| o.value.clone())
            .unwrap_or_default();
        let owned = format!("the {value} of the {}", self.name);
        let words = match &self.expected {
            Expected::Before => format!("for any {named}, {steps} persists {owned}"),
            Expected::State(s) => format!("for any {named}, after {steps}, {owned} is {s}"),
        };
        let mut chars = words.chars();
        chars
            .next()
            .map_or(String::new(), |c| c.to_uppercase().chain(chars).collect())
    }

    pub fn source(&self) -> String {
        format!(
            "always(forall(Set({}), {} => implies({}, {})))",
            self.objects.join(", "),
            self.name,
            self.guard(),
            self.compare()
        )
    }
}

fn kind(d: &Diagram, objects: &[String]) -> String {
    let kinds: HashSet<Kind> = objects
        .iter()
        .filter_map(|o| d.object(o))
        .map(|o| o.kind)
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
            let ctx = Ctx {
                cur: &now,
                nxt: &next,
                verb: &s.send.name,
            };
            let broken = c.broken(self, &ctx);
            if !broken.is_empty() {
                return Some(Verdict::Fails {
                    step,
                    evidence: c.evidence(&broken, &s.send.name),
                    marks: broken
                        .iter()
                        .map(|(o, after, want)| note(o, format!("{after} ≠ {want}")))
                        .collect(),
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

fn object(n: &KdlNode, config: &Config) -> Result<Object, String> {
    let kind = Kind::parse(name(n))?;
    let icon = config
        .styles
        .iter()
        .find(|(k, _)| *k == kind)
        .map(|(_, i)| i.clone())
        .ok_or_else(|| format!("`{kind}` has no `style {kind}` in the config"))?;
    let at = |e: String| format!("{kind}: {e}");
    let [object_name, label]: [String; 2] = shape(n, 2, &["value"], true)
        .map_err(at)?
        .try_into()
        .map_err(|_| at("takes a name and a label".into()))?;
    let at = |e: String| format!("{kind} {object_name}: {e}");
    let mut o = Object {
        name: object_name.clone(),
        kind,
        icon,
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

const CHECK: &str = "`invariant` holds `forall <name> <object>… { implies { in step <verb>…; eq (next)<name> <name> } }`, or a state of each object in place of the last `<name>`";

fn only<'a>(n: &'a KdlNode, node: &str) -> Result<&'a KdlNode, String> {
    match lines(n) {
        [one] if name(one) == node => Ok(one),
        _ => Err(format!("`{}` holds one `{node}`: {CHECK}", name(n))),
    }
}

fn plain(n: &KdlNode) -> Result<Vec<String>, String> {
    n.entries()
        .iter()
        .map(|e| match (e.name(), e.ty()) {
            (None, None) => Ok(text(e.value())),
            _ => Err(format!(
                "`{}` takes plain names, not `{e}`: {CHECK}",
                name(n)
            )),
        })
        .collect()
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
    let implies = only(forall, "implies")?;
    let (guard, compare) = match lines(implies) {
        [g, c] if name(g) == "in" && name(c) == "eq" && implies.entries().is_empty() => (g, c),
        _ => {
            return Err(format!(
                "`implies` holds `in step <verb>…` and then `eq (next){bound} …`: {CHECK}"
            ));
        }
    };
    let steps = match plain(guard)?.split_first() {
        Some((s, verbs)) if s == "step" && !verbs.is_empty() && guard.children().is_none() => {
            verbs.to_vec()
        }
        _ => return Err(format!("`in` takes `step` and one or more verbs: {CHECK}")),
    };
    let expected = match (compare.entries(), compare.children()) {
        ([after, right], None)
            if after.name().is_none()
                && after.ty().map(|t| t.value()) == Some("next")
                && text(after.value()) == bound
                && right.name().is_none()
                && right.ty().is_none() =>
        {
            match text(right.value()) {
                v if v == bound => Expected::Before,
                v => Expected::State(v),
            }
        }
        _ => {
            return Err(format!(
                "`eq` takes `(next){bound}` and then `{bound}` or a state: {CHECK}"
            ));
        }
    };
    Ok(Check {
        name: bound,
        objects,
        steps,
        expected,
    })
}

fn checked(d: &Diagram, c: &Check) -> Result<(), String> {
    if d.object(&c.name).is_some() {
        return Err(format!(
            "`forall {}` names an object, and needs a new name",
            c.name
        ));
    }
    if let Expected::State(s) = &c.expected
        && d.object(s).is_some()
    {
        return Err(format!(
            "`eq` compares `(next){}` with `{}` or a state, not with the object `{s}`",
            c.name, c.name
        ));
    }
    let verbs: Vec<&str> = d
        .objects
        .iter()
        .flat_map(|o| &o.moves)
        .map(|m| m.verb.as_str())
        .collect();
    for (i, s) in c.steps.iter().enumerate() {
        if c.steps[..i].contains(s) {
            return Err(format!("`in step` names `{s}` twice"));
        }
        if !verbs.contains(&s.as_str()) {
            return Err(format!("`{s}` is the verb of no `on` line"));
        }
    }
    for (i, o) in c.objects.iter().enumerate() {
        let x = d
            .object(o)
            .ok_or_else(|| format!("`forall` names unknown object `{o}`"))?;
        if c.objects[..i].contains(o) {
            return Err(format!("`forall` names `{o}` twice"));
        }
        if x.nodes.is_empty() {
            return Err(format!("`{o}` has no states"));
        }
        if x.value.is_none() {
            return Err(format!("`{o}` needs `value=` to be checked"));
        }
        if let Expected::State(s) = &c.expected
            && x.node(s).is_none()
        {
            return Err(format!("`{s}` is not a state of `{o}`"));
        }
        if let Some(m) = x.moves.iter().find(|m| c.taken(&m.verb)) {
            return Err(format!(
                "`{o}` has `on {}`, so `setup` decides what `check` compares",
                m.verb
            ));
        }
    }
    Ok(())
}

fn build(doc: &KdlDocument, config: &Config) -> Result<Diagram, String> {
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
                ("setup", _) => d.objects.push(object(c, config)?),
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
    checked(&d, c)?;
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
                let ctx = Ctx {
                    cur: &now,
                    nxt: &next,
                    verb: e.1,
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
                if !c.broken(d, &ctx).is_empty() {
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
    pub styles: Vec<(Kind, String)>,
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
                let named = shape(n, 1, &["icon"], false).map_err(Error)?.remove(0);
                let kind = Kind::parse(&named).map_err(|e| Error(format!("`style`: {e}")))?;
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

pub fn parse(src: &str, config: &Config) -> Result<Diagram, Error> {
    build(&document(src)?, config).map_err(Error)
}
