use std::collections::HashMap;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use serde_json::Value as Json;
use weft_hegel::Halt;
use weft_parse::{Ran, Span, State};
use weft_wire::{DONE, JOIN, OK, REFUSED, Reader, Short, TURN, Writer};

use crate::protocol::{Report, Turn};
use crate::proxy::Hop;

const CUT: &str = "the message of the runner is too short";
const SLOW: Duration = Duration::from_secs(300);

type Said = Result<String, (i32, String)>;

#[derive(Default)]
struct Desk {
    turn: Option<String>,
    report: Option<Report>,
    done: Option<Result<Ran, String>>,
    lost: Option<String>,
}

pub(crate) struct Seat {
    adapter: String,
    serves: Option<String>,
    desk: Mutex<Desk>,
    changed: Condvar,
}

#[derive(Default)]
struct Joined {
    seats: Vec<Arc<Seat>>,
    lost: Option<String>,
}

pub(crate) struct Session {
    code: String,
    impls: Vec<String>,
    hops: Mutex<Vec<Hop>>,
    clock: std::sync::atomic::AtomicUsize,
    joined: Mutex<Joined>,
    changed: Condvar,
}

#[derive(Default)]
pub(crate) struct Hub {
    sessions: Mutex<HashMap<String, Arc<Session>>>,
}

#[derive(Default)]
pub(crate) struct Peer {
    session: Option<Arc<Session>>,
    seat: Option<Arc<Seat>>,
}

pub(crate) struct Remote {
    seat: Arc<Seat>,
    session: Arc<Session>,
}

fn held<T>(lock: &Mutex<T>) -> MutexGuard<'_, T> {
    lock.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn hello(session: &Session, said: &Json) -> Result<Seat, String> {
    let adapter = said
        .get("adapter")
        .and_then(Json::as_str)
        .ok_or("the runner sends no `adapter`")?
        .to_string();
    if !session.impls.contains(&adapter) {
        return Err(format!("the diagram has no `impl \"{adapter}\"`"));
    }
    Ok(Seat {
        serves: said.get("serves").and_then(Json::as_str).map(String::from),
        adapter,
        desk: Mutex::default(),
        changed: Condvar::new(),
    })
}

fn state(said: &Json) -> Result<State, String> {
    let given = said
        .get("states")
        .and_then(Json::as_object)
        .ok_or_else(|| format!("the runner reports `{said}` with no `states`"))?;
    let mut state = State::new();
    for (object, value) in given {
        let fields = value.as_object().ok_or_else(|| {
            format!("the `state` function of `{object}` gives `{value}`, and the state of an object is a record of its `state` keys")
        })?;
        let entry = state.entry(object.clone()).or_default();
        for (key, field) in fields {
            entry.insert(key.clone(), field.clone());
        }
    }
    Ok(state)
}

fn report(said: &Json) -> Result<Report, String> {
    match (
        said.get("states").is_some(),
        said.get("thrown").and_then(Json::as_str),
        said.get("refused"),
    ) {
        (true, None, None) => Ok(Report::State(
            state(said)?,
            said.get("result").cloned().unwrap_or(Json::Null),
        )),
        (false, Some(error), None) => Ok(Report::Thrown(error.to_string())),
        (false, None, Some(Json::String(cause))) => Ok(Report::Refused(cause.clone())),
        _ => Err(format!("the runner reports `{said}`")),
    }
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
        start: spans(node.get("start")?)?,
        actions: node
            .get("actions")?
            .as_object()?
            .iter()
            .map(|(verb, all)| Some((verb.clone(), spans(all)?)))
            .collect::<Option<_>>()?,
    })
}

