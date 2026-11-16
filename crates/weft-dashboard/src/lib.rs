use std::io::{self, BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::sync::{Condvar, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, UNIX_EPOCH};

use include_dir::{Dir, include_dir};
use opentelemetry::trace::Status;
use opentelemetry::{Array, Value};
use opentelemetry_sdk::error::OTelSdkResult;
use opentelemetry_sdk::trace::{BatchConfigBuilder, BatchSpanProcessor, SpanData, SpanExporter};
use serde_json::{Map, Value as Json, json};

pub const FILE: &str = "weft.file";
pub const STATUS: &str = "weft.status";
pub const TEST_CASES: &str = "weft.test_cases";
pub const STEPS: &str = "weft.steps";
pub const CALLS: &str = "weft.calls";
pub const CHECKS: &str = "weft.checks";
pub const DIAGRAM: &str = "weft.diagram";
pub const FLOW: &str = "weft.flow";
pub const FAILED: &str = "weft.failed";
pub const TEST_CASE: &str = "hegel.test_case";
pub const STEP: &str = "weft.step";
pub const ORDER: &str = "weft.order";
pub const INPUT: &str = "weft.input";
pub const STATE: &str = "weft.state";
pub const HOPS: &str = "weft.hops";
pub const CHECK: &str = "weft.check";
pub const REFUSED: &str = "weft.refused";
pub const METHOD: &str = "http.request.method";
pub const PATH: &str = "url.path";
pub const CODE: &str = "http.response.status_code";
pub const FROM: &str = "weft.from";
pub const TO: &str = "weft.to";
pub const BODY: &str = "weft.body";
pub const REPLY: &str = "weft.reply";

const ID: &str = "id";
const RUNNING: &str = "running";
const NAME: &str = "name";
const START: &str = "start";
const DURATION: &str = "duration";
const ACTION: &str = "action";
const OBJECT: &str = "object";
const KEY: &str = "key";
const VALUE: &str = "value";
const INITIAL: &str = "«initial»";
const ITEM: &str = "item";
const FIELD: &str = "field";

const TEXT: &str = "string";
const INTEGER: &str = "integer";
const FLOAT: &str = "float";
const DATETIME: &str = "datetime";
const BOOLEAN: &str = "boolean";

const FILES: (&str, &[(&str, &str)]) = (
    "files",
    &[
        (FILE, TEXT),
        (TEST_CASE, INTEGER),
        (STATUS, TEXT),
        (TEST_CASES, INTEGER),
        (STEPS, INTEGER),
        (CHECKS, INTEGER),
        (CHECK, TEXT),
        (START, DATETIME),
        (DURATION, FLOAT),
    ],
);

const TESTS: (&str, &[(&str, &str)]) = (
    "tests",
    &[
        (ID, TEXT),
        (FILE, TEXT),
        (TEST_CASE, INTEGER),
        (STEPS, INTEGER),
        (CALLS, INTEGER),
        (CHECK, TEXT),
        (START, DATETIME),
        (DURATION, FLOAT),
    ],
);

const STEPS_TABLE: (&str, &[(&str, &str)]) = (
    "steps",
    &[
        (FILE, TEXT),
        (TEST_CASE, INTEGER),
        (STEP, INTEGER),
        (ACTION, TEXT),
        (CHECK, TEXT),
        (DIAGRAM, TEXT),
        (FLOW, TEXT),
        (FAILED, BOOLEAN),
    ],
);

const STATES: (&str, &[(&str, &str)]) = (
    "states",
    &[
        (FILE, TEXT),
        (TEST_CASE, INTEGER),
        (STEP, INTEGER),
        (ACTION, TEXT),
        (CHECK, TEXT),
        (OBJECT, TEXT),
        (KEY, TEXT),
        (ITEM, TEXT),
        (FIELD, TEXT),
        (VALUE, TEXT),
    ],
);

static SITE: Dir = include_dir!("$CARGO_MANIFEST_DIR/site");
const INDEX: &str = "index.html";
const IDLE: Duration = Duration::from_secs(5);
const SHOWN: Duration = Duration::from_millis(200);
const EVENTS: &str =
    "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncache-control: no-store\r\n\r\n";

struct Log {
    rows: Mutex<(u64, Vec<String>)>,
    grown: Condvar,
}

static LOG: Log = Log {
    rows: Mutex::new((0, Vec::new())),
    grown: Condvar::new(),
};

#[derive(Debug)]
struct Exporter;

impl Log {
    fn held(&self) -> MutexGuard<'_, (u64, Vec<String>)> {
        self.rows
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn say(&self, rows: String) {
        self.held().1.push(rows);
        self.grown.notify_all();
    }

    fn stream(&self, mut to: TcpStream) -> io::Result<()> {
        to.write_all(EVENTS.as_bytes())?;
        let run = self.held().0;
        let mut sent = 0;
        loop {
            let new = {
                let mut log = self.held();
                if log.0 == run && log.1.len() == sent {
                    log = self
                        .grown
                        .wait_timeout(log, IDLE)
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .0;
                }
                match log.0 == run {
                    true => log.1[sent..].to_vec(),
                    false => vec![json!({ "reload": true }).to_string()],
                }
            };
            sent += new.len();
            for rows in &new {
                write!(to, "data: {rows}\n\n")?;
            }
            to.write_all(b": \n\n")?;
            to.flush()?;
        }
    }
}

fn shown(value: &Value) -> Json {
    match value {
        Value::Bool(b) => json!(b),
        Value::I64(i) => json!(i),
        Value::F64(f) => json!(f),
        Value::String(s) => json!(s.as_str()),
        Value::Array(Array::String(all)) => json!(
            all.iter()
                .map(opentelemetry::StringValue::as_str)
                .collect::<Vec<_>>()
        ),
        other => json!(other.to_string()),
    }
}

fn row(span: &SpanData) -> Map<String, Json> {
    let mut row: Map<String, Json> = span
        .attributes
        .iter()
        .map(|kv| (kv.key.to_string(), shown(&kv.value)))
        .collect();
    row.insert(NAME.into(), json!(span.name));
    row.insert(
        START.into(),
        json!(
            span.start_time
                .duration_since(UNIX_EPOCH)
                .map_or(0, |d| d.as_millis() as u64)
        ),
    );
    row.insert(
        DURATION.into(),
        json!(
            span.end_time
                .duration_since(span.start_time)
                .map_or(0.0, |d| d.as_secs_f64() * 1e3)
        ),
    );
    if let Status::Error { description } = &span.status {
        row.entry(CHECK).or_insert_with(|| json!(description));
    }
    row
}

fn children(value: &Json) -> Vec<(String, &Json)> {
    match value {
        Json::Object(fields) => fields
            .iter()
            .map(|(key, item)| (key.clone(), item))
            .collect(),
        Json::Array(items) if items.iter().any(|item| item.is_array() || item.is_object()) => items
            .iter()
            .enumerate()
            .map(|(i, item)| (format!("[{i}]"), item))
            .collect(),
        _ => Vec::new(),
    }
}

fn joined(path: &str, part: &str) -> String {
    match path.is_empty() || part.starts_with('[') {
        true => format!("{path}{part}"),
        false => format!("{path}.{part}"),
    }
}

fn text(value: &Json) -> String {
    match value {
        Json::String(text) => text.clone(),
        Json::Array(items)
            if !items.is_empty()
                && items
                    .iter()
                    .all(|item| !item.is_array() && !item.is_object()) =>
        {
            items.iter().map(text).collect::<Vec<_>>().join(", ")
        }
        other => other.to_string(),
    }
}

fn leaves(path: String, value: &Json, out: &mut Vec<(String, String)>) {
    match children(value).as_slice() {
        [] => out.push((path, text(value))),
        all => all
            .iter()
            .for_each(|(part, item)| leaves(joined(&path, part), item, out)),
    }
}

fn split(value: &Json) -> Vec<(String, String, String)> {
    match children(value).as_slice() {
        [] => vec![(String::new(), String::new(), text(value))],
        all => all
            .iter()
            .flat_map(|(item, child)| {
                let mut out = Vec::new();
                leaves(String::new(), child, &mut out);
                out.into_iter()
                    .map(move |(field, value)| (item.clone(), field, value))
            })
            .collect(),
    }
}

fn moment(span: &SpanData, row: &Map<String, Json>) -> Map<String, Json> {
    let step = row.get(STEP).and_then(Json::as_i64).unwrap_or_default();
    let action = match step {
        0 => INITIAL.to_string(),
        k => format!("{k} «{}»", span.name),
    };
    [FILE, TEST_CASE, CHECK, DIAGRAM, FLOW]
        .into_iter()
        .map(|column| {
            (
                column.to_string(),
                row.get(column).cloned().unwrap_or_default(),
            )
        })
        .chain([
            (STEP.to_string(), json!(step)),
            (ACTION.to_string(), json!(action)),
            (
                FAILED.to_string(),
                json!(matches!(span.status, Status::Error { .. })),
            ),
        ])
        .collect()
}

fn states(moment: &Map<String, Json>, row: &Map<String, Json>) -> Vec<Json> {
    let Some(state) = row
        .get(STATE)
        .and_then(Json::as_str)
        .and_then(|s| serde_json::from_str::<Map<String, Json>>(s).ok())
    else {
        return Vec::new();
    };
    state
        .iter()
        .flat_map(|(object, pairs)| {
            children(pairs).into_iter().flat_map(move |(key, value)| {
                split(value).into_iter().map(move |(item, field, value)| {
                    (object.clone(), key.clone(), item, field, value)
                })
            })
        })
        .map(|(object, key, item, field, value)| {
            let mut row = moment.clone();
            row.remove(DIAGRAM);
            row.remove(FLOW);
            row.remove(FAILED);
            row.extend(
                [
                    (OBJECT, object),
                    (KEY, key),
                    (ITEM, item),
                    (FIELD, field),
                    (VALUE, value),
                ]
                .map(|(c, v)| (c.to_string(), json!(v))),
            );
            Json::Object(row)
        })
        .collect()
}

impl SpanExporter for Exporter {
    async fn export(&self, batch: Vec<SpanData>) -> OTelSdkResult {
        let (mut files, mut tests, mut steps, mut states) =
            (Vec::new(), Vec::new(), Vec::new(), Vec::new());
        for span in &batch {
            let mut row = row(span);
            match (
                row.contains_key(STATUS),
                row.contains_key(STEP),
                row.get(TEST_CASE).cloned(),
            ) {
                (true, _, _) => files.push(Json::Object(row)),
                (false, true, _) if row.contains_key(STATE) => {
                    let moment = moment(span, &row);
                    states.extend(self::states(&moment, &row));
                    steps.push(Json::Object(moment));
                }
                (false, false, Some(case)) => {
                    files.push(Json::Object(Map::from_iter([
                        (FILE.to_string(), row[FILE].clone()),
                        (STATUS.to_string(), json!(RUNNING)),
                    ])));
                    row.insert(
                        ID.into(),
                        json!(format!("{}#{case}", row[FILE].as_str().unwrap_or_default())),
                    );
                    tests.push(Json::Object(row));
                }
                _ => {}
            }
        }
        let tables = [
            (FILES.0, files),
            (TESTS.0, tests),
            (STEPS_TABLE.0, steps),
            (STATES.0, states),
        ];
        LOG.say(
            Json::Object(
                tables
                    .map(|(name, rows)| (name.to_string(), Json::Array(rows)))
                    .into_iter()
                    .collect(),
            )
            .to_string(),
        );
        Ok(())
    }
}

pub fn processor() -> BatchSpanProcessor {
    BatchSpanProcessor::builder(Exporter)
        .with_batch_config(
            BatchConfigBuilder::default()
                .with_scheduled_delay(SHOWN)
                .build(),
        )
        .build()
}

pub fn clear() {
    let mut log = LOG.held();
    *log = (log.0 + 1, Vec::new());
    LOG.grown.notify_all();
}

fn schema() -> String {
    let table = |(name, columns): (&str, &[(&str, &str)])| {
        (
            name.to_string(),
            Json::Object(
                columns
                    .iter()
                    .map(|(c, t)| (c.to_string(), json!(t)))
                    .collect(),
            ),
        )
    };
    Json::Object(Map::from_iter([
        table(FILES),
        table(TESTS),
        table(STEPS_TABLE),
        table(STATES),
    ]))
    .to_string()
}

fn kind(path: &str) -> &'static str {
    match Path::new(path).extension().and_then(|e| e.to_str()) {
        Some("html") => "text/html; charset=utf-8",
        Some("js") => "text/javascript",
        Some("css") => "text/css",
        Some("wasm") => "application/wasm",
        Some("json") => "application/json",
        _ => "text/plain; charset=utf-8",
    }
}

