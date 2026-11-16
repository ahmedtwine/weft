use std::collections::HashMap;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use serde_json::Value as Json;
use weft_hegel::{Generator, Halt};
use weft_parse::{Draw, Ran, Span, State};
use weft_wire::{
    ASSUME, DONE, GENERATE, JOIN, OK, POOL_ADD, POOL_GENERATE, REFUSED, Reader, STOP_TEST, Short,
    TARGET, TURN, Writer,
};

use crate::schema::{live, narrowed, whole};
use crate::shape::resolved;
use crate::{Action, Answer, Ask, Hello, Hop, Member, Report, Start, Turn};

const CUT: &str = "the message of the runner is too short";
const DRAW: &str = "`tc.draw`";
const SLOW: Duration = Duration::from_secs(300);

type Said = Result<String, (i32, String)>;

#[derive(Default)]
struct Desk {
    turn: Option<String>,
    report: Option<Report>,
    ask: Option<Ask>,
    answer: Option<Said>,
    done: Option<Result<Ran, String>>,
    lost: Option<String>,
}

pub struct Seat {
    hello: Hello,
    adapter: String,
    serves: Option<String>,
    states: Vec<String>,
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
    impls: Vec<String>,
    objects: Vec<String>,
    hops: Mutex<Vec<Hop>>,
    clock: std::sync::atomic::AtomicUsize,
    draws: Vec<Draw>,
    generators: Mutex<HashMap<String, Arc<Generator>>>,
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
    session: Arc<Session>,
}

fn held<T>(lock: &Mutex<T>) -> MutexGuard<'_, T> {
    lock.lock().unwrap_or_else(|p| p.into_inner())
}

fn function(node: &Json) -> Option<(String, String)> {
    let all = node.get("function")?.as_array()?;
    Some((
        all.first()?.as_str()?.to_string(),
        all.get(1)?.as_str()?.to_string(),
    ))
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
    let serves = said.get("serves").and_then(Json::as_str).map(String::from);
    let mut hello = Hello::default();
    let given = match said.get("states") {
        Some(Json::Object(all)) => all.clone(),
        None => serde_json::Map::new(),
        Some(other) => {
            return Err(format!(
                "the runner sends the states `{other}`, and weft reads a map of each key to its JSON Schema"
            ));
        }
    };
    let mut states = Vec::new();
    for (key, schema) in given {
        if key.split('.').count() > 2 || key.split('.').any(str::is_empty) {
            return Err(format!(
                "`weft.state(\"{key}\")` names `\"<object>\"` or `\"<object>.<key>\"`"
            ));
        }
        hello.schemas.push((key.clone(), resolved(&schema)));
        states.push(key);
    }
    let draws = &session.draws;
    let mut found = vec![0; draws.len()];
    let mut drawn = |node: &Json, line: &str| {
        let Some(schema) = node.get("input").filter(|i| !i.is_null()) else {
            return Ok::<Option<Generator>, String>(None);
        };
        let mut generator = live(whole(line, schema)?)
            .ok_or_else(|| format!("{line} has an input with no value that weft draws"))?;
        for (draw, found) in draws.iter().zip(&mut found) {
            let (narrow, count) = narrowed(generator, draw);
            *found += count;
            generator = narrow;
        }
        Ok(Some(generator))
    };
    let list = |key: &str| {
        said.get(key)
            .and_then(Json::as_array)
            .cloned()
            .unwrap_or_default()
    };
    for (line, node) in list("start").iter().enumerate() {
        let (module, export) =
            function(node).ok_or_else(|| format!("the runner sends the start `{node}`"))?;
        hello.start.push(Start {
            line,
            input: drawn(node, &format!("`{module}.{export}`"))?,
        });
    }
    for (line, node) in list("actions").iter().enumerate() {
        let (module, export) =
            function(node).ok_or_else(|| format!("the runner sends the action `{node}`"))?;
        if !session.objects.contains(&module) {
            return Err(format!("`module \"{module}\"` names no object of `setup`"));
        }
        hello.actions.push(Action {
            drawn: drawn(node, &format!("`{module}.{export}`"))?
                .unwrap_or(Generator::Constant(Json::Null)),
            object: module,
            verb: export,
            line,
        });
    }
    Ok(Seat {
        hello,
        adapter,
        serves,
        states,
        found,
        desk: Mutex::default(),
        changed: Condvar::new(),
    })
}

fn state(states: &[String], said: &Json) -> Result<State, String> {
    let given = said
        .get("states")
        .and_then(Json::as_object)
        .ok_or_else(|| format!("the runner reports `{said}` with no `states`"))?;
    let mut state = State::new();
    for name in states {
        let value = given
            .get(name)
            .ok_or_else(|| format!("the runner reports no `{name}`"))?;
        match name.split_once('.') {
            Some((object, key)) => {
                state
                    .entry(object.to_string())
                    .or_default()
                    .insert(key.to_string(), value.clone());
            }
            None => {
                let fields = value.as_object().ok_or_else(|| {
                    format!(
                        "`weft.state(\"{name}\")` reads `{value}`, and an object state is a record"
                    )
                })?;
                let entry = state.entry(name.clone()).or_default();
                for (key, field) in fields {
                    entry.insert(key.clone(), field.clone());
                }
            }
        }
    }
    Ok(state)
}