impl Seat {
    fn wait<T>(
        &self,
        limit: Option<Duration>,
        mut ready: impl FnMut(&mut Desk) -> Option<T>,
    ) -> Result<T, String> {
        let until = limit.map(|limit| Instant::now() + limit);
        let mut desk = held(&self.desk);
        loop {
            if let Some(found) = ready(&mut desk) {
                return Ok(found);
            }
            if let Some(why) = &desk.lost {
                return Err(why.clone());
            }
            let left = until.map_or(Ok(SLOW), |until| {
                until.checked_duration_since(Instant::now()).ok_or_else(|| {
                    format!(
                        "the runner of `impl \"{}\"` sends no report in {} seconds",
                        self.adapter,
                        SLOW.as_secs()
                    )
                })
            })?;
            desk = self
                .changed
                .wait_timeout(desk, left)
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .0;
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
    pub(crate) fn serves(&self, adapter: &str, wait: Duration) -> Result<String, String> {
        let until = Instant::now() + wait;
        let mut joined = held(&self.joined);
        loop {
            if let Some(url) = joined
                .seats
                .iter()
                .find(|s| s.adapter == adapter)
                .and_then(|s| s.serves.clone())
            {
                return Ok(url);
            }
            if let Some(why) = &joined.lost {
                return Err(why.clone());
            }
            let left = until
                .checked_duration_since(Instant::now())
                .ok_or_else(|| {
                    format!("the runner of `impl \"{adapter}\"` serves no `serve` url")
                })?;
            joined = self
                .changed
                .wait_timeout(joined, left)
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .0;
        }
    }

    pub(crate) fn tick(&self) -> usize {
        self.clock
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    }

    pub(crate) fn record(&self, hop: Hop) {
        held(&self.hops).push(hop);
    }

    pub(crate) fn members(self: &Arc<Self>, wait: Duration) -> Result<Vec<Remote>, String> {
        let until = Instant::now() + wait;
        let mut joined = held(&self.joined);
        loop {
            let missing = self
                .impls
                .iter()
                .find(|adapter| !joined.seats.iter().any(|s| s.adapter == **adapter))
                .map(|adapter| format!("no runner of `impl \"{adapter}\"` joins"));
            let (Some(missing), None) = (missing, &joined.lost) else {
                return match joined.lost.clone() {
                    Some(why) => Err(why),
                    None => Ok({
                        let mut seats = joined.seats.clone();
                        seats.sort_by_key(|seat| {
                            self.impls
                                .iter()
                                .position(|adapter| *adapter == seat.adapter)
                        });
                        seats
                    }
                    .iter()
                    .map(|seat| Remote {
                        seat: seat.clone(),
                        session: self.clone(),
                    })
                    .collect()),
                };
            };
            let left = until
                .checked_duration_since(Instant::now())
                .ok_or(missing)?;
            joined = self
                .changed
                .wait_timeout(joined, left)
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .0;
        }
    }

    pub(crate) fn lose(&self, why: &str) {
        let mut joined = held(&self.joined);
        joined.lost.get_or_insert_with(|| why.to_string());
        joined.seats.iter().for_each(|seat| seat.lose(why));
        self.changed.notify_all();
    }
}

impl Hub {
    pub(crate) fn open(&self, name: &str, code: String, impls: Vec<String>) -> Arc<Session> {
        let session = Arc::new(Session {
            code,
            impls,
            hops: Mutex::default(),
            clock: std::sync::atomic::AtomicUsize::new(0),
            joined: Mutex::default(),
            changed: Condvar::new(),
        });
        held(&self.sessions).insert(name.to_string(), session.clone());
        session
    }

    pub(crate) fn close(&self, name: &str) {
        if let Some(session) = held(&self.sessions).remove(name) {
            session.lose("weft ends the run");
        }
    }
}

impl Peer {
    fn session(&self) -> Result<&Arc<Session>, String> {
        self.session
            .as_ref()
            .ok_or_else(|| "the runner joins a diagram before it runs".to_string())
    }

    fn join(&mut self, hub: &Hub, name: &str) -> Result<String, String> {
        let session = held(&hub.sessions)
            .get(name)
            .cloned()
            .ok_or_else(|| format!("weft runs no diagram `{name}`"))?;
        let code = session.code.clone();
        self.session = Some(session);
        self.seat = None;
        Ok(code)
    }

    fn turn(&mut self, said: &Json) -> Result<String, String> {
        let seat = match &self.seat {
            Some(seat) => {
                let report = report(said)?;
                seat.set(|d| d.report = Some(report));
                seat.clone()
            }
            None => {
                let session = self.session()?;
                let seat = Arc::new(hello(session, said).inspect_err(|e| session.lose(e))?);
                let mut joined = held(&session.joined);
                if let Some(why) = &joined.lost {
                    return Err(why.clone());
                }
                joined.seats.push(seat.clone());
                session.changed.notify_all();
                drop(joined);
                self.seat.insert(seat).clone()
            }
        };
        seat.wait(None, |d| d.turn.take())
    }

    fn done(&mut self, said: &Json) -> Result<String, String> {
        let done = match (said.get("error").and_then(Json::as_str), said.get("ran")) {
            (Some(e), _) => Err(e.to_string()),
            (None, Some(node)) => {
                ran(node).ok_or_else(|| format!("the runner sends the `ran` `{node}`"))
            }
            (None, None) => Err(format!("the runner ends with `{said}`")),
        };
        match (&self.seat, &done) {
            (Some(seat), Err(e)) => seat.lose(e),
            (None, Err(e)) => self.session()?.lose(e),
            _ => {}
        }
        if let Some(seat) = &self.seat {
            seat.set(|d| d.done = Some(done));
        }
        Ok(String::new())
    }

    fn answer(&mut self, hub: &Hub, r: &mut Reader) -> Said {
        let refused = |e: String| (REFUSED, e);
        let call = r.call().map_err(|Short| refused(CUT.into()))?;
        let part = r.bytes().map_err(|Short| refused(CUT.into()))?;
        let said = || {
            serde_json::from_slice(part)
                .map_err(|e| refused(format!("{e}: {}", String::from_utf8_lossy(part))))
        };
        match call {
            JOIN => self
                .join(hub, &String::from_utf8_lossy(part))
                .map_err(refused),
            TURN => self.turn(&said()?).map_err(refused),
            DONE => self.done(&said()?).map_err(refused),
            _ => Err(refused(format!("weft has no call {call}"))),
        }
    }

    pub(crate) fn serve(&mut self, hub: &Hub, message: &[u8]) -> Vec<u8> {
        match self.answer(hub, &mut Reader(message)) {
            Ok(said) => Writer::reply(OK, said.as_bytes()).0,
            Err((code, e)) => Writer::reply(code, e.as_bytes()).0,
        }
    }
}

impl Drop for Peer {
    fn drop(&mut self) {
        if let Some(seat) = &self.seat {
            seat.lose("the runner closes the connection");
        }
    }
}

impl Remote {
    pub(crate) fn adapter(&self) -> &str {
        &self.seat.adapter
    }

    pub(crate) fn turn(&mut self, turn: &Turn) -> Result<Report, Halt> {
        self.seat.set(|d| d.turn = Some(turn.json()));
        Ok(self.seat.wait(Some(SLOW), |d| d.report.take())?)
    }

    pub(crate) fn finish(&mut self) -> Result<Ran, String> {
        self.seat.set(|d| d.turn = Some(Turn::Done.json()));
        self.seat.wait(Some(SLOW), |d| d.done.take())?
    }

    pub(crate) fn hops(&mut self) -> Vec<Hop> {
        std::mem::take(&mut *held(&self.session.hops))
    }
}

impl Drop for Remote {
    fn drop(&mut self) {
        self.seat.lose("weft stops the run");
    }
}
