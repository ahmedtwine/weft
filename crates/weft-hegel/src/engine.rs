use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::env;
use std::ffi::{c_char, c_void};
use std::path::PathBuf;
use std::ptr;
use std::rc::Rc;
use std::slice;

use hegel_c::hegel_result_t::{HEGEL_E_ASSUME, HEGEL_E_RETRY, HEGEL_E_STOP_TEST, HEGEL_OK};
use hegel_c::{
    HegelContext, HegelGrammar, HegelPool, HegelRun, HegelStateMachine, HegelTestCase,
    hegel_result_t, hegel_string_t,
};
use serde_json::Value as Json;

use crate::generator::{Drawer, Generator};

const STORE: &str = "weft";
const STEP: &str = "hegel.stateful.rule";
const QUIET: u32 = 1;
const FILTER_TOO_MUCH: u32 = 1 << 0;
const TOO_SLOW: u32 = 1 << 1;
const FAILED: u32 = 1;
const ERROR: u32 = 2;
pub const VALID: u32 = 0;
pub const INVALID: u32 = 1;
pub const OVERRUN: u32 = 2;
pub const INTERESTING: u32 = 3;

pub enum Halt {
    Overrun,
    Invalid,
    Empty,
    Retry,
    Fatal(String),
}

impl From<String> for Halt {
    fn from(e: String) -> Halt {
        Halt::Fatal(e)
    }
}

pub(crate) type Grammars = Rc<RefCell<HashMap<(String, Option<String>), *mut HegelGrammar>>>;

pub(crate) fn text(bytes: &[u8]) -> hegel_string_t {
    hegel_string_t {
        data: bytes.as_ptr().cast::<c_char>(),
        len: bytes.len(),
    }
}

pub(crate) unsafe fn bytes<'a>(s: hegel_string_t) -> &'a [u8] {
    match s.data.is_null() {
        true => &[],
        false => unsafe { slice::from_raw_parts(s.data.cast::<u8>(), s.len) },
    }
}

fn said(ctx: *mut HegelContext) -> String {
    String::from_utf8_lossy(unsafe { bytes(hegel_c::hegel_context_last_error(ctx)) }).into_owned()
}

pub(crate) fn checked(ctx: *mut HegelContext, code: hegel_result_t) -> Result<(), Halt> {
    match code {
        HEGEL_OK => Ok(()),
        HEGEL_E_STOP_TEST => Err(Halt::Overrun),
        HEGEL_E_ASSUME => Err(Halt::Invalid),
        HEGEL_E_RETRY => Err(Halt::Retry),
        code => Err(Halt::Fatal(format!(
            "Hegel gives {}: {}",
            code as i32,
            said(ctx)
        ))),
    }
}

