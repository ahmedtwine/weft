use std::collections::HashMap;
use std::ffi::c_void;
use std::mem::MaybeUninit;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use serde_json::Value as Json;
use weft_parse::{Pairs, Ran, Span, State, Value};
use weft_wire::{DONE, JOIN, Reader, Short, TURN, Wire, Writer};

use crate::engine::{bytes, said};
use crate::host::{CALLS, Host, PRINT, dispatch, kind};
use crate::{Hello, Mark, Member, Report, Tagged, Turn};

const INVALID: i32 = -5;
const TOKEN: usize = 16;
const RUN: &str = "hegel_run_t";
const CASE: &str = "hegel_test_case_t";
const RESULT: &str = "hegel_run_result_t";
const FAILURE: &str = "hegel_failure_t";
const MARK: &str = "hegel_mark_complete";

#[derive(Default)]
struct Desk {
    case: Option<(u64, usize)>,
    taken: u64,
    turn: Option<String>,
    report: Option<Report>,
    mark: Option<Mark>,
    ended: Option<usize>,
    done: Option<Result<Ran, String>>,
    origins: HashMap<String, String>,
    lost: Option<String>,
}

pub struct Seat {
    hello: Hello,
    desk: Mutex<Desk>,
    changed: Condvar,
}

#[derive(Default)]
struct Joined {
    seats: Vec<Arc<Seat>>,
    lost: Option<String>,
}

pub struct Session {
    dir: PathBuf,
    joined: Mutex<Joined>,
    changed: Condvar,
}

#[derive(Default)]
pub struct Hub {
    sessions: Mutex<HashMap<String, Arc<Session>>>,
}

#[derive(Default)]
pub struct Peer {
    host: Host,
    seat: Option<Arc<Seat>>,
}

pub struct Remote {
    seat: Arc<Seat>,
}

fn held<T>(lock: &Mutex<T>) -> MutexGuard<'_, T> {
    lock.lock().unwrap_or_else(|p| p.into_inner())
}

fn read<T: Wire>(r: &mut Reader) -> Result<T, String> {
    r.get()
        .map_err(|Short| "the message of the runner is too short".to_string())
}

fn parsed(r: &mut Reader) -> Result<Json, String> {
    let text = r
        .bytes()
        .map_err(|Short| "the message of the runner is too short".to_string())?;
    serde_json::from_slice(text).map_err(|e| format!("{e}: {}", String::from_utf8_lossy(text)))
}

fn texts(node: Option<&Json>) -> Vec<String> {
    node.and_then(Json::as_array)
        .into_iter()
        .flatten()
        .filter_map(|s| s.as_str().map(String::from))
        .collect()
}

fn tagged(node: &Json, key: &str) -> Vec<Tagged> {
    node.get(key)
        .and_then(Json::as_array)
        .into_iter()
        .flatten()
        .map(|t| Tagged {
            tag: t
                .get("tag")
                .and_then(Json::as_str)
                .unwrap_or_default()
                .to_string(),
            from: texts(t.get("from")),
            run: texts(t.get("run")),
        })
        .collect()
}

fn real(hello: &Hello, dir: &Path) -> Result<(), String> {
    let inside = |file: &String| Path::new(file).starts_with(dir);
    let driver = |said: String, file: &String| {
        format!(
            "{said} is from `{file}`, a file of the driver: only the code outside `{}` gives it",
            dir.display()
        )
    };
    for t in hello.reads.iter().chain(&hello.actions) {
        if t.from.is_empty() {
            return Err(format!(
                "the tag `{}` has no file: the code registers each tag",
                t.tag
            ));
        }
        if let Some(file) = t.from.iter().find(|f| inside(f)) {
            return Err(driver(format!("the tag `{}`", t.tag), file));
        }
    }
    for (kind, all, needed) in [
        ("action", &hello.actions, true),
        ("mock", &hello.mocks, false),
    ] {
        for t in all {
            let said = format!("the function of the {kind} `{}`", t.tag);
            match t.run.as_slice() {
                [] if needed => {
                    return Err(format!(
                        "{said} is no export of the code: the code exports each function"
                    ));
                }
                [first, ..] if t.run.iter().all(inside) => return Err(driver(said, first)),
                _ => {}
            }
        }
    }
    Ok(())
}

