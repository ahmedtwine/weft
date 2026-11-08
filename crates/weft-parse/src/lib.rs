use std::collections::HashMap;
use std::fmt;

use nom::{
    IResult, Parser,
    branch::alt,
    bytes::complete::{escaped_transform, tag, take_while1},
    character::complete::{char, multispace0, none_of, not_line_ending, space0, space1},
    combinator::{map, opt, value},
    multi::many0,
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

#[derive(Debug, Clone)]
pub struct Object {
    pub name: String,
    pub label: String,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Holds,
    Fails(usize),
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
        let check = self.check.as_ref()?;
        let mut now: State = self
            .init
            .iter()
            .map(|a| (a.object.as_str(), a.value.as_str()))
            .collect();
        for (k, step) in self.steps.iter().enumerate() {
            let mut next = now.clone();
            next.extend(
                step.sets
                    .iter()
                    .map(|a| (a.object.as_str(), a.value.as_str())),
            );
            let when = check
                .when
                .as_ref()
                .is_none_or(|c| c.holds(&now, &next, &step.from));
            if when && !check.then.holds(&now, &next, &step.from) {
                return Some(Verdict::Fails(k));
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

fn object(i: &str) -> Res<'_, Object> {
    map((name, space1, string), |(name, _, label)| Object {
        name,
        label,
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
            tag("check"),
            space1,
            string,
            multispace0,
            char('{'),
            multispace0,
            cond,
            opt(preceded(
                delimited(multispace0, tag("=>"), multispace0),
                cond,
            )),
            multispace0,
            char('}'),
        ),
        |(_, _, title, _, _, _, first, second, _, _)| match second {
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

pub fn parse(src: &str) -> Result<Diagram, Error> {
    let line_of = |rest: &str| src[..src.len() - rest.len()].matches('\n').count() + 1;
    let (rest, items) = many0(preceded(multispace0, item))
        .parse(src)
        .map_err(|_| Error {
            line: 1,
            message: "cannot parse".into(),
        })?;
    let rest_trimmed = rest.trim_start();
    if !rest_trimmed.is_empty() {
        let text = rest_trimmed.lines().next().unwrap_or_default();
        return Err(Error {
            line: line_of(rest_trimmed),
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
    let mut known = std::collections::HashSet::new();
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
