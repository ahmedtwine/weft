use std::collections::HashMap;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use serde_json::Value as Json;
use weft_parse::{Draw, Pairs, Ran, Role, Span, State, Use, Value};
use weft_wire::{DONE, JOIN, Reader, Short, TURN, Writer};

use crate::schema::{Choice, choice};
use crate::{Hello, Member, Report, Turn};

const REFUSED: i32 = 1;
const CUT: &str = "the message of the runner is too short";

#[derive(Default)]
struct Desk {
    turn: Option<String>,
    report: Option<Report>,
    done: Option<Result<Ran, String>>,
    lost: Option<String>,
}

pub struct Seat {
    hello: Hello,
    found: Vec<usize>,
    desk: Mutex<Desk>,
    changed: Condvar,
}

#[derive(Default)]
struct Joined {
    seats: Vec<Arc<Seat>>,
    lost: Option<String>,
}

pub struct Session {
    code: String,
    uses: Vec<Use>,
    draws: Vec<Draw>,
    joined: Mutex<Joined>,
    changed: Condvar,
}

#[derive(Default)]
pub struct Hub {
    sessions: Mutex<HashMap<String, Arc<Session>>>,
}

#[derive(Default)]
pub struct Peer {
    session: Option<Arc<Session>>,
    seat: Option<Arc<Seat>>,
}

pub struct Remote {
    seat: Arc<Seat>,
}

fn held<T>(lock: &Mutex<T>) -> MutexGuard<'_, T> {
    lock.lock().unwrap_or_else(|p| p.into_inner())
}

fn listed<'a>(node: &'a Json, key: &str) -> &'a [Json] {
    node.get(key)
        .and_then(Json::as_array)
        .map_or(&[], Vec::as_slice)
}

