use std::collections::{HashMap, HashSet};
use std::fmt;

use nom::{
    IResult, Parser,
    branch::alt,
    bytes::complete::{escaped_transform, is_not, tag, take_while1},
    character::complete::{char, multispace0, none_of, not_line_ending, space0, space1},
    combinator::{all_consuming, map, opt, recognize, value},
    multi::{many0, separated_list1},
    sequence::{delimited, preceded},
};

#[derive(Debug, Clone, Default)]
pub struct Diagram {
    pub title: String,
    pub objects: Vec<Object>,
    pub init: Vec<Assign>,
    pub steps: Vec<Step>,
    pub check: Option<Check>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Type {
    Person { name: String },
    Api { method: String, path: String },
    Db { table: String, key: String },
    File { store: String, path: String },
    Page { element: String, page: String },
    Code { symbol: String },
    Browser { storage: String },
}

impl Type {
    pub fn label(&self) -> String {
        match self {
            Type::Person { name } => name.clone(),
            Type::Api { method, path } => format!("{method} {path}"),
            Type::Db { table, key } => format!("{table} {key}"),
            Type::File { path, .. } => path.clone(),
            Type::Page { element, .. } => element.clone(),
            Type::Code { symbol } => symbol.clone(),
            Type::Browser { storage } => storage.clone(),
        }
    }

    pub fn tag(&self) -> String {
        match self {
            Type::Person { .. } => String::new(),
            Type::Api { .. } => "api".into(),
            Type::Db { .. } => "db row".into(),
            Type::File { store, .. } => format!("{store} file"),
            Type::Page { page, .. } => page.clone(),
            Type::Code { .. } => "code".into(),
            Type::Browser { .. } => "browser".into(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Object {
    pub name: String,
    pub ty: Type,
    pub machine: Vec<(String, String)>,
}

#[derive(Debug, Clone)]
pub struct Assign {
    pub object: String,
    pub value: String,
}

#[derive(Debug, Clone)]
pub struct Step {
    pub from: String,
    pub to: String,
    pub text: String,
    pub sets: Vec<Assign>,
}

#[derive(Debug, Clone)]
pub struct Check {
    pub title: String,
    pub when: Option<Cond>,
    pub then: Cond,
}

#[derive(Debug, Clone)]
pub enum Cond {
    From(String),
    Equal(Term, Term),
    Differ(Term, Term),
}

#[derive(Debug, Clone)]
pub enum Term {
    Now(String),
    Next(String),
    Text(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    pub caption: String,
    pub fields: Vec<(String, String)>,
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
pub struct Error {
    pub line: usize,
    pub message: String,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.line {
            0 => write!(f, "{}", self.message),
            n => write!(f, "line {n}: {}", self.message),
        }
    }
}

impl std::error::Error for Error {}

type State<'a> = HashMap<&'a str, &'a str>;

impl Diagram {
    pub fn verdict(&self) -> Option<Verdict> {
        let machines: Vec<&Object> = self
            .objects
            .iter()
            .filter(|o| !o.machine.is_empty())
            .collect();
        if self.check.is_none() && machines.is_empty() {
            return None;
        }
        let mut now: State = self
            .init
            .iter()
            .map(|a| (a.object.as_str(), a.value.as_str()))
            .collect();
        for (step, s) in self.steps.iter().enumerate() {
            let mut next = now.clone();
            next.extend(s.sets.iter().map(|a| (a.object.as_str(), a.value.as_str())));
            for o in &machines {
                if let (Some(a), Some(b)) = (now.get(o.name.as_str()), next.get(o.name.as_str()))
                    && a != b
                    && !o.machine.iter().any(|(x, y)| x == a && y == b)
                {
                    let reason = format!("{} has no transition {a} → {b}", o.ty.label());
                    return Some(Verdict::Fails {
                        step,
                        objects: vec![s.from.clone(), o.name.clone()],
                        reason,
                    });
                }
            }
            if let Some(c) = &self.check
                && c.when
                    .as_ref()
                    .is_none_or(|w| w.holds(&now, &next, &s.from))
                && !c.then.holds(&now, &next, &s.from)
            {
                let mut objects: Vec<String> = c.objects().into_iter().map(String::from).collect();
                objects.extend([s.from.clone(), s.to.clone()]);
                return Some(Verdict::Fails {
                    step,
                    objects,
                    reason: c.title.clone(),
                });
            }
            now = next;
        }
        Some(Verdict::Holds)
    }
}

impl Check {
    pub fn objects(&self) -> Vec<&str> {
        self.when
            .iter()
            .chain([&self.then])
            .flat_map(Cond::objects)
            .collect()
    }
}

impl Cond {
    fn holds(&self, now: &State, next: &State, from: &str) -> bool {
        match self {
            Cond::From(o) => o == from,
            Cond::Equal(a, b) => a.value(now, next) == b.value(now, next),
            Cond::Differ(a, b) => a.value(now, next) != b.value(now, next),
        }
    }

    fn objects(&self) -> Vec<&str> {
        match self {
            Cond::From(o) => vec![o.as_str()],
            Cond::Equal(a, b) | Cond::Differ(a, b) => {
                [a, b].into_iter().filter_map(Term::object).collect()
            }
        }
    }
}

impl Term {
    fn value<'a>(&'a self, now: &State<'a>, next: &State<'a>) -> &'a str {
        match self {
            Term::Now(o) => now.get(o.as_str()).copied().unwrap_or(""),
            Term::Next(o) => next.get(o.as_str()).copied().unwrap_or(""),
            Term::Text(t) => t,
        }
    }

    fn object(&self) -> Option<&str> {
        match self {
            Term::Now(o) | Term::Next(o) => Some(o),
            Term::Text(_) => None,
        }
    }
}

type Res<'a, T> = IResult<&'a str, T>;

enum Item {
    Title(String),
    Objects(Vec<Object>),
    Init(Vec<Assign>),
    Run(Vec<Line>),
    Check(Check),
}

enum Line {
    Set(Assign),
    Step(Step),
}

fn name(i: &str) -> Res<'_, String> {
    map(
        take_while1(|c: char| c.is_alphanumeric() || c == '_'),
        String::from,
    )
    .parse(i)
}

fn string(i: &str) -> Res<'_, String> {
    let escape = alt((
        value("\\", tag("\\")),
        value("\"", tag("\"")),
        value("\n", tag("n")),
    ));
    map(
        delimited(
            char('"'),
            opt(escaped_transform(none_of("\\\""), '\\', escape)),
            char('"'),
        ),
        Option::unwrap_or_default,
    )
    .parse(i)
}

fn rest(i: &str) -> Res<'_, String> {
    map(not_line_ending, |s: &str| s.trim().replace("\\n", "\n")).parse(i)
}

fn ty(i: &str) -> Res<'_, (String, Type)> {
    let words = |options: &'static [&'static str]| {
        move |i| -> Res<'_, String> {
            let (rest, w) = take_while1(|c: char| !c.is_whitespace() && c != '{')(i)?;
            match options.contains(&w) {
                true => Ok((rest, w.to_string())),
                false => Err(nom::Err::Error(nom::error::Error::new(
                    i,
                    nom::error::ErrorKind::Tag,
                ))),
            }
        }
    };
    let methods: &'static [&'static str] = &["GET", "POST", "PUT", "PATCH", "DELETE"];
    let stores: &'static [&'static str] = &["S3", "disk"];
    let storages: &'static [&'static str] =
        &["history.state", "localStorage", "sessionStorage", "cookie"];
    alt((
        map(
            (tag("person"), space1, name, space1, string),
            |(.., n, _, name)| (n, Type::Person { name }),
        ),
        map(
            (
                tag("api"),
                space1,
                name,
                space1,
                words(methods),
                space1,
                string,
            ),
            |(.., n, _, method, _, path)| (n, Type::Api { method, path }),
        ),
        map(
            (tag("db"), space1, name, space1, name, space1, string),
            |(.., n, _, table, _, key)| (n, Type::Db { table, key }),
        ),
        map(
            (
                tag("file"),
                space1,
                name,
                space1,
                words(stores),
                space1,
                string,
            ),
            |(.., n, _, store, _, path)| (n, Type::File { store, path }),
        ),
        map(
            (
                tag("page"),
                space1,
                name,
                space1,
                string,
                space1,
                tag("on"),
                space1,
                string,
            ),
            |(.., n, _, element, _, _, _, page)| (n, Type::Page { element, page }),
        ),
        map(
            (tag("code"), space1, name, space1, string),
            |(.., n, _, symbol)| (n, Type::Code { symbol }),
        ),
        map(
            (tag("browser"), space1, name, space1, words(storages)),
            |(.., n, _, storage)| (n, Type::Browser { storage }),
        ),
    ))
    .parse(i)
}