fn state(node: &Json) -> Result<State, String> {
    node.as_object()
        .ok_or_else(|| format!("the state `{node}` is no object"))?
        .iter()
        .map(|(object, keys)| {
            let pairs = keys
                .as_object()
                .ok_or_else(|| format!("the state of `{object}` is no object"))?
                .iter()
                .map(|(key, v)| match (v.as_str(), v.as_i64()) {
                    (Some(s), _) => Ok((key.clone(), Value::Text(s.to_string()))),
                    (_, Some(n)) => Ok((key.clone(), Value::Int(n.into()))),
                    _ => Err(format!(
                        "`{object}.{key}` is `{v}`, and a state is a text or a whole number"
                    )),
                })
                .collect::<Result<Pairs, String>>()?;
            Ok((object.clone(), pairs))
        })
        .collect()
}

fn spans(node: &Json) -> Option<Vec<Span>> {
    let line = |s: &Json, k: &str| usize::try_from(s.get(k)?.as_u64()?).ok();
    node.as_array()?
        .iter()
        .map(|s| {
            Some(Span {
                file: s.get("file")?.as_str()?.to_string(),
                from: line(s, "from")?,
                to: line(s, "to")?,
            })
        })
        .collect()
}

fn ran(node: &Json) -> Option<Ran> {
    Some(Ran {
        reset: spans(node.get("reset")?)?,
        actions: node
            .get("actions")?
            .as_object()?
            .iter()
            .map(|(verb, all)| Some((verb.clone(), spans(all)?)))
            .collect::<Option<_>>()?,
    })
}

impl Seat {
    fn wait<T>(&self, mut ready: impl FnMut(&mut Desk) -> Option<T>) -> Result<T, String> {
        let mut desk = held(&self.desk);
        loop {
            if let Some(found) = ready(&mut desk) {
                return Ok(found);
            }
            if let Some(why) = &desk.lost {
                return Err(why.clone());
            }
            desk = self.changed.wait(desk).unwrap_or_else(|p| p.into_inner());
        }
    }

    fn set(&self, change: impl FnOnce(&mut Desk)) {
        change(&mut held(&self.desk));
        self.changed.notify_all();
    }

    fn lose(&self, why: &str) {
        self.set(|d| {
            d.lost.get_or_insert_with(|| why.to_string());
        });
    }
}

impl Session {
    pub fn members(&self, verbs: &[String], wait: Duration) -> Result<Vec<Remote>, String> {
        let until = Instant::now() + wait;
        let mut joined = held(&self.joined);
        loop {
            let missing = verbs.iter().find(|verb| {
                !joined
                    .seats
                    .iter()
                    .any(|s| s.hello.actions.iter().any(|a| a.tag == **verb))
            });
            let (Some(verb), None) = (missing, &joined.lost) else {
                return match joined.lost.clone() {
                    Some(why) => Err(why),
                    None => Ok(joined
                        .seats
                        .iter()
                        .map(|seat| Remote { seat: seat.clone() })
                        .collect()),
                };
            };
            let left = until
                .checked_duration_since(Instant::now())
                .ok_or_else(|| {
                    format!("the verb `{verb}` has no runner with an action of the tag `{verb}`")
                })?;
            joined = self
                .changed
                .wait_timeout(joined, left)
                .unwrap_or_else(|p| p.into_inner())
                .0;
        }
    }

    pub fn lose(&self, why: &str) {
        let mut joined = held(&self.joined);
        joined.lost.get_or_insert_with(|| why.to_string());
        joined.seats.iter().for_each(|seat| seat.lose(why));
        self.changed.notify_all();
    }
}

impl Hub {
    pub fn open(&self, name: &str, dir: &Path) -> Arc<Session> {
        let session = Arc::new(Session {
            dir: dir.to_path_buf(),
            joined: Mutex::default(),
            changed: Condvar::new(),
        });
        held(&self.sessions).insert(name.to_string(), session.clone());
        session
    }

    pub fn close(&self, name: &str) {
        held(&self.sessions).remove(name);
    }
}

impl Peer {
    fn seat(&self) -> Result<&Arc<Seat>, String> {
        self.seat
            .as_ref()
            .ok_or_else(|| "the runner joins a diagram before it runs".to_string())
    }