fn hello(uses: &[Use], draws: &[Draw], said: &Json) -> Result<(Hello, Vec<usize>), String> {
    let all = listed(said, "uses");
    if all.len() != uses.len() {
        return Err(format!(
            "the runner sends {} uses, and `impl` holds {} lines",
            all.len(),
            uses.len()
        ));
    }
    let mut hello = Hello::default();
    let mut found = vec![0; draws.len()];
    for (u, node) in uses.iter().zip(all).filter(|(_, node)| !node.is_null()) {
        let line = u.line();
        let reads = listed(node, "reads")
            .iter()
            .filter_map(|tag| tag.as_str().map(String::from));
        let mut inputs = listed(node, "inputs")
            .iter()
            .map(|input| {
                Ok((
                    input.get("tag").and_then(Json::as_str),
                    choice(
                        &u.schema.as_ref().unwrap_or(&u.function).to_string(),
                        input.get("schema").unwrap_or(&Json::Null),
                    )?
                    .live()
                    .ok_or_else(|| format!("{line} has an input with no value that weft draws"))?,
                ))
            })
            .collect::<Result<Vec<_>, String>>()?;
        for (draw, found) in draws.iter().zip(&mut found) {
            inputs = inputs
                .into_iter()
                .map(|(tag, choice)| {
                    let (choice, count) = choice.narrowed(draw);
                    *found += count;
                    (tag, choice)
                })
                .collect();
        }
        let before = hello.reads.len();
        hello.reads.extend(reads);
        match (u.role, inputs.as_slice()) {
            (Role::Read, []) if hello.reads.len() > before => {}
            (Role::Read, _) => {
                return Err(format!(
                    "{line} is a `read`, and its schema has no tag `<object>.<key>`"
                ));
            }
            (Role::Start, [(_, start)]) => hello.start.push(start.clone()),
            (Role::Start, []) if u.schema.is_none() => {
                hello.start.push(Choice::Enum(vec![Json::Null]));
            }
            (Role::Start, _) => {
                return Err(format!("{line} is a `start`, and it takes one input"));
            }
            (Role::Run, []) => {
                return Err(format!(
                    "{line} is a `run`, and its schema has no tag of a verb"
                ));
            }
            (Role::Run, _) => {
                for (tag, choice) in inputs {
                    let verb = tag.ok_or_else(|| format!("{line} has an input with no tag"))?;
                    hello.actions.push((verb.to_string(), choice));
                }
            }
        }
    }
    Ok((hello, found))
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

fn report(said: &Json) -> Result<Report, String> {
    match (
        said.get("state"),
        said.get("thrown").and_then(Json::as_str),
        said.get("refused"),
    ) {
        (Some(s), None, None) => state(s).map(Report::State),
        (None, Some(error), None) => Ok(Report::Thrown(error.to_string())),
        (None, None, Some(_)) => Ok(Report::Refused),
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
                    .any(|s| s.hello.actions.iter().any(|(tag, _)| tag == *verb))
            });
            let (Some(verb), None) = (missing, &joined.lost) else {
                let unused = self
                    .draws
                    .iter()
                    .enumerate()
                    .find(|(at, _)| joined.seats.iter().all(|s| s.found[*at] == 0));
                return match (joined.lost.clone(), unused) {
                    (Some(why), _) => Err(why),
                    (None, Some((_, draw))) => Err(format!(
                        "the line `draw {}` names a field of no input, or no value of the line is a value of the field",
                        draw.path.join(".")
                    )),
                    (None, None) => Ok(joined
                        .seats
                        .iter()
                        .map(|seat| Remote { seat: seat.clone() })
                        .collect()),
                };
            };
            let left = until
                .checked_duration_since(Instant::now())
                .ok_or_else(|| {
                    format!("the verb `{verb}` has no runner with an input of the tag `{verb}`")
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
    pub fn open(&self, name: &str, code: String, uses: Vec<Use>, draws: Vec<Draw>) -> Arc<Session> {
        let session = Arc::new(Session {
            code,
            uses,
            draws,
            joined: Mutex::default(),
            changed: Condvar::new(),
        });
        held(&self.sessions).insert(name.to_string(), session.clone());
        session
    }

    pub fn close(&self, name: &str) {
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
                let (hello, found) =
                    hello(&session.uses, &session.draws, said).inspect_err(|e| session.lose(e))?;
                let seat = Arc::new(Seat {
                    hello,
                    found,
                    desk: Mutex::default(),
                    changed: Condvar::new(),
                });
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
        seat.wait(|d| d.turn.take())
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

    fn answer(&mut self, hub: &Hub, r: &mut Reader) -> Result<String, String> {
        let call = r.call().map_err(|Short| CUT.to_string())?;
        let part = r.bytes().map_err(|Short| CUT.to_string())?;
        let said = || {
            serde_json::from_slice(part)
                .map_err(|e| format!("{e}: {}", String::from_utf8_lossy(part)))
        };
        match call {
            JOIN => self.join(hub, &String::from_utf8_lossy(part)),
            TURN => self.turn(&said()?),
            DONE => self.done(&said()?),
            _ => Err(format!("weft has no call {call}")),
        }
    }

    pub fn serve(&mut self, hub: &Hub, message: &[u8]) -> Vec<u8> {
        match self.answer(hub, &mut Reader(message)) {
            Ok(said) => Writer::reply(0, said.as_bytes()).0,
            Err(e) => Writer::reply(REFUSED, e.as_bytes()).0,
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

impl Member for Remote {
    fn hello(&self) -> &Hello {
        &self.seat.hello
    }

    fn turn(&mut self, turn: &Turn) -> Result<Report, String> {
        self.seat.set(|d| d.turn = Some(turn.json()));
        self.seat.wait(|d| d.report.take())
    }

    fn finish(&mut self) -> Result<Ran, String> {
        self.seat.set(|d| d.turn = Some(Turn::Done.json()));
        self.seat.wait(|d| d.done.take())?
    }
}

impl Drop for Remote {
    fn drop(&mut self) {
        self.seat.lose("weft stops the run");
    }
}