fn state(i: &str) -> Res<'_, String> {
    alt((string, map(is_not(" \t\r\n{}"), String::from))).parse(i)
}

fn transition(i: &str) -> Res<'_, (String, String)> {
    map((state, space0, tag("->"), space0, state), |(a, .., b)| {
        (a, b)
    })
    .parse(i)
}

fn object(i: &str) -> Res<'_, Object> {
    let machine = delimited(
        (space0, char('{')),
        many0(preceded(multispace0, transition)),
        (multispace0, char('}')),
    );
    map((ty, opt(machine)), |((name, ty), machine)| Object {
        name,
        ty,
        machine: machine.unwrap_or_default(),
    })
    .parse(i)
}

fn assign(i: &str) -> Res<'_, Assign> {
    map(
        (name, char('\''), space0, char('='), space0, rest),
        |(object, .., value)| Assign { object, value },
    )
    .parse(i)
}

fn step(i: &str) -> Res<'_, Step> {
    map(
        (
            name,
            space0,
            tag("->"),
            space0,
            name,
            space0,
            opt(preceded(char(':'), rest)),
        ),
        |(from, _, _, _, to, _, text)| Step {
            from,
            to,
            text: text.unwrap_or_default(),
            sets: Vec::new(),
        },
    )
    .parse(i)
}

fn line(i: &str) -> Res<'_, Line> {
    alt((map(assign, Line::Set), map(step, Line::Step))).parse(i)
}

