use std::cell::{Cell, RefCell};
use std::panic::{self, AssertUnwindSafe};

use hegel::generators as gs;
use hegel::stateful::{self, Invariant, Rule, StateMachine};
use hegel::{Database, HealthCheck, Settings, TestCase, Verbosity};
use serde_json::Value as Json;

use crate::{Case, Choice, Engine, Source};

const STOP: &str = "weft";
const NARROW: usize = 256;

pub struct Hegel;

struct Drawn<'b> {
    tc: &'b TestCase,
    best: &'b Cell<Option<f64>>,
}

impl Drawn<'_> {
    fn below(&self, count: usize) -> usize {
        self.tc
            .draw(gs::integers::<usize>().min_value(0).max_value(count - 1))
    }
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
        match count <= NARROW {
            true => self.below(count),
            false => {
                let at = self.below(count.div_ceil(NARROW)) * NARROW + self.below(NARROW);
                self.tc.assume(at < count);
                at
            }
        }
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

impl Engine for Hegel {
    fn run(&mut self, cases: u64, steps: u32, case: &mut dyn Case) -> Result<(), String> {
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
                .steps(i64::from(steps))
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
}

pub struct Random {
    state: Cell<u64>,
}

impl Random {
    pub fn new(seed: u64) -> Random {
        Random {
            state: Cell::new(seed),
        }
    }

    fn next(&self) -> u64 {
        let s = self.state.get().wrapping_add(0x9e37_79b9_7f4a_7c15);
        self.state.set(s);
        let z = (s ^ (s >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        let z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    fn below(&self, count: u128) -> u128 {
        u128::from(self.next()) % count
    }
}

impl Source for Random {
    fn draw(&self, choice: &Choice) -> Json {
        choice.draw(self)
    }

    fn bool(&self) -> bool {
        self.next() & 1 == 1
    }

    fn int(&self, min: i64, max: i64) -> i64 {
        let span = (i128::from(max) - i128::from(min) + 1) as u128;
        (i128::from(min) + self.below(span) as i128) as i64
    }

    fn text(&self, min: usize, max: Option<usize>) -> String {
        let top = max.unwrap_or(min + 8);
        let size = min + self.index(top - min + 1);
        (0..size)
            .map(|_| char::from(b'a' + self.below(26) as u8))
            .collect()
    }

    fn index(&self, count: usize) -> usize {
        self.below(count as u128) as usize
    }

    fn target(&self, _: f64) {}
}

impl Engine for Random {
    fn run(&mut self, cases: u64, steps: u32, case: &mut dyn Case) -> Result<(), String> {
        for _ in 0..cases {
            if case.start(self).is_err() {
                return Ok(());
            }
            for _ in 0..=self.index(steps as usize) {
                if case.step(self).is_err() {
                    return Ok(());
                }
            }
        }
        Ok(())
    }
}
