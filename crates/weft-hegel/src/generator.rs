use std::net::{Ipv4Addr, Ipv6Addr};
use std::ptr;

use hegel_c::hegel_result_t::HEGEL_E_NOT_FOUND;
use hegel_c::{
    HEGEL_MAX_SIZE_DEFAULT, HegelGrammar, HegelRecursion, HegelStringGenerator, hegel_date_t,
    hegel_datetime_t, hegel_generate_bytes_result_t, hegel_string_t, hegel_time_t,
};
use serde_json::{Map, Value as Json};

use crate::engine::{Case, Halt, bytes, text};

const DOMAIN: u64 = 255;
const DEPTH: u64 = 32;
const LEAVES: u64 = 100;
const BASE64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
const NONE: hegel_string_t = hegel_string_t {
    data: ptr::null(),
    len: 0,
};
const FIRST: hegel_date_t = hegel_date_t {
    year: 1,
    month: 1,
    day: 1,
};
const LAST: hegel_date_t = hegel_date_t {
    year: 9999,
    month: 12,
    day: 31,
};
const MIDNIGHT: hegel_time_t = hegel_time_t {
    hour: 0,
    minute: 0,
    second: 0,
    nanosecond: 0,
};
const NIGHT: hegel_time_t = hegel_time_t {
    hour: 23,
    minute: 59,
    second: 59,
    nanosecond: 0,
};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Bound {
    Open,
    At(f64),
    Past(f64),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Generator {
    Boolean,
    Integer(i64, i64),
    Float(Bound, Bound),
    Bytes(u64, Option<u64>),
    Text(u64, Option<u64>),
    Regex(String),
    Email,
    Url,
    Domain,
    Uuid,
    Ipv4,
    Ipv6,
    Date,
    Time,
    Datetime,
    Grammar(String, Option<String>),
    Constant(Json),
    SampledFrom(Vec<Json>),
    OneOf(Vec<Generator>),
    Optional(Box<Generator>),
    List(Box<Generator>, u64, Option<u64>, bool),
    Dict(Box<Generator>, Box<Generator>),
    Tuple(Vec<Generator>),
    Record(Vec<(String, Generator)>),
    Recursive(String, Box<Generator>, Option<u64>, Option<u64>),
    Subtree(String),
    Add(String, Box<Generator>),
    Pool(String, bool),
    Event(String, Box<Generator>),
}

impl Generator {
    pub fn ends(&self, closed: &[&str]) -> bool {
        match self {
            Generator::Subtree(name) => !closed.contains(&name.as_str()),
            Generator::Recursive(_, body, ..)
            | Generator::Add(_, body)
            | Generator::Event(_, body) => body.ends(closed),
            Generator::OneOf(all) => all.iter().any(|g| g.ends(closed)),
            Generator::List(item, min, ..) => *min == 0 || item.ends(closed),
            Generator::Tuple(all) => all.iter().all(|g| g.ends(closed)),
            Generator::Record(fields) => fields.iter().all(|(_, g)| g.ends(closed)),
            _ => true,
        }
    }

    pub fn loops(&self, name: &str) -> bool {
        match self {
            Generator::Subtree(known) => known == name,
            Generator::Recursive(known, body, ..) => known != name && body.loops(name),
            Generator::Optional(body) | Generator::Add(_, body) | Generator::Event(_, body) => {
                body.loops(name)
            }
            Generator::OneOf(all) | Generator::Tuple(all) => all.iter().any(|g| g.loops(name)),
            Generator::List(item, ..) => item.loops(name),
            Generator::Dict(key, value) => key.loops(name) || value.loops(name),
            Generator::Record(fields) => fields.iter().any(|(_, g)| g.loops(name)),
            _ => false,
        }
    }

    fn kind(&self) -> &'static str {
        match self {
            Generator::Boolean => "hegel.boolean",
            Generator::Integer(..) => "hegel.integer",
            Generator::Float(..) => "hegel.float",
            Generator::Bytes(..) => "hegel.bytes",
            Generator::Text(..) => "hegel.text",
            Generator::Regex(_) => "hegel.regex",
            Generator::Email => "hegel.email",
            Generator::Url => "hegel.url",
            Generator::Domain => "hegel.domain",
            Generator::Uuid => "hegel.uuid",
            Generator::Ipv4 => "hegel.ipv4",
            Generator::Ipv6 => "hegel.ipv6",
            Generator::Date => "hegel.date",
            Generator::Time => "hegel.time",
            Generator::Datetime => "hegel.datetime",
            Generator::Grammar(..) => "hegel.from_lark",
            Generator::Constant(_) => "hegel.constant",
            Generator::SampledFrom(_) => "hegel.sampled_from",
            Generator::OneOf(_) => "hegel.one_of",
            Generator::Optional(_) => "hegel.optional",
            Generator::List(..) => "hegel.lists",
            Generator::Dict(..) => "hegel.dicts",
            Generator::Tuple(_) => "hegel.tuples",
            Generator::Record(_) => "hegel.fixed_dicts",
            Generator::Recursive(..) | Generator::Subtree(_) => "hegel.recursive",
            Generator::Add(..) => "hegel.pool",
            Generator::Pool(_, false) => "hegel.pool.values_reusable",
            Generator::Pool(_, true) => "hegel.pool.values_consumed",
            Generator::Event(..) => "hegel.event",
        }
    }

    fn parts(&self) -> Vec<&Generator> {
        match self {
            Generator::OneOf(all) | Generator::Tuple(all) => all.iter().collect(),
            Generator::Optional(body)
            | Generator::List(body, ..)
            | Generator::Recursive(_, body, ..)
            | Generator::Add(_, body)
            | Generator::Event(_, body) => vec![body],
            Generator::Dict(key, value) => vec![key, value],
            Generator::Record(fields) => fields.iter().map(|(_, g)| g).collect(),
            _ => Vec::new(),
        }
    }
}

