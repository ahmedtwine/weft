mod address;
mod graph;
mod link;
mod otlp;
mod rows;
mod source;
mod spool;
mod trace;

use std::borrow::Cow;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::Instant;

use opentelemetry_proto::tonic::collector::logs::v1::ExportLogsServiceRequest;
use opentelemetry_proto::tonic::collector::trace::v1::ExportTraceServiceRequest;
use serde_json::{Value as Json, json};
use weft_dashboard::{Board, Reply};

use crate::graph::Graph;
pub use crate::link::Linker;
use crate::link::Resolved;
use crate::otlp::{Export, Format};
use crate::source::{Signal, Sources};
use crate::spool::Spool;
use crate::trace::Traces;

const TRACES: &str = "v1/traces";
const LOGS: &str = "v1/logs";
const BAD: &str = "400 Bad Request";
const UNSUPPORTED: &str = "415 Unsupported Media Type";
const SCHEMA: &str = r#"{
  "stream": {"time": "datetime", "service": "string", "kind": "string", "what": "string", "peer": "string", "code": "string", "status": "string", "duration_ms": "float", "trace_id": "string"},
  "traces": {"key": "string", "trace": "string", "root": "string", "span_id": "string", "parent": "string", "service": "string", "operation": "string", "kind": "string", "status": "string", "link": "string", "depth": "integer", "time": "datetime", "offset_ms": "float", "duration_ms": "float", "self_ms": "float", "skew_ms": "float", "attributes": "string", "events": "string", "symbol": "string", "source": "string", "via": "string", "chain": "string"},
  "sources": {"service": "string", "language": "string", "runtime": "string", "sdk": "string", "scopes": "string", "spans": "integer", "logs": "integer"},
  "edges": {"edge": "string", "from_service": "string", "from": "string", "to_service": "string", "to": "string", "calls": "integer", "errors": "integer", "duration_ms": "float"}
}"#;

#[derive(Default)]
struct State {
    graph: Graph,
    sources: Sources,
    traces: Traces,
}

pub struct Collector {
    state: Mutex<State>,
    linker: Option<Linker>,
    spool: Mutex<Option<Spool>>,
}

impl Collector {
    pub fn open(spool: PathBuf, linker: Option<Linker>) -> Result<(Self, usize), String> {
        let collector = Self::load(&spool, linker)?;
        *lock(&collector.0.spool) = Some(Spool::open(spool).map_err(|e| e.to_string())?);
        Ok(collector)
    }

    fn load(spool: &Path, linker: Option<Linker>) -> Result<(Self, usize), String> {
        let collector = Self {
            state: Mutex::default(),
            linker,
            spool: Mutex::default(),
        };
        let replayed = Spool::records(spool)
            .map_err(|e| format!("{}: {e}", spool.display()))?
            .iter()
            .filter(|(signal, format, body)| {
                Format::of_code(*format)
                    .is_some_and(|format| collector.replay(*signal, format, body))
            })
            .count();
        Ok((collector, replayed))
    }

    pub fn sweep(&self) {
        let mut state = lock(&self.state);
        let edges = state.graph.sweep(Instant::now());
        let sources = state.sources.take();
        let traces = state.traces.sweep();
        drop(state);
        publish([("edges", edges), ("sources", sources), ("traces", traces)]);
    }

    fn replay(&self, signal: u8, format: Format, body: &[u8]) -> bool {
        match signal {
            code if code == Signal::Spans as u8 => {
                self.ingest::<ExportTraceServiceRequest>(format, body)
            }
            code if code == Signal::Logs as u8 => {
                self.ingest::<ExportLogsServiceRequest>(format, body)
            }
            _ => false,
        }
    }

    fn ingest<T: Export>(&self, format: Format, body: &[u8]) -> bool {
        let Some(request) = format.decode::<T>(body) else {
            return false;
        };
        let resolved = self.observe(&request);
        publish([("stream", request.stream(&resolved))]);
        true
    }

    fn observe<T: Export>(&self, request: &T) -> Resolved {
        let resolved = request
            .trace()
            .zip(self.linker.as_ref())
            .map(|(trace, linker)| link::resolve(linker, trace))
            .unwrap_or_default();
        let mut state = lock(&self.state);
        for (resource, scope, count) in request.origins() {
            state.sources.record(resource, scope, T::SIGNAL, count);
        }
        if let Some(trace) = request.trace() {
            state.graph.observe(trace);
            state.traces.observe(trace, &resolved);
        }
        resolved
    }

    fn export<T: Export>(&self, kind: &str, body: &[u8]) -> Reply {
        let Some(format) = Format::of(kind) else {
            return reply(UNSUPPORTED, "text/plain", b"");
        };
        if !self.ingest::<T>(format, body) {
            return reply(BAD, "text/plain", b"");
        }
        if let Some(spool) = lock(&self.spool).as_mut() {
            let _ = spool.append(T::SIGNAL as u8, format as u8, body);
        }
        reply("200 OK", format.kind(), format.accepted())
    }
}

impl Board for Collector {
    const PAGE: &str = "collect.html";