fn respond(mut stream: TcpStream, opened: &(impl Fn() + Sync)) -> io::Result<()> {
    let mut request = String::new();
    BufReader::new(&stream).read_line(&mut request)?;
    let path = request
        .split_whitespace()
        .nth(1)
        .unwrap_or_default()
        .trim_start_matches('/');
    let tables;
    let (status, kind, body): (&str, &str, &[u8]) = match path {
        "events" => return LOG.stream(stream),
        "schema" => {
            tables = schema();
            ("200 OK", kind("schema.json"), tables.as_bytes())
        }
        "" => {
            opened();
            (
                "200 OK",
                kind(INDEX),
                SITE.get_file(INDEX).map_or(&[], |f| f.contents()),
            )
        }
        _ => match SITE.get_file(path) {
            Some(file) => ("200 OK", kind(path), file.contents()),
            None => ("404 Not Found", kind(path), &[]),
        },
    };
    write!(
        stream,
        "HTTP/1.1 {status}\r\ncontent-type: {kind}\r\ncache-control: no-store\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
        body.len()
    )?;
    stream.write_all(body)
}

pub fn serve(address: &str, opened: impl Fn() + Sync) -> Result<(), String> {
    let listener = TcpListener::bind(address).map_err(|e| format!("{address}: {e}"))?;
    println!("Weft: http://{address}");
    opened();
    thread::scope(|s| {
        for stream in listener.incoming().flatten() {
            let opened = &opened;
            s.spawn(move || {
                let _ = respond(stream, opened);
            });
        }
    });
    Ok(())
}