struct Open<'c> {
    name: &'c str,
    body: &'c Generator,
    recursion: *mut HegelRecursion,
    depth: u64,
    leaf: bool,
    branch: bool,
}

pub(crate) struct Drawer<'c> {
    case: &'c Case,
    open: Vec<Open<'c>>,
    outgrown: *mut HegelRecursion,
}

fn python(pattern: &str) -> String {
    let mut out = String::new();
    let mut class = false;
    let mut chars = pattern.chars();
    while let Some(c) = chars.next() {
        let set = match (c, class) {
            ('\\', _) => match chars.next() {
                Some('d') => Some("0-9"),
                Some('w') => Some("A-Za-z0-9_"),
                Some('s') => Some(" \\t\\n\\r\\f\\v"),
                Some(other) => {
                    out.extend(['\\', other]);
                    None
                }
                None => None,
            },
            ('$', false) => {
                out.push_str("\\Z");
                None
            }
            (c, _) => {
                class = match c {
                    '[' => true,
                    ']' => false,
                    _ => class,
                };
                out.push(c);
                None
            }
        };
        match (set, class) {
            (Some(set), true) => out.push_str(set),
            (Some(set), false) => out.extend(["[", set, "]"]),
            (None, _) => {}
        }
    }
    out
}

fn base64(data: &[u8]) -> String {
    let mut out = String::new();
    for chunk in data.chunks(3) {
        let word = chunk.iter().enumerate().fold(0u32, |word, (at, byte)| {
            word | u32::from(*byte) << (16 - 8 * at)
        });
        for at in 0..4 {
            out.push(match at <= chunk.len() {
                true => char::from(BASE64[(word >> (18 - 6 * at) & 63) as usize]),
                false => '=',
            });
        }
    }
    out
}

fn date(d: hegel_date_t) -> String {
    format!("{:04}-{:02}-{:02}", d.year, d.month, d.day)
}

fn time(t: hegel_time_t) -> String {
    format!("{:02}:{:02}:{:02}", t.hour, t.minute, t.second)
}

