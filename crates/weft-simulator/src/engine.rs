use std::cell::Cell;
use std::env;
use std::panic::{self, AssertUnwindSafe};
use std::path::PathBuf;

use hegel::generators as gs;
use hegel::{Database, HealthCheck, NondeterminismStrictness, Settings, TestCase, Verbosity};
use serde_json::Value as Json;

use crate::{Case, Choice, Source, Stop};

const STOP: &str = "weft";
const STORE: &str = "weft";
const STEPS: i64 = 20;

struct Drawn<'b> {
    tc: &'b TestCase,
    best: &'b Cell<Option<f64>>,
}

impl Source for Drawn<'_> {
    fn draw(&self, choice: &Choice) -> Json {
        choice.draw(self)
    }

    fn bool(&self) -> bool {
        self.tc.draw_silent(gs::booleans())
    }

    fn int(&self, min: i64, max: i64) -> i64 {
        self.tc
            .draw_silent(gs::integers::<i64>().min_value(min).max_value(max))
    }

    fn text(&self, min: usize, max: Option<usize>) -> String {
        match max {
            Some(max) => self.tc.draw_silent(gs::text().min_size(min).max_size(max)),
            None => self.tc.draw_silent(gs::text().min_size(min)),
        }
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

fn steps(source: &Drawn) -> i64 {
    let full = source.bool();
    let some = source.int(0, STEPS);
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
                .and_then(|()| (0..steps(&source)).try_for_each(|_| case.step(&source)));
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
