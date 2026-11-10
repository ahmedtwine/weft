use std::cell::{Cell, RefCell};
use std::panic::{self, AssertUnwindSafe};

use hegel::generators as gs;
use hegel::stateful::{self, Invariant, Rule, StateMachine};
use hegel::{Database, HealthCheck, Settings, TestCase, Verbosity};
use serde_json::Value as Json;

use crate::{Case, Choice, Source};

const STOP: &str = "weft";
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
        self.tc.draw(gs::booleans())
    }

    fn int(&self, min: i64, max: i64) -> i64 {
        self.tc
            .draw(gs::integers::<i64>().min_value(min).max_value(max))
    }

    fn text(&self, min: usize, max: Option<usize>) -> String {
        match max {
            Some(max) => self.tc.draw(gs::text().min_size(min).max_size(max)),
            None => self.tc.draw(gs::text().min_size(min)),
        }
    }

    fn index(&self, count: usize) -> usize {
        self.tc
            .draw(gs::integers::<usize>().min_value(0).max_value(count - 1))
    }

    fn target(&self, score: f64) {
        self.best
            .set(Some(self.best.get().map_or(score, |b| b.max(score))));
    }
}

struct Machine<'m, 'c> {
    case: &'m RefCell<&'c mut dyn Case>,
    best: &'m Cell<Option<f64>>,
}

impl Machine<'_, '_> {
    fn step(&mut self, tc: TestCase) {
        let source = Drawn {
            tc: &tc,
            best: self.best,
        };
        if self.case.borrow_mut().step(&source).is_err() {
            panic!("{STOP}")
        }
    }
}

impl StateMachine for Machine<'_, '_> {
    fn rules(&self) -> Vec<Rule<Self>> {
        vec![Rule::new("step", 1.0, Machine::step)]
    }

    fn invariants(&self) -> Vec<Invariant<Self>> {
        Vec::new()
    }
}

fn said(p: &(dyn std::any::Any + Send)) -> String {
    p.downcast_ref::<&str>()
        .map(|s| s.to_string())
        .or_else(|| p.downcast_ref::<String>().cloned())
        .unwrap_or_default()
}

pub fn run(cases: u64, case: &mut dyn Case) -> Result<(), String> {
    let case = RefCell::new(case);
    let ran = panic::catch_unwind(AssertUnwindSafe(|| {
        hegel::Hegel::new(|tc: TestCase| {
            let best = Cell::new(None);
            let started = case.borrow_mut().start(&Drawn {
                tc: &tc,
                best: &best,
            });
            if started.is_err() {
                panic!("{STOP}")
            }
            let kept = tc.clone();
            stateful::machine(Machine {
                case: &case,
                best: &best,
            })
            .steps(STEPS)
            .run(tc);
            if let Some(score) = best.get() {
                kept.target(score);
            }
        })
        .settings(
            Settings::new()
                .test_cases(cases)
                .database(Database::Disabled)
                .suppress_health_check([HealthCheck::TooSlow])
                .verbosity(Verbosity::Quiet)
                .print_blob(false),
        )
        .run()
    }));
    match ran {
        Ok(()) => Ok(()),
        Err(p) if said(&*p) == STOP => Ok(()),
        Err(p) => Err(said(&*p)),
    }
}