impl<'c> Drawer<'c> {
    pub(crate) fn new(case: &'c Case) -> Drawer<'c> {
        Drawer {
            case,
            open: Vec::new(),
            outgrown: ptr::null_mut(),
        }
    }

    fn label(&self, generator: &Generator) -> Result<u64, Halt> {
        let key = ptr::from_ref(generator);
        if let Some(known) = self.case.labels.borrow().get(&key) {
            return Ok(*known);
        }
        let ctx = self.case.ctx;
        let mut label = 0;
        hegel!(
            ctx,
            hegel_label_from_name(text(generator.kind().as_bytes()), &mut label)
        )?;
        let parts = generator.parts();
        if !parts.is_empty() {
            let labels = [label]
                .into_iter()
                .map(Ok)
                .chain(parts.into_iter().map(|part| self.label(part)))
                .collect::<Result<Vec<u64>, Halt>>()?;
            hegel!(
                ctx,
                hegel_label_combine(labels.as_ptr(), labels.len(), &mut label)
            )?;
        }
        self.case.labels.borrow_mut().insert(key, label);
        Ok(label)
    }

    fn closed(&self) -> Vec<&'c str> {
        self.open
            .iter()
            .filter(|open| open.leaf)
            .map(|open| open.name)
            .collect()
    }

    fn index(&self, count: usize) -> Result<usize, Halt> {
        let mut at = 0;
        hegel!(
            self.case.ctx,
            hegel_generate_integer(self.case.tc, 0, count as i64 - 1, &mut at)
        )?;
        Ok(at as usize)
    }

    fn string(&self, generator: *mut HegelStringGenerator) -> Result<Json, Halt> {
        let ctx = self.case.ctx;
        let mut out = NONE;
        let drawn = hegel!(
            ctx,
            hegel_generate_string(self.case.tc, generator, &mut out)
        );
        hegel!(ctx, hegel_string_generator_free(generator))?;
        drawn?;
        self.owned(out)
    }

    fn owned(&self, mut out: hegel_string_t) -> Result<Json, Halt> {
        let value = String::from_utf8_lossy(unsafe { bytes(out) }).into_owned();
        hegel!(self.case.ctx, hegel_owned_string_free(&mut out))?;
        Ok(Json::from(value))
    }

    fn text(&self, min: u64, max: Option<u64>) -> Result<Json, Halt> {
        let mut generator = ptr::null_mut();
        hegel!(
            self.case.ctx,
            hegel_string_generator_text(
                min,
                max.unwrap_or(HEGEL_MAX_SIZE_DEFAULT),
                ptr::null(),
                0,
                u32::MAX,
                ptr::null(),
                0,
                ptr::null(),
                0,
                NONE,
                NONE,
                &mut generator
            )
        )?;
        self.string(generator)
    }

    fn float(&self, min: Bound, max: Bound) -> Result<Json, Halt> {
        let end = |bound: Bound, open: f64| match bound {
            Bound::Open => (open, false),
            Bound::At(n) => (n, false),
            Bound::Past(n) => (n, true),
        };
        let ((low, above), (high, below)) = (end(min, f64::NEG_INFINITY), end(max, f64::INFINITY));
        let mut out = 0.0;
        hegel!(
            self.case.ctx,
            hegel_generate_float(
                self.case.tc,
                64,
                low,
                high,
                false,
                false,
                above,
                below,
                f64::from_bits(1),
                &mut out
            )
        )?;
        Ok(Json::from(out))
    }

