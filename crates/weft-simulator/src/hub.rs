use std::collections::HashMap;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use serde_json::Value as Json;
use weft_hegel::Generator;
use weft_parse::{Draw, Ran, Role, Span, State, Use};
use weft_wire::{DONE, JOIN, Reader, Short, TURN, Writer};

use crate::schema::{Branch, Tag, branches, live, narrowed, tags, whole};
use crate::{Action, Hello, Member, Report, Start, Turn};

const REFUSED: i32 = 1;
const CUT: &str = "the message of the runner is too short";
const SOURCES: [&str; 2] = ["outputs", "errors"];

#[derive(Default)]
struct Desk {
    turn: Option<String>,
    report: Option<Report>,
    done: Option<Result<Ran, String>>,
    lost: Option<String>,
}

struct Read {
    line: usize,
    source: &'static str,
    tag: Tag,
}

pub struct Seat {
    hello: Hello,
    reads: Vec<Read>,
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
    pools: Vec<String>,
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

fn hello(session: &Session, said: &Json) -> Result<Seat, String> {
    let Session {
        uses, draws, pools, ..
    } = session;
    let all = said
        .get("uses")
        .and_then(Json::as_array)
        .map_or(&[][..], Vec::as_slice);
    if all.len() != uses.len() {
        return Err(format!(
            "the runner sends {} uses, and `impl` has {} lines",
            all.len(),
            uses.len()
        ));
    }
    let mut hello = Hello {
        sized: true,
        ..Hello::default()
    };
    let mut reads: Vec<Read> = Vec::new();
    let mut found = vec![0; draws.len()];
    let mut drawn = |generator: Generator, line: &str| {
        let mut generator = live(generator)
            .ok_or_else(|| format!("{line} has an input with no value that weft draws"))?;
        for (draw, found) in draws.iter().zip(&mut found) {
            let (narrow, count) = narrowed(generator, draw);
            *found += count;
            generator = narrow;
        }
        Ok::<Generator, String>(generator)
    };
    for (at, (u, node)) in uses
        .iter()
        .zip(all)
        .enumerate()
        .filter(|(_, (_, node))| !node.is_null())
    {
        let line = u.line();
        let schema = |key: &str| {
            node.get(key)
                .ok_or_else(|| format!("the runner sends no `{key}` schema for {line}"))
        };
        let before = reads.len();
        for (source, name) in SOURCES.into_iter().zip([&u.output, &u.error]) {
            if name.is_some() {
                reads.extend(
                    tags(schema(&source[..source.len() - 1])?)?
                        .into_iter()
                        .map(|tag| Read {
                            line: at,
                            source,
                            tag,
                        }),
                );
            }
        }
        let input = u
            .input
            .as_ref()
            .map(|name| Ok::<_, String>((name.to_string(), schema("input")?)))
            .transpose()?;
        match (u.role, input, &u.tag, &u.verb) {
            (Role::Read, ..) if reads.len() > before => {}
            (Role::Read, ..) => {
                return Err(format!(
                    "{line} is a `read`, and its schema has no `read` annotation"
                ));
            }
            (Role::Start, None, ..) => hello.start.push(Start {
                line: at,
                input: None,
                pool: u.pool.clone(),
            }),
            (Role::Start, Some((name, json)), tag, _) => {
                let generator = match tag {
                    None => whole(&name, json, pools)?,
                    Some(tag) => {
                        let mut tagged: Vec<Branch> = branches(&name, json, pools)?
                            .into_iter()
                            .filter(|b| b.verb.as_ref() == Some(tag))
                            .collect();
                        match (tagged.pop(), tagged.is_empty()) {
                            (Some(branch), true) => branch.generator,
                            _ => {
                                return Err(format!(
                                    "{line} needs one branch with the verb `{tag}`"
                                ));
                            }
                        }
                    }
                };
                hello.start.push(Start {
                    line: at,
                    input: Some(drawn(generator, &line)?),
                    pool: u.pool.clone(),
                });
            }
            (Role::Run, input, _, verb) => {
                let from = u
                    .from
                    .as_ref()
                    .map(|(pool, consume)| Generator::Pool(pool.clone(), *consume));
                let inputs: Vec<(String, Option<Generator>)> = match (input, verb) {
                    (None, verb) => verb.iter().map(|verb| (verb.clone(), None)).collect(),
                    (Some((name, json)), Some(verb)) => {
                        vec![(verb.clone(), Some(whole(&name, json, pools)?))]
                    }
                    (Some((name, json)), None) => branches(&name, json, pools)?
                        .into_iter()
                        .filter_map(|b| Some((b.verb?, Some(b.generator))))
                        .collect(),
                };
                if inputs.is_empty() {
                    return Err(format!(
                        "{line} is a `run`, and its schema has no `verb` annotation"
                    ));
                }
                for (verb, input) in inputs {
                    hello.actions.push(Action {
                        verb,
                        line: at,
                        drawn: Generator::Tuple(vec![
                            from.clone().unwrap_or(Generator::Constant(Json::Null)),
                            input
                                .map(|g| drawn(g, &line))
                                .transpose()?
                                .unwrap_or(Generator::Constant(Json::Null)),
                        ]),
                        pool: u.pool.clone(),
                    });
                }
            }
        }
    }
    hello.reads = reads
        .iter()
        .map(|r| format!("{}.{}", r.tag.object, r.tag.key))
        .collect();
    hello.errors = reads
        .iter()
        .filter(|r| r.source == SOURCES[1])
        .map(|r| (r.tag.object.clone(), r.tag.key.clone()))
        .collect();
    Ok(Seat {
        hello,
        reads,
        found,
        desk: Mutex::default(),
        changed: Condvar::new(),
    })
}

fn state(reads: &[Read], said: &Json) -> Result<State, String> {
    let mut state = State::new();
    for Read { line, source, tag } in reads {
        let Some(whole) = said.get(source).and_then(|all| all.get(line.to_string())) else {
            continue;
        };
        let (object, key) = (&tag.object, &tag.key);
        let value = tag
            .path
            .iter()
            .try_fold(whole, |node, part| node.get(part))
            .ok_or_else(|| format!("the value `{whole}` has no `{object}.{key}`"))?;
        state
            .entry(object.clone())
            .or_default()
            .insert(key.clone(), value.clone());
    }
    Ok(state)
}

fn report(reads: &[Read], said: &Json) -> Result<Report, String> {
    match (
        SOURCES.iter().all(|source| said.get(source).is_some()),
        said.get("thrown").and_then(Json::as_str),
        said.get("refused"),
    ) {
        (true, None, None) => {
            let added = said
                .get("added")
                .and_then(Json::as_array)
                .map_or(Vec::new(), |all| {
                    all.iter().filter_map(Json::as_u64).collect()
                });
            Ok(Report::State(state(reads, said)?, added))
        }
        (false, Some(error), None) => Ok(Report::Thrown(error.to_string())),
        (false, None, Some(_)) => Ok(Report::Refused),
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
                    .any(|s| s.hello.actions.iter().any(|action| action.verb == **verb))
            });
            let (Some(verb), None) = (missing, &joined.lost) else {
                let unused = self.draws.iter().enumerate().find(|(at, draw)| {
                    !draw.keys && joined.seats.iter().all(|s| s.found[*at] == 0)
                });
                return match (joined.lost.clone(), unused) {
                    (Some(why), _) => Err(why),
                    (None, Some((_, draw))) => Err(format!(
                        "the line `{}` of `draw` names a field of no input, or no value of the line is a value of the field",
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
                    format!(
                        "the verb `{verb}` has no runner with a `run` line of the verb `{verb}`"
                    )
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
    pub fn open(
        &self,
        name: &str,
        code: String,
        uses: Vec<Use>,
        draws: Vec<Draw>,
        pools: Vec<String>,
    ) -> Arc<Session> {
        let session = Arc::new(Session {
            code,
            uses,
            draws,
            pools,
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
                let report = report(&seat.reads, said)?;
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
