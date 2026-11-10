use std::cell::Cell;
use std::env;
use std::panic::{self, AssertUnwindSafe};
use std::path::PathBuf;
use std::sync::Arc;

use hegel::extras::serde_json::values;
use hegel::generators::{self as gs, Generator};
use hegel::{Database, HealthCheck, NondeterminismStrictness, Settings, TestCase, Verbosity};
use serde_json::{Map, Value as Json};

use crate::{Bound, Case, Choice, Format, Source, Stop};

const STOP: &str = "weft";
const STORE: &str = "weft";
const STEPS: i64 = 20;
const DEPTH: usize = 3;

struct Drawer<'c> {
    choice: &'c Choice,
    open: Arc<Vec<(&'c str, &'c Choice)>>,
    depth: usize,
}

impl<'c> Drawer<'c> {
    fn of(&self, choice: &'c Choice) -> Drawer<'c> {
        Drawer {
            choice,
            open: Arc::clone(&self.open),
            depth: self.depth,
        }
    }

    fn last(&self) -> bool {
        self.depth >= DEPTH
    }

    fn most(&self, item: &Choice, min: usize, max: Option<usize>) -> Option<usize> {
        match (self.last() && !item.ends(), loops(item)) {
            (true, _) => Some(min),
            (false, true) => {
                let wide = min + DEPTH - self.depth;
                Some(max.map_or(wide, |max| max.min(wide)))
            }
            (false, false) => max,
        }
    }

    fn sized<G: Generator<Json>>(
        &self,
        all: gs::VecGenerator<G, Json>,
        item: &Choice,
        min: usize,
        max: Option<usize>,
    ) -> gs::VecGenerator<G, Json> {
        match self.most(item, min, max) {
            Some(max) => all.min_size(min).max_size(max),
            None => all.min_size(min),
        }
    }

    fn number(&self, tc: &TestCase, min: Bound, max: Bound) -> f64 {
        let any = gs::floats::<f64>().allow_nan(false).allow_infinity(false);
        let low = match min {
            Bound::Open => any,
            Bound::At(n) => any.min_value(n),
            Bound::Past(n) => any.min_value_exclusive(n),
        };
        match max {
            Bound::Open => low,
            Bound::At(n) => low.max_value(n),
            Bound::Past(n) => low.max_value_exclusive(n),
        }
        .do_draw(tc)
    }

    fn format(&self, tc: &TestCase, format: Format) -> String {
        match format {
            Format::Email => gs::emails().do_draw(tc),
            Format::Uri => gs::urls().do_draw(tc),
            Format::Uuid => gs::uuids().do_draw(tc),
            Format::Date => gs::date_strings().do_draw(tc),
            Format::Time => gs::time_strings().do_draw(tc),
            Format::DateTime => gs::datetime_strings().do_draw(tc),
            Format::Ipv4 => gs::ip_addresses().v4().do_draw(tc).to_string(),
            Format::Ipv6 => gs::ip_addresses().v6().do_draw(tc).to_string(),
            Format::Hostname => gs::domains().do_draw(tc),
        }
    }
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

fn loops(choice: &Choice) -> bool {
    match choice {
        Choice::Var(_) => true,
        Choice::Rec(_, _, shown) => loops(shown),
        Choice::AnyOf(all) | Choice::Tuple(all) => all.iter().any(loops),
        Choice::Array(item, ..) => loops(item),
        Choice::Record(key, value) => loops(key) || loops(value),
        Choice::Object(fields) => fields.iter().any(|(_, c, _)| loops(c)),
        _ => false,
    }
}

fn kind(choice: &Choice) -> &'static str {
    match choice {
        Choice::Any => "weft.any",
        Choice::Bool => "weft.bool",
        Choice::Int(..) => "weft.int",
        Choice::Text(..) => "weft.text",
        Choice::Pattern(_) => "weft.pattern",
        Choice::Format(_) => "weft.format",
        Choice::Number(..) => "weft.number",
        Choice::Enum(_) => "weft.enum",
        Choice::AnyOf(_) => "weft.any_of",
        Choice::Array(..) => "weft.array",
        Choice::Tuple(_) => "weft.tuple",
        Choice::Record(..) => "weft.record",
        Choice::Object(_) => "weft.object",
        Choice::Rec(..) => "weft.rec",
        Choice::Var(_) => "weft.var",
    }
}

impl Generator<Json> for Drawer<'_> {
    fn label(&self) -> u64 {
        gs::label_from_name(kind(self.choice))
    }

