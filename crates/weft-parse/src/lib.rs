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
    pub verb: String,
    pub from: String,
    pub to: String,
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
    pub initial: Option<String>,
    pub nodes: Vec<Node>,
    pub moves: Vec<Move>,
}

#[derive(Debug, Clone)]
pub struct Step {
    pub from: String,
    pub to: String,
    pub verb: String,
    pub shows: Vec<(String, String)>,
    pub set: Vec<(String, String)>,
}

#[derive(Debug, Clone)]
pub struct Check {
    pub kind: Kind,
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

    fn right(&self, bound: &str) -> String {
        match &self.expected {
            Expected::Before => bound.to_string(),
            Expected::State(s) => format!("\"{s}\""),
        }
    }

    fn set(&self) -> String {
        let verbs: Vec<String> = self.steps.iter().map(|s| format!("\"{s}\"")).collect();
        format!("Set({})", verbs.join(", "))
    }

    fn guard(&self) -> String {
        format!("in(step, {})", self.set())
    }

    fn compare(&self) -> String {
        let bound = self.kind.to_string();
        format!("eq(next({bound}), {})", self.right(&bound))
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
                    format!("{} = {o}", self.kind),
                    format!("in(\"{verb}\", {}) = true", self.set()),
                    format!(
                        "eq(next({o}), {}) = eq(\"{after}\", \"{want}\") = false",
                        self.right(o)
                    ),
                ]
            })
            .collect()
    }

    pub fn statement(&self, d: &Diagram) -> String {
        let sent: Vec<&Step> = d.steps.iter().filter(|s| self.taken(&s.verb)).collect();
        let place = match sent.split_first() {
            Some((s, rest)) if rest.iter().all(|r| r.from == s.from && r.to == s.to) => {
                format!(" by the {} on the {}", s.from, s.to)
            }
            _ => String::new(),
        };
        let steps = match self.steps.as_slice() {
            [one] => format!("each {one}{place}"),
            all => format!("any combination of {}{place}", list(all)),
        };
        let owned = format!("the state of the {}", self.kind);
        let words = match &self.expected {
            Expected::Before => format!("for any {}, {steps} persists {owned}", self.kind),
            Expected::State(s) => format!("for any {}, after {steps}, {owned} is {s}", self.kind),
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
            self.kind,
            self.guard(),
            self.compare()
        )
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
                verb: &s.verb,
            };
            let broken = c.broken(self, &ctx);
            if !broken.is_empty() {
                return Some(Verdict::Fails {
                    step,
                    evidence: c.evidence(&broken, &s.verb),
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
                "the state `{name}` of `{}` holds states and no `initial=`",
                self.name
            )),
        }
    }

    fn within(&self, leaf: &str, name: &str) -> bool {
        self.path(leaf).iter().any(|n| n.name == name)
    }

    fn validate(&self, steps: &[Step]) -> Result<(), String> {
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
                    "the `initial=` of the state `{}` of `{o}` is not one of its states",
                    n.name
                ));
            }
            self.leaf(&n.name)?;
        }
        let initial = match (&self.initial, self.nodes.is_empty(), self.kind) {
            (_, false, Kind::Person) => {
                return Err(format!("`{o}` is a person, and a person has no states"));
            }
            (None, true, _) => return Ok(()),
            (None, false, _) => return Err(format!("`{o}` has states and no `initial=`")),
            (Some(_), true, _) => return Err(format!("`{o}` has `initial=` and no states")),
            (Some(i), false, _) if self.node(i).is_none_or(|j| self.nodes[j].parent.is_some()) => {
                return Err(format!(
                    "the `initial=` of `{o}` is not one of its top states"
                ));
            }
            (Some(i), false, _) => self.leaf(i)?,
        };
        for (k, m) in self.moves.iter().enumerate() {
            if self.node(&m.to).is_none() {
                return Err(format!(
                    "the state `{}` of `{o}` has `on {} {}`, and `{}` is not a state of `{o}`",
                    m.from, m.verb, m.to, m.to
                ));
            }
            if self.moves[..k].iter().any(|p| {
                p.verb == m.verb && (self.within(&p.from, &m.from) || self.within(&m.from, &p.from))
            }) {
                return Err(format!(
                    "`{o}` has two `on {}` lines that start from the state `{}`",
                    m.verb, m.from
                ));
            }
        }
        let mut reached = vec![initial];
        let mut k = 0;
        while k < reached.len() {
            let at = reached[k].clone();
            for m in self.moves.iter().filter(|m| {
                self.within(&at, &m.from) && steps.iter().any(|s| &s.to == o && s.verb == m.verb)
            }) {
                let next = self.leaf(&m.to)?;
                if !reached.contains(&next) {
                    reached.push(next);
                }
            }
            k += 1;
        }
        match self
            .nodes
            .iter()
            .find(|n| !reached.iter().any(|r| self.within(r, &n.name)))
        {
            Some(n) => Err(format!(
                "no `action` reaches the state `{}` of `{o}`",
                n.name
            )),
            None => Ok(()),
        }
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
    if let Some(e) = n
        .entries()
        .iter()
        .find(|e| !matches!(e.value(), KdlValue::String(_)))
    {
        return Err(format!("`{}` takes names, not `{}`", name(n), e.value()));
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

fn state(n: &KdlNode, parent: Option<usize>, o: &mut Object) -> Result<(), String> {
    let at = o.nodes.len();
    o.nodes.push(Node {
        name: shape(n, 1, &["initial"], true)?.remove(0),
        parent,
        initial: n.get("initial").map(text),
    });
    for c in n.iter_children() {
        match name(c) {
            "state" => state(c, Some(at), o)?,
            "on" => o.moves.push(on(c, &o.nodes[at].name)?),
            other => {
                return Err(format!(
                    "the state `{}` holds `state` and `on`, not `{other}`",
                    o.nodes[at].name
                ));
            }
        }
    }
    Ok(())
}

fn on(n: &KdlNode, from: &str) -> Result<Move, String> {
    let [verb, to]: [String; 2] = shape(n, 2, &[], false)?
        .try_into()
        .map_err(|_| "`on` takes a verb and a state".to_string())?;
    Ok(Move {
        verb,
        from: from.to_string(),
        to,
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
    let object_name = shape(n, 1, &["initial"], true).map_err(at)?.remove(0);
    let at = |e: String| format!("{kind} {object_name}: {e}");
    let mut o = Object {
        name: object_name.clone(),
        kind,
        icon,
        initial: n.get("initial").map(text),
        nodes: Vec::new(),
        moves: Vec::new(),
    };
    for c in n.iter_children() {
        match name(c) {
            "state" => state(c, None, &mut o),
            other => Err(format!("holds `state`, not `{other}`")),
        }
        .map_err(at)?;
    }
    Ok(o)
}

fn step(n: &KdlNode) -> Result<Step, String> {
    let shows = keys(n)
        .into_iter()
        .filter(|k| !["from", "to"].contains(k))
        .map(|k| (k.to_string(), need(n, k).unwrap_or_default()))
        .collect();
    let allowed: Vec<&str> = keys(n);
    shape(n, 0, &allowed, false)?;
    Ok(Step {
        from: need(n, "from")?,
        to: need(n, "to")?,
        verb: name(n).to_string(),
        shows,
        set: Vec::new(),
    })
}

const CHECK: &str = "`invariant` holds `forall <kind> <object>… { implies { in step <verb>…; eq (next)<kind> <kind> } }`, or a state of each object in place of the last `<kind>`";

fn only<'a>(n: &'a KdlNode, node: &str) -> Result<&'a KdlNode, String> {
    match lines(n) {
        [one] if name(one) == node => Ok(one),
        _ => Err(format!("`{}` holds one `{node}`: {CHECK}", name(n))),
    }
}

fn plain(n: &KdlNode) -> Result<Vec<String>, String> {
    n.entries()
        .iter()
        .map(|e| match (e.name(), e.ty(), e.value()) {
            (None, None, KdlValue::String(s)) => Ok(s.clone()),
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
        kind: Kind::parse(&bound)?,
        objects,
        steps,
        expected,
    })
}

fn checked(d: &Diagram, c: &Check) -> Result<(), String> {
    if let Expected::State(s) = &c.expected
        && d.object(s).is_some()
    {
        return Err(format!(
            "`eq` compares `(next){}` with `{}` or a state, not with the object `{s}`",
            c.kind, c.kind
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
        match (x.kind == c.kind, x.nodes.is_empty()) {
            (false, _) => {
                return Err(format!(
                    "`forall {}` takes the kind of its objects, and `{o}` is of kind `{}`",
                    c.kind, x.kind
                ));
            }
            (true, true) => return Err(format!("`{o}` has no states")),
            (true, false) => {}
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
            "action" => "`<verb> from=<object> to=<object>` lines",
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
        o.validate(&d.steps)?;
        if let Some(i) = &o.initial {
            d.init.push((o.name.clone(), o.leaf(i)?));
        }
    }
    let c = d.check.clone().ok_or("needs `check`")?;
    checked(&d, &c)?;
    run(&d.objects, &d.init, &mut d.steps, &c)?;
    explore(&d, &c)?;
    let last = d.steps.len();
    match d.verdict() {
        Some(Verdict::Fails { step, .. }) if step + 1 == last => Ok(d),
        Some(Verdict::Fails { step, .. }) => Err(format!(
            "the run breaks `{}` at `action` line {}, before its last line",
            c.statement(&d),
            step + 1
        )),
        _ => Err(format!(
            "the run keeps `{}` at each `action` line",
            c.statement(&d)
        )),
    }
}

fn moved(o: &Object, now: &State, verb: &str) -> Result<String, String> {
    let at = now.get(&o.name).cloned().unwrap_or_default();
    match o
        .moves
        .iter()
        .find(|m| m.verb == verb && o.within(&at, &m.from))
    {
        Some(m) => o.leaf(&m.to),
        None if o.moves.iter().any(|m| m.verb == verb) => Err(format!(
            "`{}` is `{at}`, and no `on {verb}` line starts from `{at}`",
            o.name
        )),
        None => Err(format!("`{}` has no `on {verb}` line", o.name)),
    }
}

fn run(
    objects: &[Object],
    init: &[(String, String)],
    steps: &mut [Step],
    c: &Check,
) -> Result<(), String> {
    let mut now: State = init.iter().cloned().collect();
    let last = steps.len();
    for (k, s) in steps.iter_mut().enumerate() {
        let at = |e: String| format!("`action` line {}: {e}", k + 1);
        let find = |x: &String| {
            objects
                .iter()
                .find(|o| &o.name == x)
                .ok_or_else(|| at(format!("unknown object `{x}`")))
        };
        find(&s.from)?;
        let target = find(&s.to)?;
        let mut set = vec![(
            target.name.clone(),
            moved(target, &now, &s.verb).map_err(at)?,
        )];
        for (x, v) in &s.shows {
            let o = find(x)?;
            match (k + 1 == last, c.objects.contains(x)) {
                (false, _) => {
                    return Err(at(format!(
                        "`{x}={v}` is on a line before the last, and only the last line sets a state"
                    )));
                }
                (true, false) => {
                    return Err(at(format!(
                        "`{x}` is not an object of `forall`, and the last line sets only their states"
                    )));
                }
                (true, true) => set.push((x.clone(), o.leaf(v).map_err(at)?)),
            }
        }
        now.extend(set.iter().cloned());
        s.set = set;
    }
    Ok(())
}

fn explore(d: &Diagram, c: &Check) -> Result<(), String> {
    let mut events: Vec<(&Object, &str)> = Vec::new();
    for o in &d.objects {
        for m in &o.moves {
            if !events.iter().any(|(x, v)| x.name == o.name && *v == m.verb) {
                events.push((o, &m.verb));
            }
        }
    }
    let start: State = d.init.iter().cloned().collect();
    let mut seen = HashSet::from([start.clone()]);
    let mut queue = VecDeque::from([(start, Vec::<String>::new())]);
    while let Some((now, trace)) = queue.pop_front() {
        for (o, verb) in &events {
            let Ok(v) = moved(o, &now, verb) else {
                continue;
            };
            let mut next = now.clone();
            next.insert(o.name.clone(), v.clone());
            let ctx = Ctx {
                cur: &now,
                nxt: &next,
                verb,
            };
            let path = [
                trace.clone(),
                vec![format!("{verb} to={} ({}={v})", o.name, o.name)],
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