fn fatal(halt: Halt, at: &str) -> String {
    match halt {
        Halt::Fatal(e) => e,
        _ => format!("Hegel stops {at}"),
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

unsafe extern "C" fn written(output: *mut c_void, line: *const c_char, len: usize) {
    let output = unsafe { &*output.cast::<RefCell<Vec<String>>>() };
    let line = unsafe { slice::from_raw_parts(line.cast::<u8>(), len) };
    output
        .borrow_mut()
        .push(String::from_utf8_lossy(line).into_owned());
}

pub struct Engine {
    ctx: *mut HegelContext,
    run: *mut HegelRun,
    step: u64,
    output: Box<RefCell<Vec<String>>>,
    grammars: Grammars,
}

struct Pool {
    name: String,
    raw: *mut HegelPool,
    values: Vec<(i64, Json)>,
}

pub struct Case {
    pub(crate) ctx: *mut HegelContext,
    pub(crate) tc: *mut HegelTestCase,
    pub(crate) grammars: Grammars,
    pub(crate) labels: RefCell<HashMap<*const Generator, u64>>,
    machine: *mut HegelStateMachine,
    step: u64,
    spans: Cell<usize>,
    pools: RefCell<Vec<Pool>>,
    taken: RefCell<Vec<(usize, i64, Json)>>,
}

pub struct Ended {
    pub failed: bool,
    pub caveat: Option<String>,
    pub output: Vec<String>,
}

impl Engine {
    pub fn start(cases: u64, key: Option<&str>) -> Result<Engine, String> {
        let ctx = hegel_c::hegel_context_new();
        let output = Box::new(RefCell::new(Vec::new()));
        let sink = ptr::from_ref::<RefCell<Vec<String>>>(&output)
            .cast_mut()
            .cast::<c_void>();
        let made = || -> Result<(*mut HegelRun, u64), Halt> {
            let mut settings = ptr::null_mut();
            hegel!(ctx, hegel_settings_new(&mut settings))?;
            let store = key.map_or(String::new(), |_| store());
            let database = text(store.as_bytes());
            hegel!(ctx, hegel_settings_set_test_cases(settings, cases))?;
            hegel!(ctx, hegel_settings_set_verbosity(settings, QUIET))?;
            hegel!(ctx, hegel_settings_set_print_blob(settings, false))?;
            hegel!(ctx, hegel_settings_set_show_statistics(settings, true))?;
            hegel!(
                ctx,
                hegel_settings_set_suppress_health_check(settings, FILTER_TOO_MUCH | TOO_SLOW)
            )?;
            hegel!(ctx, hegel_settings_set_database(settings, &database))?;
            if let Some(key) = key {
                hegel!(
                    ctx,
                    hegel_settings_set_database_key(settings, &text(key.as_bytes()))
                )?;
            }
            let mut run = ptr::null_mut();
            let started = hegel!(
                ctx,
                hegel_run_start(settings, Some(written), sink, &mut run)
            );
            hegel!(ctx, hegel_settings_free(settings))?;
            started?;
            let mut step = 0;
            hegel!(ctx, hegel_label_from_name(text(STEP.as_bytes()), &mut step))?;
            Ok((run, step))
        };
        let (run, step) = made().map_err(|halt| fatal(halt, "before the run starts"))?;
        Ok(Engine {
            ctx,
            run,
            step,
            output,
            grammars: Grammars::default(),
        })
    }

    pub fn case(&mut self) -> Result<Option<Case>, String> {
        let mut tc = ptr::null_mut();
        hegel!(self.ctx, hegel_next_test_case(self.run, &mut tc))
            .map_err(|halt| fatal(halt, "between two cases"))?;
        Ok((!tc.is_null()).then(|| Case {
            ctx: self.ctx,
            tc,
            grammars: self.grammars.clone(),
            labels: RefCell::default(),
            machine: ptr::null_mut(),
            step: self.step,
            spans: Cell::new(0),
            pools: RefCell::default(),
            taken: RefCell::default(),
        }))
    }

    pub fn end(&mut self) -> Result<Ended, String> {
        let ctx = self.ctx;
        let read = || -> Result<(bool, Option<String>), Halt> {
            let mut result = ptr::null_mut();
            hegel!(ctx, hegel_run_result(self.run, &mut result))?;
            let mut status = hegel_c::hegel_run_status_t::HEGEL_RUN_STATUS_PASSED;
            hegel!(ctx, hegel_run_result_status(result, &mut status))?;
            let mut error = ptr::null();
            hegel!(ctx, hegel_run_result_error(result, &mut error))?;
            if status as u32 == ERROR {
                return Err(Halt::Fatal(
                    String::from_utf8_lossy(unsafe {
                        error.as_ref().map_or(&[][..], |e| bytes(*e))
                    })
                    .into_owned(),
                ));
            }
            let mut caveat = None;
            if status as u32 == FAILED {
                let mut failure = ptr::null_mut();
                hegel!(ctx, hegel_run_result_failure(result, 0, &mut failure))?;
                let mut given = ptr::null();
                hegel!(ctx, hegel_failure_caveat(failure, &mut given))?;
                caveat = unsafe { given.as_ref() }
                    .map(|t| String::from_utf8_lossy(unsafe { bytes(*t) }).into_owned());
                hegel!(ctx, hegel_failure_free(failure))?;
            }
            hegel!(ctx, hegel_run_result_free(result))?;
            Ok((status as u32 == FAILED, caveat))
        };
        let (failed, caveat) = read().map_err(|halt| fatal(halt, "after the run"))?;
        Ok(Ended {
            failed,
            caveat,
            output: self.output.take(),
        })
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        unsafe {
            for grammar in self.grammars.borrow().values() {
                let _ = hegel_c::hegel_grammar_free(self.ctx, *grammar);
            }
            let _ = hegel_c::hegel_run_free(self.ctx, self.run);
            let _ = hegel_c::hegel_context_free(self.ctx);
        }
    }
}

impl Case {
    pub(crate) fn start(&self, label: u64) -> Result<(), Halt> {
        hegel!(self.ctx, hegel_start_span(self.tc, label))?;
        self.spans.set(self.spans.get() + 1);
        Ok(())
    }

    pub(crate) fn stop(&self, discard: bool) -> Result<(), Halt> {
        hegel!(self.ctx, hegel_stop_span(self.tc, discard))?;
        self.spans.set(self.spans.get() - 1);
        Ok(())
    }

    pub(crate) fn open(&self) -> usize {
        self.spans.get()
    }

    pub(crate) fn closed(&self, open: usize) {
        self.spans.set(open);
    }

    pub fn chance(&self, share: f64) -> Result<bool, Halt> {
        let mut heads = false;
        hegel!(
            self.ctx,
            hegel_generate_boolean(self.tc, share, false, false, &mut heads)
        )?;
        Ok(heads)
    }

    pub fn event(&self, label: &str) -> Result<(), Halt> {
        hegel!(self.ctx, hegel_event(self.tc, text(label.as_bytes())))
    }

    pub fn draw(&self, generator: &Generator) -> Result<Json, Halt> {
        let open = self.open();
        self.taken.borrow_mut().clear();
        let drawn = Drawer::new(self).draw(generator);
        if matches!(drawn, Err(Halt::Empty)) {
            while self.open() > open {
                self.stop(true)?;
            }
            self.undo()?;
        }
        drawn
    }

    fn pooled(&self, name: &str) -> Result<usize, Halt> {
        self.pools
            .borrow()
            .iter()
            .position(|pool| pool.name == name)
            .ok_or_else(|| Halt::Fatal(format!("the case has no pool `{name}`")))
    }

    pub fn pool(&self, name: &str) -> Result<(), Halt> {
        let mut raw = ptr::null_mut();
        hegel!(self.ctx, hegel_new_pool(self.tc, &mut raw))?;
        self.pools.borrow_mut().push(Pool {
            name: name.to_string(),
            raw,
            values: Vec::new(),
        });
        Ok(())
    }

    pub fn add(&self, name: &str, value: Json) -> Result<(), Halt> {
        let at = self.pooled(name)?;
        let raw = self.pools.borrow()[at].raw;
        let mut id = 0;
        hegel!(self.ctx, hegel_pool_add(self.tc, raw, &mut id))?;
        self.pools.borrow_mut()[at].values.push((id, value));
        Ok(())
    }

    pub(crate) fn take(&self, name: &str, consume: bool) -> Result<Json, Halt> {
        let at = self.pooled(name)?;
        let raw = match &self.pools.borrow()[at] {
            pool if pool.values.is_empty() => return Err(Halt::Empty),
            pool => pool.raw,
        };
        let mut id = 0;
        hegel!(
            self.ctx,
            hegel_pool_generate(self.tc, raw, consume, &mut id)
        )?;
        let values = &mut self.pools.borrow_mut()[at].values;
        let found = values
            .iter()
            .position(|(known, _)| *known == id)
            .ok_or_else(|| {
                Halt::Fatal(format!(
                    "Hegel gives the value {id}, and the pool `{name}` has no value {id}"
                ))
            })?;
        match consume {
            true => {
                let (id, value) = values.remove(found);
                self.taken.borrow_mut().push((at, id, value.clone()));
                Ok(value)
            }
            false => Ok(values[found].1.clone()),
        }
    }

    pub fn undo(&self) -> Result<(), Halt> {
        let taken: Vec<(usize, i64, Json)> = self.taken.borrow_mut().drain(..).rev().collect();
        for (at, id, value) in taken {
            let raw = self.pools.borrow()[at].raw;
            hegel!(self.ctx, hegel_pool_restore(self.tc, raw, id))?;
            self.pools.borrow_mut()[at].values.push((id, value));
        }
        Ok(())
    }

    pub fn size(&self, name: &str) -> usize {
        self.pools
            .borrow()
            .iter()
            .find(|pool| pool.name == name)
            .map_or(0, |pool| pool.values.len())
    }

    pub fn machine(
        &mut self,
        rules: &[String],
        invariants: &[String],
        steps: i64,
    ) -> Result<(), Halt> {
        let (ctx, tc) = (self.ctx, self.tc);
        hegel!(
            ctx,
            hegel_new_state_machine(tc, 1, 1, steps, &mut self.machine)
        )?;
        for rule in rules {
            hegel!(
                ctx,
                hegel_state_machine_add_rule(
                    tc,
                    self.machine,
                    text(rule.as_bytes()),
                    1.0,
                    [0].as_ptr(),
                    1
                )
            )?;
        }
        for invariant in invariants {
            hegel!(
                ctx,
                hegel_state_machine_add_invariant(
                    tc,
                    self.machine,
                    text(invariant.as_bytes()),
                    true
                )
            )?;
        }
        let mut concurrency = 0;
        hegel!(
            ctx,
            hegel_state_machine_start(tc, self.machine, &mut concurrency)
        )
    }

    pub fn invariant(&self, index: usize) -> Result<bool, Halt> {
        let mut check = false;
        hegel!(
            self.ctx,
            hegel_state_machine_should_check_invariant(
                self.tc,
                self.machine,
                index as i64,
                &mut check
            )
        )?;
        Ok(check)
    }

    pub fn round(&mut self) -> Result<bool, Halt> {
        let (mut group, mut concurrency) = (0, 0);
        self.start(self.step)?;
        hegel!(
            self.ctx,
            hegel_state_machine_next_group(self.tc, self.machine, &mut group, &mut concurrency)
        )?;
        Ok(group != hegel_c::HEGEL_STATE_MACHINE_DONE)
    }

    pub fn rule(&mut self) -> Result<Option<usize>, Halt> {
        let mut rule = 0;
        hegel!(
            self.ctx,
            hegel_state_machine_next_rule(self.tc, self.machine, 0, &mut rule)
        )?;
        Ok(usize::try_from(rule).ok())
    }

    pub fn rejected(&mut self) -> Result<(), Halt> {
        hegel!(
            self.ctx,
            hegel_state_machine_rule_rejected(self.tc, self.machine, 0)
        )
    }

    pub fn close(&mut self, discard: bool) -> Result<(), Halt> {
        self.stop(discard)
    }

    pub fn complete(self, status: u32, origin: Option<&str>) -> Result<(), String> {
        let origin = origin.map(|o| text(o.as_bytes()));
        let marked = hegel!(
            self.ctx,
            hegel_mark_complete(
                self.tc,
                status,
                origin.as_ref().map_or(ptr::null(), |o| o as *const _)
            )
        );
        unsafe {
            for pool in self.pools.borrow().iter() {
                let _ = hegel_c::hegel_pool_free(self.ctx, pool.raw);
            }
            let _ = hegel_c::hegel_state_machine_free(self.ctx, self.machine);
            let _ = hegel_c::hegel_test_case_free(self.ctx, self.tc);
        }
        marked.map_err(|halt| fatal(halt, "at the end of a case"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_manifest_pins_the_version_that_hegel_gives() {
        let ctx = hegel_c::hegel_context_new();
        let mut version = text(&[]);
        assert!(hegel!(ctx, hegel_version(&mut version)).is_ok());
        let version = String::from_utf8_lossy(unsafe { bytes(version) }).into_owned();
        assert!(
            include_str!("../Cargo.toml").contains(&format!("hegeltest-c = \"={version}\"")),
            "Cargo.toml does not pin hegeltest-c {version}"
        );
    }
}