fn block<'a, T>(
    keyword: &'static str,
    item: fn(&'a str) -> Res<'a, T>,
) -> impl Parser<&'a str, Output = Vec<T>, Error = nom::error::Error<&'a str>> {
    delimited(
        (tag(keyword), multispace0, char('{')),
        many0(preceded(multispace0, item)),
        (multispace0, char('}')),
    )
}

fn term(i: &str) -> Res<'_, Term> {
    alt((
        map(string, Term::Text),
        map((name, opt(char('\''))), |(o, p)| {
            if p.is_some() {
                Term::Next(o)
            } else {
                Term::Now(o)
            }
        }),
    ))
    .parse(i)
}

fn cond(i: &str) -> Res<'_, Cond> {
    alt((
        map(preceded((tag("from"), space1), name), Cond::From),
        map(
            (
                term,
                delimited(multispace0, alt((tag("!="), tag("="))), multispace0),
                term,
            ),
            |(a, op, b)| {
                if op == "=" {
                    Cond::Equal(a, b)
                } else {
                    Cond::Differ(a, b)
                }
            },
        ),
    ))
    .parse(i)
}

fn check(i: &str) -> Res<'_, Check> {
    map(
        (
            (tag("check"), space1),
            string,
            (multispace0, char('{'), multispace0),
            cond,
            opt(preceded(
                delimited(multispace0, tag("=>"), multispace0),
                cond,
            )),
            (multispace0, char('}')),
        ),
        |(_, title, _, first, second, _)| match second {
            Some(then) => Check {
                title,
                when: Some(first),
                then,
            },
            None => Check {
                title,
                when: None,
                then: first,
            },
        },
    )
    .parse(i)
}

fn item(i: &str) -> Res<'_, Item> {
    alt((
        map(preceded((tag("title"), space1), string), Item::Title),
        map(block("objects", object), Item::Objects),
        map(block("init", assign), Item::Init),
        map(block("run", line), Item::Run),
        map(check, Item::Check),
    ))
    .parse(i)
}

fn field(i: &str) -> Res<'_, (String, String)> {
    let quoted = recognize((char('"'), opt(is_not("\"")), char('"')));
    let list = recognize((char('['), opt(is_not("]")), char(']')));
    map(
        (
            delimited(
                multispace0,
                take_while1(|c: char| c.is_alphanumeric() || c == '_' || c == '-'),
                multispace0,
            ),
            char(':'),
            delimited(multispace0, alt((quoted, list, is_not(",}"))), multispace0),
        ),
        |(k, _, v): (&str, char, &str)| (k.to_string(), v.trim().to_string()),
    )
    .parse(i)
}

pub fn record(s: &str) -> Option<Record> {
    let fields = delimited(char('{'), separated_list1(char(','), field), char('}'));
    all_consuming(map(
        (opt(is_not("{")), fields),
        |(caption, fields): (Option<&str>, _)| Record {
            caption: caption.unwrap_or_default().trim().to_string(),
            fields,
        },
    ))
    .parse(s.trim())
    .ok()
    .map(|(_, r)| r)
}

pub fn parse(src: &str) -> Result<Diagram, Error> {
    let (rest, items) = many0(preceded(multispace0, item))
        .parse(src)
        .map_err(|_| Error {
            line: 1,
            message: "cannot parse".into(),
        })?;
    let rest = rest.trim_start();
    if !rest.is_empty() {
        let line = src[..src.len() - rest.len()].matches('\n').count() + 1;
        let text = rest.lines().next().unwrap_or_default();
        return Err(Error {
            line,
            message: format!("cannot parse `{text}`"),
        });
    }
    let fail = |message: String| Error { line: 0, message };
    let mut d = Diagram::default();
    for item in items {
        match item {
            Item::Title(t) => d.title = t,
            Item::Objects(o) => d.objects.extend(o),
            Item::Init(a) => d.init.extend(a),
            Item::Run(lines) => {
                for l in lines {
                    match l {
                        Line::Step(s) => d.steps.push(s),
                        Line::Set(a) => d
                            .steps
                            .last_mut()
                            .ok_or_else(|| fail(format!("`{}'` needs a step before it", a.object)))?
                            .sets
                            .push(a),
                    }
                }
            }
            Item::Check(c) => d.check = Some(c),
        }
    }
    if d.objects.is_empty() {
        return Err(fail("no objects".into()));
    }
    let mut known = HashSet::new();
    for o in &d.objects {
        if !known.insert(o.name.as_str()) {
            return Err(fail(format!("`{}` is declared twice", o.name)));
        }
    }
    let used = d
        .init
        .iter()
        .map(|a| a.object.as_str())
        .chain(d.steps.iter().flat_map(|s| {
            [s.from.as_str(), s.to.as_str()]
                .into_iter()
                .chain(s.sets.iter().map(|a| a.object.as_str()))
        }))
        .chain(d.check.iter().flat_map(Check::objects));
    for name in used {
        if !known.contains(name) {
            return Err(fail(format!("unknown object `{name}`")));
        }
    }
    Ok(d)
}