    fn join(&mut self, hub: &Hub, r: &mut Reader) -> Result<(), String> {
        let name = r
            .bytes()
            .map(String::from_utf8_lossy)
            .map_err(|Short| "the message of the runner is too short".to_string())?;
        let name = name.as_ref();
        let said = parsed(r)?;
        let session = held(&hub.sessions)
            .get(name)
            .cloned()
            .ok_or_else(|| format!("weft runs no diagram `{name}`"))?;
        let hello = Hello {
            reads: tagged(&said, "reads"),
            actions: tagged(&said, "actions"),
            mocks: tagged(&said, "mocks"),
        };
        if let Err(e) = real(&hello, &session.dir) {
            session.lose(&e);
            return Err(e);
        }
        let seat = Arc::new(Seat {
            hello,
            desk: Mutex::default(),
            changed: Condvar::new(),
        });
        held(&session.joined).seats.push(seat.clone());
        session.changed.notify_all();
        self.seat = Some(seat);
        Ok(())
    }

    fn turn(&mut self, r: &mut Reader, w: &mut Writer) -> Result<(), String> {
        let said = parsed(r)?;
        let input = said.get("input").and_then(Json::as_str);
        let thrown = said.get("thrown").and_then(Json::as_str);
        let report = match (said.get("state"), input, thrown, said.get("refused")) {
            (Some(s), Some(input), None, None) => Some(Report::Ran(state(s)?, input.to_string())),
            (Some(s), None, None, None) => Some(Report::State(state(s)?)),
            (None, Some(input), Some(error), None) => {
                Some(Report::Thrown(input.to_string(), error.to_string()))
            }
            (None, None, None, Some(_)) => Some(Report::Refused),
            (None, None, None, None) => None,
            _ => return Err(format!("the runner reports `{said}`")),
        };
        let seat = self.seat()?;
        if report.is_some() {
            seat.set(|d| d.report = report);
        }
        w.bytes(seat.wait(|d| d.turn.take())?.as_bytes());
        Ok(())
    }

    fn done(&mut self, r: &mut Reader) -> Result<(), String> {
        let said = parsed(r)?;
        let done = match (said.get("error").and_then(Json::as_str), said.get("ran")) {
            (Some(e), _) => Err(e.to_string()),
            (None, Some(node)) => {
                ran(node).ok_or_else(|| format!("the runner sends the `ran` `{node}`"))
            }
            (None, None) => Err(format!("the runner ends with `{said}`")),
        };
        let seat = self.seat()?;
        if let Err(e) = &done {
            seat.lose(e);
        }
        seat.set(|d| d.done = Some(done));
        Ok(())
    }

    fn owned(&mut self, name: &str, r: &mut Reader, w: &mut Writer) -> Result<i32, String> {
        match name {
            "hegel_run_start" => {
                read::<u64>(r)?;
                read::<bool>(r)?;
                self.seat()?;
                w.put(self.host.share(kind(RUN), TOKEN as *mut c_void));
            }
            "hegel_next_test_case" => {
                read::<u64>(r)?;
                let offered = self.seat()?.wait(|d| match (d.case, d.ended) {
                    (Some((seq, tc)), _) if seq > d.taken => {
                        d.taken = seq;
                        Some(tc)
                    }
                    (_, Some(_)) => Some(0),
                    _ => None,
                })?;
                w.put(self.host.share(kind(CASE), offered as *mut c_void));
            }
            "hegel_mark_complete" => {
                let _ = read::<u64>(r)?;
                let status = read::<u32>(r)?;
                let origin = r
                    .opt()
                    .map_err(|Short| "the message of the runner is too short".to_string())?
                    .map(|o| String::from_utf8_lossy(o).into_owned());
                self.seat()?.set(|d| d.mark = Some(Mark { status, origin }));
            }
            "hegel_run_result" => {
                read::<u64>(r)?;
                let ended = self.seat()?.wait(|d| d.ended)?;
                w.put(self.host.share(kind(RESULT), ended as *mut c_void));
            }
            "hegel_failure_origin" => {
                let failure = self
                    .host
                    .handle(kind(FAILURE), read::<u64>(r)?)
                    .map_err(|Short| "the runner names no failure".to_string())?;
                let mut out = MaybeUninit::uninit();
                let code = unsafe {
                    hegel_c::hegel_failure_origin(self.host.ctx(), failure as _, out.as_mut_ptr())
                } as i32;
                if code != 0 {
                    return Ok(code);
                }
                let real =
                    String::from_utf8_lossy(unsafe { bytes(out.assume_init()) }).into_owned();
                let own = held(&self.seat()?.desk).origins.get(&real).cloned();
                w.bytes(own.unwrap_or(real).as_bytes());
            }
            _ => return Err(format!("weft owns the run, and a runner makes no `{name}`")),
        }
        Ok(0)
    }