    fn grammar(&self, source: &str, start: Option<&str>) -> Result<*mut HegelGrammar, Halt> {
        let key = (source.to_string(), start.map(String::from));
        if let Some(known) = self.case.grammars.borrow().get(&key) {
            return Ok(*known);
        }
        let ctx = self.case.ctx;
        let mut builder = ptr::null_mut();
        hegel!(
            ctx,
            hegel_grammar_builder_new(text(source.as_bytes()), &mut builder)
        )?;
        let built = (|| -> Result<*mut HegelGrammar, Halt> {
            for file in 0.. {
                let mut path = NONE;
                let listed =
                    unsafe { hegel_c::hegel_grammar_builder_file(ctx, builder, file, &mut path) };
                if listed == HEGEL_E_NOT_FOUND {
                    break;
                }
                hegel!(
                    ctx,
                    hegel_grammar_builder_provide(builder, file, ptr::null())
                )?;
            }
            let start = start.map(|rule| text(rule.as_bytes()));
            let mut grammar = ptr::null_mut();
            hegel!(
                ctx,
                hegel_grammar_new(
                    builder,
                    start.as_ref().map_or(ptr::null(), ptr::from_ref),
                    0,
                    0,
                    ptr::null(),
                    ptr::null(),
                    0,
                    &mut grammar
                )
            )?;
            Ok(grammar)
        })();
        hegel!(ctx, hegel_grammar_builder_free(builder))?;
        let grammar = built?;
        self.case.grammars.borrow_mut().insert(key, grammar);
        Ok(grammar)
    }

    fn many(
        &mut self,
        min: u64,
        max: Option<u64>,
        mut one: impl FnMut(&mut Self) -> Result<bool, Halt>,
    ) -> Result<(), Halt> {
        let (ctx, tc) = (self.case.ctx, self.case.tc);
        let mut all = ptr::null_mut();
        hegel!(
            ctx,
            hegel_new_collection(tc, min, max.unwrap_or(HEGEL_MAX_SIZE_DEFAULT), &mut all)
        )?;
        let drawn = (|| -> Result<(), Halt> {
            loop {
                let mut more = false;
                hegel!(ctx, hegel_collection_more(tc, all, &mut more))?;
                if !more {
                    return Ok(());
                }
                if !one(self)? {
                    hegel!(ctx, hegel_collection_reject(tc, all, ptr::null()))?;
                }
            }
        })();
        hegel!(ctx, hegel_collection_free(all))?;
        drawn
    }

    fn recursive(
        &mut self,
        name: &'c str,
        body: &'c Generator,
        depth: Option<u64>,
        leaves: Option<u64>,
    ) -> Result<Json, Halt> {
        let (ctx, tc) = (self.case.ctx, self.case.tc);
        let mut recursion = ptr::null_mut();
        hegel!(
            ctx,
            hegel_new_recursion(
                tc,
                depth.unwrap_or(DEPTH),
                leaves.unwrap_or(LEAVES),
                &mut recursion
            )
        )?;
        let open = self.case.open();
        let above = self.open.len();
        let drawn = loop {
            self.open.push(Open {
                name,
                body,
                recursion,
                depth: 0,
                leaf: false,
                branch: false,
            });
            let grown = self.subtree(above);
            self.open.truncate(above);
            match grown {
                Ok((value, true)) => break Ok(value),
                Ok((_, false)) => self.case.closed(open),
                Err(Halt::Retry) if self.outgrown == recursion => {
                    match hegel!(ctx, hegel_recursion_retry(tc, recursion)) {
                        Ok(()) => self.case.closed(open),
                        Err(halt) => break Err(halt),
                    }
                }
                Err(halt) => break Err(halt),
            }
        };
        hegel!(ctx, hegel_recursion_free(recursion))?;
        drawn
    }

    fn subtree(&mut self, at: usize) -> Result<(Json, bool), Halt> {
        let (ctx, tc) = (self.case.ctx, self.case.tc);
        let Open {
            body,
            recursion,
            depth,
            leaf,
            ..
        } = self.open[at];
        let mut branch = false;
        hegel!(
            ctx,
            hegel_recursion_branch(tc, recursion, depth, &mut branch)
        )?;
        if !branch {
            hegel!(ctx, hegel_recursion_leaf(tc, recursion)).inspect_err(|_| {
                self.outgrown = recursion;
            })?;
        }
        self.open[at].depth = depth + 1;
        self.open[at].leaf = !branch;
        self.open[at].branch = branch;
        let value = self.draw(body);
        self.open[at].depth = depth;
        self.open[at].leaf = leaf;
        self.open[at].branch = false;
        let value = value?;
        let done = match depth {
            0 => match hegel!(ctx, hegel_recursion_finish(tc, recursion)) {
                Err(Halt::Retry) => false,
                finished => finished.map(|()| true)?,
            },
            _ => true,
        };
        Ok((value, done))
    }

