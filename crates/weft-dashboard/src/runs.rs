use std::time::{Duration, UNIX_EPOCH};

use opentelemetry::trace::Status;
use opentelemetry::{Array, Value};
use opentelemetry_sdk::error::OTelSdkResult;
use opentelemetry_sdk::trace::{BatchConfigBuilder, BatchSpanProcessor, SpanData, SpanExporter};
use serde_json::{Map, Value as Json, json};

use crate::Board;

pub const FILE: &str = "weft.file";
pub const STATUS: &str = "weft.status";
pub const TEST_CASES: &str = "weft.test_cases";
pub const STEPS: &str = "weft.steps";
pub const CALLS: &str = "weft.calls";
pub const CHECKS: &str = "weft.checks";
pub const DIAGRAM: &str = "weft.diagram";
pub const MACHINE: &str = "weft.machine";
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
        (DIAGRAM, TEXT),
        (MACHINE, TEXT),
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
        (INPUT, TEXT),
        (DIAGRAM, TEXT),
        (MACHINE, TEXT),
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

const SHOWN: Duration = Duration::from_millis(200);

#[derive(Debug)]
struct Exporter;

pub struct Runs<F>(pub F);

impl<F: Fn() + Sync> Board for Runs<F> {
    const PAGE: &str = "tests.html";

    fn schema(&self) -> String {
        schema()
    }

    fn opened(&self) {
        (self.0)();
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
    [FILE, TEST_CASE, CHECK, INPUT, DIAGRAM, MACHINE]
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
            row.remove(INPUT);
            row.remove(MACHINE);
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
        crate::say(
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