    fn answer(
        &mut self,
        hub: &Hub,
        r: &mut Reader,
        w: &mut Writer,
    ) -> Result<(i32, String), String> {
        if read::<u32>(r)? != PRINT {
            return Err("the runner has another version of Hegel than weft".into());
        }
        let mut failed: Option<(i32, String)> = None;
        for _ in 1..read::<u32>(r)? {
            let marks =
                r.0.get(..2)
                    .and_then(|c| CALLS.get(usize::from(u16::from_le_bytes([c[0], c[1]]))))
                    == Some(&MARK);
            let code = self.call(hub, r, w)?;
            match (marks, code) {
                (true, _) => failed = None,
                (false, 0) => {}
                (false, code) => {
                    failed.get_or_insert_with(|| (code, said(self.host.ctx())));
                }
            }
        }
        let code = self.call(hub, r, w)?;
        Ok(match (code, failed) {
            (0, Some(failed)) => failed,
            (code, _) => (code, said(self.host.ctx())),
        })
    }

    fn call(&mut self, hub: &Hub, r: &mut Reader, w: &mut Writer) -> Result<i32, String> {
        let call = read::<u16>(r)?;
        let name = CALLS.get(usize::from(call)).copied().unwrap_or_default();
        match (call, name) {
            (JOIN, _) => self.join(hub, r).map(|()| 0),
            (TURN, _) => self.turn(r, w).map(|()| 0),
            (DONE, _) => self.done(r).map(|()| 0),
            (_, "") => Err(format!("weft has no call {call}")),
            (_, "hegel_run_start" | "hegel_run_start_blob" | "hegel_test_case_from_blob")
            | (_, "hegel_next_test_case")
            | (_, "hegel_mark_complete" | "hegel_run_result" | "hegel_failure_origin") => {
                self.owned(name, r, w)
            }
            _ => dispatch(&mut self.host, call, r, w)
                .map_err(|Short| format!("weft reads no `{name}` from the message of the runner")),
        }
    }

    pub fn serve(&mut self, hub: &Hub, message: &[u8]) -> Vec<u8> {
        let mut w = Writer::default();
        let mut reply = Writer::default();
        match self.answer(hub, &mut Reader(message), &mut w) {
            Ok((0, _)) => {
                reply.put(0i32);
                reply.0.extend(w.0);
            }
            Ok((code, why)) => {
                reply.put(code);
                reply.bytes(why.as_bytes());
            }
            Err(e) => {
                reply.put(INVALID);
                reply.bytes(e.as_bytes());
            }
        }
        reply.0
    }
}

impl Drop for Peer {
    fn drop(&mut self) {
        if let Some(seat) = &self.seat {
            seat.lose("the runner closes the connection");
        }
    }
}

impl Member for Remote {
    fn hello(&self) -> &Hello {
        &self.seat.hello
    }

    fn begin(&mut self, tc: *mut c_void) {
        self.seat.set(|d| {
            d.case = Some((d.case.map_or(1, |(seq, _)| seq + 1), tc as usize));
            d.mark = None;
            d.turn = None;
            d.report = None;
        });
    }

    fn turn(&mut self, turn: &Turn) -> Result<Report, String> {
        self.seat.set(|d| {
            if d.mark.is_none() {
                d.turn = Some(turn.json());
            }
        });
        self.seat
            .wait(|d| d.report.take().or_else(|| d.mark.clone().map(Report::Left)))
    }

    fn end(&mut self, turn: &Turn) -> Result<Mark, String> {
        self.seat.set(|d| {
            if d.mark.is_none() {
                d.turn = Some(turn.json());
            }
        });
        self.seat.wait(|d| d.mark.clone())
    }

    fn told(&mut self, origin: &str) {
        self.seat.set(|d| {
            if let Some(own) = d.mark.as_ref().and_then(|m| m.origin.clone()) {
                d.origins.insert(origin.to_string(), own);
            }
        });
    }

    fn finish(&mut self, result: *mut c_void) -> Result<Ran, String> {
        self.seat.set(|d| d.ended = Some(result as usize));
        self.seat.wait(|d| d.done.take())?
    }
}

impl Drop for Remote {
    fn drop(&mut self) {
        self.seat.lose("weft stops the run");
    }
}
