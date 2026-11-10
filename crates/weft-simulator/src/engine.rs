use std::env;
use std::ffi::c_char;
use std::path::PathBuf;
use std::ptr;

use hegel_c::hegel_result_t::{HEGEL_E_ASSUME, HEGEL_E_STOP_TEST, HEGEL_OK};
use hegel_c::{
    HegelContext, HegelRun, HegelRunResult, HegelStateMachine, HegelTestCase, hegel_result_t,
    hegel_string_t,
};

const STORE: &str = "weft";
const STEP: &str = "weft.step";
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
    Fatal(String),
}

impl From<String> for Halt {
    fn from(e: String) -> Halt {
        Halt::Fatal(e)
    }
}

pub fn text(bytes: &[u8]) -> hegel_string_t {
    hegel_string_t {
        data: bytes.as_ptr().cast::<c_char>(),
        len: bytes.len(),
    }
}

pub unsafe fn raw<'a>(data: *const u8, len: usize) -> &'a [u8] {
    match data.is_null() {
        true => &[],
        false => unsafe { std::slice::from_raw_parts(data, len) },
    }
}

pub unsafe fn bytes<'a>(s: hegel_string_t) -> &'a [u8] {
    unsafe { raw(s.data.cast::<u8>(), s.len) }
}

pub fn said(ctx: *mut HegelContext) -> String {
    String::from_utf8_lossy(unsafe { bytes(hegel_c::hegel_context_last_error(ctx)) }).into_owned()
}

fn checked(ctx: *mut HegelContext, code: hegel_result_t) -> Result<(), Halt> {
    match code {
        HEGEL_OK => Ok(()),
        HEGEL_E_STOP_TEST => Err(Halt::Overrun),
        HEGEL_E_ASSUME => Err(Halt::Invalid),
        code => Err(Halt::Fatal(format!(
            "Hegel gives {}: {}",
            code as i32,
            said(ctx)
        ))),
    }
}

macro_rules! hegel {
    ($ctx:expr, $name:ident($($arg:expr),*)) => {
        checked($ctx, unsafe { hegel_c::$name($ctx, $($arg),*) })
    };
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

pub struct Engine {
    ctx: *mut HegelContext,
    run: *mut HegelRun,
    step: u64,
}

pub struct Case {
    ctx: *mut HegelContext,
    tc: *mut HegelTestCase,
    machine: *mut HegelStateMachine,
    step: u64,
}

pub struct Ended {
    pub result: *mut HegelRunResult,
    pub failed: bool,
    pub caveat: Option<String>,
}

impl Engine {
    pub fn start(cases: u64, key: Option<&str>) -> Result<Engine, String> {
        let ctx = hegel_c::hegel_context_new();
        let made = || -> Result<Engine, Halt> {
            let mut settings = ptr::null_mut();
            hegel!(ctx, hegel_settings_new(&mut settings))?;
            let store = key.map_or(String::new(), |_| store());
            let database = text(store.as_bytes());
            hegel!(ctx, hegel_settings_set_test_cases(settings, cases))?;
            hegel!(ctx, hegel_settings_set_verbosity(settings, QUIET))?;
            hegel!(ctx, hegel_settings_set_print_blob(settings, false))?;
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
                hegel_run_start(settings, None, ptr::null_mut(), &mut run)
            );
            hegel!(ctx, hegel_settings_free(settings))?;
            started?;
            let mut step = 0;
            hegel!(ctx, hegel_label_from_name(text(STEP.as_bytes()), &mut step))?;
            Ok(Engine { ctx, run, step })
        };
        made().map_err(|halt| match halt {
            Halt::Fatal(e) => e,
            Halt::Overrun | Halt::Invalid => "Hegel stops before the run starts".into(),
        })
    }

    pub fn next(&mut self) -> Result<Option<Case>, String> {
        let mut tc = ptr::null_mut();
        match hegel!(self.ctx, hegel_next_test_case(self.run, &mut tc)) {
            Ok(()) => Ok((!tc.is_null()).then_some(Case {
                ctx: self.ctx,
                tc,
                machine: ptr::null_mut(),
                step: self.step,
            })),
            Err(Halt::Fatal(e)) => Err(e),
            Err(Halt::Overrun | Halt::Invalid) => Err("Hegel stops between two cases".into()),
        }
    }

    pub fn end(&mut self) -> Result<Ended, String> {
        let ctx = self.ctx;
        let read = || -> Result<Ended, Halt> {
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
                let mut told = ptr::null();
                hegel!(ctx, hegel_failure_caveat(failure, &mut told))?;
                caveat = unsafe { told.as_ref() }
                    .map(|t| String::from_utf8_lossy(unsafe { bytes(*t) }).into_owned());
                hegel!(ctx, hegel_failure_free(failure))?;
            }
            Ok(Ended {
                result,
                failed: status as u32 == FAILED,
                caveat,
            })
        };
        read().map_err(|halt| match halt {
            Halt::Fatal(e) => e,
            Halt::Overrun | Halt::Invalid => "Hegel stops after the run".into(),
        })
    }

    pub fn free(&mut self, ended: Ended) {
        unsafe {
            let _ = hegel_c::hegel_run_result_free(self.ctx, ended.result);
        }
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        unsafe {
            let _ = hegel_c::hegel_run_free(self.ctx, self.run);
            let _ = hegel_c::hegel_context_free(self.ctx);
        }
    }
}

impl Case {
    pub fn raw(&self) -> *mut HegelTestCase {
        self.tc
    }

    pub fn machine(&mut self, rules: &[String], steps: i64) -> Result<(), Halt> {
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
        let mut concurrency = 0;
        hegel!(
            ctx,
            hegel_state_machine_start(tc, self.machine, &mut concurrency)
        )
    }

    pub fn round(&mut self) -> Result<bool, Halt> {
        let (mut group, mut concurrency) = (0, 0);
        hegel!(self.ctx, hegel_start_span(self.tc, self.step))?;
        hegel!(
            self.ctx,
            hegel_state_machine_next_group(self.tc, self.machine, &mut group, &mut concurrency)
        )?;
        Ok(group != i64::MIN)
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
        hegel!(self.ctx, hegel_stop_span(self.tc, discard))
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
            let _ = hegel_c::hegel_state_machine_free(self.ctx, self.machine);
            let _ = hegel_c::hegel_test_case_free(self.ctx, self.tc);
        }
        marked.map_err(|halt| match halt {
            Halt::Fatal(e) => e,
            Halt::Overrun | Halt::Invalid => "Hegel stops at the end of a case".into(),
        })
    }
}