    fn do_draw(&self, tc: &TestCase) -> Json {
        match self.choice {
            Choice::Any => values().do_draw(tc),
            Choice::Bool => Json::from(gs::booleans().do_draw(tc)),
            Choice::Int(min, max) => Json::from(
                gs::integers::<i64>()
                    .min_value(*min)
                    .max_value(*max)
                    .do_draw(tc),
            ),
            Choice::Text(min, max) => Json::from(match max {
                Some(max) => gs::text().min_size(*min).max_size(*max).do_draw(tc),
                None => gs::text().min_size(*min).do_draw(tc),
            }),
            Choice::Pattern(pattern) => Json::from(
                gs::from_regex(&python(pattern))
                    .fullmatch(false)
                    .do_draw(tc),
            ),
            Choice::Format(format) => Json::from(self.format(tc, *format)),
            Choice::Number(min, max) => Json::from(self.number(tc, *min, *max)),
            Choice::Enum(values) => gs::sampled_from(values.as_slice()).do_draw(tc),
            Choice::AnyOf(all) => gs::one_of(
                all.iter()
                    .filter(|c| !self.last() || c.ends())
                    .map(|c| self.of(c)),
            )
            .do_draw(tc),
            Choice::Array(item, min, max, unique) => Json::Array(
                self.sized(gs::vecs(self.of(item)), item, *min, *max)
                    .unique(*unique)
                    .do_draw(tc),
            ),
            Choice::Tuple(all) => {
                Json::Array(all.iter().map(|c| tc.draw_silent(self.of(c))).collect())
            }
            Choice::Record(key, value) => {
                let keys = self.of(key).map(|k| match k {
                    Json::String(k) => k,
                    k => k.to_string(),
                });
                let all = gs::btree_maps(keys, self.of(value));
                Json::Object(
                    match self.most(value, 0, None) {
                        Some(max) => all.max_size(max),
                        None => all,
                    }
                    .do_draw(tc)
                    .into_iter()
                    .collect::<Map<_, _>>(),
                )
            }
            Choice::Object(fields) => Json::Object(
                fields
                    .iter()
                    .filter_map(|(k, c, required)| {
                        match (*required, self.last() && !c.ends()) {
                            (true, _) => Some(tc.draw_silent(self.of(c))),
                            (false, true) => None,
                            (false, false) => tc.draw_silent(gs::optional(self.of(c))),
                        }
                        .map(|v| (k.clone(), v))
                    })
                    .collect::<Map<_, _>>(),
            ),
            Choice::Rec(key, body, shown) => Drawer {
                choice: shown,
                open: Arc::new([self.open.as_slice(), &[(key.as_str(), &**body)]].concat()),
                depth: self.depth,
            }
            .do_draw(tc),
            Choice::Var(key) => {
                let (_, body) = self
                    .open
                    .iter()
                    .rev()
                    .find(|(k, _)| k == key)
                    .unwrap_or_else(|| panic!("the schema `{key}` is not open"));
                Drawer {
                    choice: body,
                    open: Arc::clone(&self.open),
                    depth: self.depth + 1,
                }
                .do_draw(tc)
            }
        }
    }
}

struct Drawn<'b> {
    tc: &'b TestCase,
    best: &'b Cell<Option<f64>>,
}

impl Source for Drawn<'_> {
    fn draw(&self, choice: &Choice) -> Json {
        self.tc.draw_silent(Drawer {
            choice,
            open: Arc::new(Vec::new()),
            depth: 0,
        })
    }

    fn index(&self, count: usize) -> usize {
        self.tc
            .draw_silent(gs::integers::<usize>().min_value(0).max_value(count - 1))
    }

    fn target(&self, score: f64) {
        self.best
            .set(Some(self.best.get().map_or(score, |b| b.max(score))));
    }
}

fn said(p: &(dyn std::any::Any + Send)) -> String {
    p.downcast_ref::<&str>()
        .map(|s| s.to_string())
        .or_else(|| p.downcast_ref::<String>().cloned())
        .unwrap_or_default()
}

fn settings(cases: u64, key: Option<&str>) -> Settings {
    let settings = Settings::new()
        .test_cases(cases)
        .suppress_health_check([HealthCheck::TooSlow])
        .nondeterminism_strictness(NondeterminismStrictness::Error)
        .verbosity(Verbosity::Quiet)
        .print_blob(false);
    match key {
        Some(key) => settings
            .database(Database::Path(store()))
            .database_key(key.to_string()),
        None => settings.database(Database::Disabled),
    }
}

fn store() -> String {
    env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| env::home_dir().map(|home| home.join(".cache")))
        .unwrap_or_else(env::temp_dir)
        .join(STORE)
        .display()
        .to_string()
}

fn steps(tc: &TestCase) -> i64 {
    let full = tc.draw_silent(gs::booleans());
    let some = tc.draw_silent(gs::integers::<i64>().min_value(0).max_value(STEPS));
    if full { STEPS } else { some }
}

pub fn run(cases: u64, key: Option<&str>, case: &mut dyn Case) -> Result<(), String> {
    let hegel = || {
        hegel::Hegel::new(|tc: TestCase| {
            let best = Cell::new(None);
            let source = Drawn {
                tc: &tc,
                best: &best,
            };
            let ended = case
                .start(&source)
                .and_then(|()| (0..steps(&tc)).try_for_each(|_| case.step(&source)));
            match (ended, best.get()) {
                (Err(Stop::Fail), _) => panic!("{STOP}"),
                (_, Some(score)) => tc.target(score),
                _ => {}
            }
        })
        .settings(settings(cases, key))
        .run()
    };
    match panic::catch_unwind(AssertUnwindSafe(hegel)) {
        Err(p) if said(&*p) != STOP => Err(said(&*p)),
        _ => Ok(()),
    }
}