    fn schema(&self) -> String {
        SCHEMA.to_string()
    }

    fn kept(&self) -> Vec<String> {
        let state = lock(&self.state);
        let tables = [
            ("edges", state.graph.snapshot()),
            ("sources", state.sources.snapshot()),
        ];
        drop(state);
        message(tables).into_iter().collect()
    }

    fn posted(&self, path: &str, kind: &str, body: &[u8]) -> Option<Reply> {
        match path {
            TRACES => Some(self.export::<ExportTraceServiceRequest>(kind, body)),
            LOGS => Some(self.export::<ExportLogsServiceRequest>(kind, body)),
            _ => None,
        }
    }
}

const fn reply(status: &'static str, kind: &'static str, body: &'static [u8]) -> Reply {
    Reply {
        status,
        kind,
        body: Cow::Borrowed(body),
    }
}

fn message<const N: usize>(tables: [(&str, Vec<Json>); N]) -> Option<String> {
    let tables: serde_json::Map<String, Json> = tables
        .into_iter()
        .filter(|(_, rows)| !rows.is_empty())
        .map(|(name, rows)| (name.to_string(), Json::Array(rows)))
        .collect();
    (!tables.is_empty()).then(|| json!(tables).to_string())
}

fn publish<const N: usize>(tables: [(&str, Vec<Json>); N]) {
    if let Some(rows) = message(tables) {
        weft_dashboard::say(rows);
    }
}

pub(crate) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::time::Instant;

    use serde_json::{Value as Json, json};
    use weft_dashboard::Board;

    use super::{Collector, Linker, lock};

    fn span(id: u8, parent: Option<u8>, kind: u8, attributes: &Json) -> Json {
        json!({
            "traceId": "ab".repeat(16),
            "spanId": format!("{id:02x}").repeat(8),
            "parentSpanId": parent.map(|parent| format!("{parent:02x}").repeat(8)).unwrap_or_default(),
            "name": "span",
            "kind": kind,
            "startTimeUnixNano": "1700000000000000000",
            "endTimeUnixNano": "1700000000010000000",
            "attributes": attributes.as_object().into_iter().flatten().map(|(key, value)| json!({ "key": key, "value": { "stringValue": value } })).collect::<Vec<_>>(),
        })
    }

    fn service(name: &str, spans: &[Json]) -> Json {
        json!({
            "resource": { "attributes": [{ "key": "service.name", "value": { "stringValue": name } }] },
            "scopeSpans": [{ "scope": { "name": "test" }, "spans": spans }],
        })
    }

    #[test]
    fn links_spans_to_code_and_joins_services() {
        let map = serde_json::from_value(json!({
            "files": [{ "path": "app/api/orders/[id]/route.ts", "home": "app", "package": "shop", "root": 0 }],
            "symbols": [{ "file": 0, "name": "route.ts", "line": 1 }, { "file": 0, "name": "GET", "line": 3 }],
            "sites": [],
            "declared": [[0, 3, 1]],
            "routes": [{ "method": "GET", "path": "/api/orders/[id]", "handler": 1 }],
        }))
        .unwrap_or_else(|e| panic!("{e}"));
        let linker = Linker::mapped(PathBuf::from("/srv/shop"), map);
        let (collector, _) = Collector::load(Path::new("/nonexistent/spool"), Some(linker))
            .unwrap_or_else(|e| panic!("{e}"));
        let body = json!({ "resourceSpans": [
            service("web", &[span(1, None, 3, &json!({
                "http.request.method": "GET",
                "url.full": "http://localhost:3000/api/orders/7",
                "server.address": "localhost",
                "code.stacktrace": "    at load (/srv/shop/app/api/orders/[id]/route.ts:4:2)",
            }))]),
            service("shop", &[span(2, Some(1), 2, &json!({
                "http.request.method": "GET",
                "http.route": "/api/orders/[id]",
            }))]),
        ]});
        let reply = collector
            .posted("v1/traces", "application/json", body.to_string().as_bytes())
            .unwrap_or_else(|| panic!("no reply"));
        assert_eq!(reply.status, "200 OK");
        let mut state = lock(&collector.state);
        let traces = state.traces.sweep();
        let linked = |service: &str| {
            traces
                .iter()
                .find(|row| row["service"] == service)
                .map(|row| {
                    (
                        row["symbol"].clone(),
                        row["source"].clone(),
                        row["via"].clone(),
                    )
                })
        };
        assert_eq!(
            linked("shop"),
            Some((
                json!("GET"),
                json!("app/api/orders/[id]/route.ts:3"),
                json!("route")
            ))
        );
        assert_eq!(
            linked("web"),
            Some((
                json!("GET"),
                json!("app/api/orders/[id]/route.ts:4"),
                json!("stack")
            ))
        );
        let edges = state.graph.sweep(Instant::now());
        assert_eq!(edges.len(), 1);
        assert_eq!(
            (
                &edges[0]["from_service"],
                &edges[0]["from"],
                &edges[0]["to"]
            ),
            (&json!("web"), &json!(""), &json!("GET /api/orders/[id]"))
        );
    }
}