fn report(states: &[String], said: &Json) -> Result<Report, String> {
    match (
        said.get("states").is_some(),
        said.get("thrown").and_then(Json::as_str),
        said.get("refused"),
    ) {
        (true, None, None) => Ok(Report::State(state(states, said)?)),
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

fn asked(session: &Session, call: u8, part: &[u8]) -> Result<Ask, String> {
    let said: Json = serde_json::from_slice(part)
        .map_err(|e| format!("{e}: {}", String::from_utf8_lossy(part)))?;
    let text = |key: &str| {
        said.get(key)
            .and_then(Json::as_str)
            .map(String::from)
            .ok_or_else(|| format!("the runner sends `{said}` with no `{key}`"))
    };
    match call {
        GENERATE => {
            let key = said.to_string();
            let mut generators = held(&session.generators);
            if let Some(known) = generators.get(&key) {
                return Ok(Ask::Generate(known.clone()));
            }
            let made = Arc::new(live(whole(DRAW, &said)?).ok_or_else(|| {
                format!("{DRAW} takes the schema `{said}`, which has no value that weft draws")
            })?);
            generators.insert(key, made.clone());
            Ok(Ask::Generate(made))
        }
        POOL_ADD => Ok(Ask::PoolAdd(
            text("pool")?,
            said.get("value").cloned().unwrap_or_default(),
        )),
        POOL_GENERATE => Ok(Ask::PoolGenerate(
            text("pool")?,
            said.get("consume").and_then(Json::as_bool) == Some(true),
        )),
        _ => Ok(Ask::Target(
            said.get("value")
                .and_then(Json::as_f64)
                .ok_or_else(|| format!("the runner sends `{said}` with no `value`"))?,
            text("label")?,
        )),
    }
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
                .unwrap_or_else(|p| p.into_inner())
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
    pub fn serves(&self, adapter: &str, wait: Duration) -> Result<String, String> {
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
                .unwrap_or_else(|p| p.into_inner())
                .0;
        }
    }

    pub fn tick(&self) -> usize {
        self.clock
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    }

    pub fn record(&self, hop: Hop) {
        held(&self.hops).push(hop);
    }

    pub fn members(self: &Arc<Self>, wait: Duration) -> Result<Vec<Remote>, String> {
        let until = Instant::now() + wait;
        let mut joined = held(&self.joined);
        loop {
            let missing = self
                .impls
                .iter()
                .find(|adapter| !joined.seats.iter().any(|s| s.adapter == **adapter))
                .map(|adapter| format!("no runner of `impl \"{adapter}\"` joins"));
            let (Some(missing), None) = (missing, &joined.lost) else {
                let unused = self.draws.iter().enumerate().find(|(at, draw)| {
                    !draw.keys && joined.seats.iter().all(|s| s.found[*at] == 0)
                });
                return match (joined.lost.clone(), unused) {
                    (Some(why), _) => Err(why),
                    (None, Some((_, draw))) => Err(format!(
                        "the line `{}` of `draw` names a field of no input, or no value of the line is a value of the field",
                        draw.path.join(".")
                    )),
                    (None, None) => Ok({
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
        impls: Vec<String>,
        objects: Vec<String>,
        draws: Vec<Draw>,
    ) -> Arc<Session> {
        let session = Arc::new(Session {
            code,
            impls,
            objects,
            hops: Mutex::default(),
            clock: std::sync::atomic::AtomicUsize::new(0),
            draws,
            generators: Mutex::default(),
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
                let report = report(&seat.states, said)?;
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

    fn ask(&mut self, call: u8, part: &[u8]) -> Said {
        let refused = |e: String| (REFUSED, e);
        let seat = self
            .seat
            .as_ref()
            .ok_or_else(|| refused("the runner calls `tc` in an action".into()))?;
        let ask = asked(self.session().map_err(refused)?, call, part).map_err(refused)?;
        seat.set(|d| d.ask = Some(ask));
        seat.wait(None, |d| d.answer.take()).map_err(refused)?
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
            GENERATE | POOL_ADD | POOL_GENERATE | TARGET => self.ask(call, part),
            _ => Err(refused(format!("weft has no call {call}"))),
        }
    }

    pub fn serve(&mut self, hub: &Hub, message: &[u8]) -> Vec<u8> {
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

impl Member for Remote {
    fn hello(&self) -> &Hello {
        &self.seat.hello
    }

    fn turn(&mut self, turn: &Turn, answer: &mut Answer) -> Result<Report, Halt> {
        self.seat.set(|d| d.turn = Some(turn.json()));
        let mut halt = None;
        loop {
            let heard = self.seat.wait(Some(SLOW), |d| {
                d.report.take().map(Ok).or_else(|| d.ask.take().map(Err))
            })?;
            let ask = match heard {
                Ok(report) => return halt.map_or(Ok(report), Err),
                Err(ask) => ask,
            };
            let said = match halt.is_none().then(|| answer(ask)) {
                Some(Ok(Some(value))) => Ok(value.to_string()),
                Some(Ok(None)) => Err((ASSUME, String::new())),
                Some(Err(Halt::Fatal(e))) => {
                    halt = Some(Halt::Fatal(e.clone()));
                    Err((REFUSED, e))
                }
                Some(Err(stop)) => {
                    let code = match stop {
                        Halt::Invalid => ASSUME,
                        _ => STOP_TEST,
                    };
                    halt = Some(stop);
                    Err((code, String::new()))
                }
                None => Err((STOP_TEST, String::new())),
            };
            self.seat.set(|d| d.answer = Some(said));
        }
    }

    fn finish(&mut self) -> Result<Ran, String> {
        self.seat.set(|d| d.turn = Some(Turn::Done.json()));
        self.seat.wait(Some(SLOW), |d| d.done.take())?
    }

    fn hops(&mut self) -> Vec<Hop> {
        std::mem::take(&mut *held(&self.session.hops))
    }
}

impl Drop for Remote {
    fn drop(&mut self) {
        self.seat.lose("weft stops the run");
    }
}