    pub(crate) fn draw(&mut self, generator: &'c Generator) -> Result<Json, Halt> {
        match generator {
            Generator::Add(pool, body) => {
                let value = self.draw(body)?;
                self.case.add(pool, value.clone())?;
                Ok(value)
            }
            Generator::Event(label, body) => {
                let value = self.draw(body)?;
                self.case.event(&format!("{label} {value}"))?;
                Ok(value)
            }
            generator => {
                self.case.start(self.label(generator)?)?;
                let value = self.value(generator)?;
                self.case.stop(false)?;
                Ok(value)
            }
        }
    }

    fn value(&mut self, generator: &'c Generator) -> Result<Json, Halt> {
        let (ctx, tc) = (self.case.ctx, self.case.tc);
        match generator {
            Generator::Boolean => self.case.chance(0.5).map(Json::from),
            Generator::Integer(min, max) => {
                let mut out = 0;
                hegel!(ctx, hegel_generate_integer(tc, *min, *max, &mut out))?;
                Ok(Json::from(out))
            }
            Generator::Float(min, max) => self.float(*min, *max),
            Generator::Bytes(min, max) => {
                let mut out = hegel_generate_bytes_result_t {
                    data: ptr::null_mut(),
                    len: 0,
                };
                hegel!(
                    ctx,
                    hegel_generate_bytes(tc, *min, max.unwrap_or(HEGEL_MAX_SIZE_DEFAULT), &mut out)
                )?;
                let value = base64(unsafe { std::slice::from_raw_parts(out.data, out.len) });
                hegel!(ctx, hegel_generate_bytes_result_free(&mut out))?;
                Ok(Json::from(value))
            }
            Generator::Text(min, max) => self.text(*min, *max),
            Generator::Regex(pattern) => {
                let mut generator = ptr::null_mut();
                hegel!(
                    ctx,
                    hegel_string_generator_regex(
                        text(python(pattern).as_bytes()),
                        false,
                        ptr::null(),
                        &mut generator
                    )
                )?;
                self.string(generator)
            }
            Generator::Email => {
                let mut generator = ptr::null_mut();
                hegel!(ctx, hegel_string_generator_email(&mut generator))?;
                self.string(generator)
            }
            Generator::Url => {
                let mut generator = ptr::null_mut();
                hegel!(ctx, hegel_string_generator_url(&mut generator))?;
                self.string(generator)
            }
            Generator::Domain => {
                let mut generator = ptr::null_mut();
                hegel!(ctx, hegel_string_generator_domain(DOMAIN, &mut generator))?;
                self.string(generator)
            }
            Generator::Uuid => {
                let mut out = [0u8; 16];
                hegel!(ctx, hegel_generate_uuid(tc, 0, false, out.as_mut_ptr()))?;
                let hex: String = out.iter().map(|b| format!("{b:02x}")).collect();
                Ok(Json::from(format!(
                    "{}-{}-{}-{}-{}",
                    &hex[..8],
                    &hex[8..12],
                    &hex[12..16],
                    &hex[16..20],
                    &hex[20..]
                )))
            }
            Generator::Ipv4 => {
                let mut out = [0u8; 4];
                hegel!(ctx, hegel_generate_ipv4(tc, out.as_mut_ptr()))?;
                Ok(Json::from(Ipv4Addr::from(out).to_string()))
            }
            Generator::Ipv6 => {
                let mut out = [0u8; 16];
                hegel!(ctx, hegel_generate_ipv6(tc, out.as_mut_ptr()))?;
                Ok(Json::from(Ipv6Addr::from(out).to_string()))
            }
            Generator::Date => {
                let mut out = FIRST;
                hegel!(ctx, hegel_generate_date(tc, FIRST, LAST, &mut out))?;
                Ok(Json::from(date(out)))
            }
            Generator::Time => {
                let mut out = MIDNIGHT;
                hegel!(ctx, hegel_generate_time(tc, MIDNIGHT, NIGHT, &mut out))?;
                Ok(Json::from(time(out)))
            }
            Generator::Datetime => {
                let (first, last) = (
                    hegel_datetime_t {
                        date: FIRST,
                        time: MIDNIGHT,
                    },
                    hegel_datetime_t {
                        date: LAST,
                        time: NIGHT,
                    },
                );
                let mut out = first;
                hegel!(ctx, hegel_generate_datetime(tc, first, last, &mut out))?;
                Ok(Json::from(format!(
                    "{}T{}Z",
                    date(out.date),
                    time(out.time)
                )))
            }
            Generator::Grammar(source, start) => {
                let grammar = self.grammar(source, start.as_deref())?;
                let mut out = NONE;
                hegel!(ctx, hegel_generate_grammar(tc, grammar, 0, &mut out))?;
                self.owned(out)
            }
            Generator::Constant(value) => Ok(value.clone()),
            Generator::SampledFrom(values) => Ok(values[self.index(values.len())?].clone()),
            Generator::OneOf(all) => {
                let closed = self.closed();
                let ending: Vec<&Generator> = all.iter().filter(|g| g.ends(&closed)).collect();
                let looping: Vec<&Generator> =
                    self.open.iter_mut().rev().find(|open| open.branch).map_or(
                        Vec::new(),
                        |open| {
                            open.branch = false;
                            ending
                                .iter()
                                .copied()
                                .filter(|g| g.loops(open.name))
                                .collect()
                        },
                    );
                let open = match looping.is_empty() {
                    true => ending,
                    false => looping,
                };
                let at = self.index(open.len())?;
                self.draw(open[at])
            }
            Generator::Optional(body) => match self.case.chance(0.5)? {
                true => self.draw(body),
                false => Ok(Json::Null),
            },
            Generator::List(item, min, max, unique) => {
                let max = match item.ends(&self.closed()) {
                    true => *max,
                    false => Some(*min),
                };
                let mut values: Vec<Json> = Vec::new();
                self.many(*min, max, |d| {
                    let value = d.draw(item)?;
                    let fresh = !*unique || !values.contains(&value);
                    if fresh {
                        values.push(value);
                    }
                    Ok(fresh)
                })?;
                Ok(Json::Array(values))
            }
            Generator::Dict(key, value) => {
                let max = match value.ends(&self.closed()) {
                    true => None,
                    false => Some(0),
                };
                let mut fields = Map::new();
                self.many(0, max, |d| {
                    let name = match d.draw(key)? {
                        Json::String(name) => name,
                        other => other.to_string(),
                    };
                    let fresh = !fields.contains_key(&name);
                    if fresh {
                        fields.insert(name, d.draw(value)?);
                    }
                    Ok(fresh)
                })?;
                Ok(Json::Object(fields))
            }
            Generator::Tuple(all) => all.iter().map(|g| self.draw(g)).collect(),
            Generator::Record(fields) => {
                let mut drawn = Map::new();
                for (name, field) in fields {
                    match field {
                        Generator::Optional(body) => {
                            if body.ends(&self.closed()) && self.case.chance(0.5)? {
                                drawn.insert(name.clone(), self.draw(body)?);
                            }
                        }
                        field => {
                            drawn.insert(name.clone(), self.draw(field)?);
                        }
                    }
                }
                Ok(Json::Object(drawn))
            }
            Generator::Recursive(name, body, depth, leaves) => {
                self.recursive(name, body, *depth, *leaves)
            }
            Generator::Subtree(name) => {
                let at = self
                    .open
                    .iter()
                    .rposition(|open| open.name == name)
                    .ok_or_else(|| Halt::Fatal(format!("the schema `{name}` is not open")))?;
                self.subtree(at).map(|(value, _)| value)
            }
            Generator::Pool(name, consume) => self.case.take(name, *consume),
            Generator::Add(..) | Generator::Event(..) => self.draw(generator),
        }
    }
}
