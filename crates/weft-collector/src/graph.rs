use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt::Write as _;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use opentelemetry_proto::tonic::collector::trace::v1::ExportTraceServiceRequest;
use opentelemetry_proto::tonic::common::v1::KeyValue;
use opentelemetry_proto::tonic::trace::v1::Span;
use opentelemetry_proto::tonic::trace::v1::span::SpanKind;
use serde_json::{Value as Json, json};

use crate::address;
use crate::rows;

const LOCAL_WAIT: Duration = Duration::from_secs(30);
const REMOTE_WAIT: Duration = Duration::from_secs(3);
const KEEP: Duration = Duration::from_secs(120);
const DEPTH: usize = 4;
const METHOD_KEYS: [&str; 2] = ["http.request.method", "http.method"];
pub(crate) const PATH_KEYS: [&str; 5] = [
    "http.route",
    "url.path",
    "http.target",
    "url.full",
    "http.url",
];
const HOST_KEYS: [&str; 8] = [
    "peer.service",
    "server.address",
    "net.peer.name",
    "db.system.name",
    "db.system",
    "messaging.system",
    "rpc.system",
    "network.peer.address",
];
const LOCAL_HOSTS: [&str; 4] = ["localhost", "127.0.0.1", "::1", "0.0.0.0"];

type Id = [u8; 8];

#[derive(Clone, PartialEq, Eq, Hash)]
struct Node {
    service: Arc<str>,
    label: Arc<str>,
}

#[derive(Clone, PartialEq, Eq, Hash)]
struct Edge {
    source: Node,
    target: Node,
}

#[derive(Default)]
struct Stats {
    calls: i64,
    errors: i64,
    duration_ms: f64,
}

struct Call {
    node: Node,
    parent: Option<Id>,
    kind: SpanKind,
    host: Option<Arc<str>>,
    server: String,
    address: String,
    failed: bool,
    duration_ms: f64,
    arrived: Instant,
}

enum Step {
    Wait,
    Done(Option<Edge>),
}

#[derive(Default)]
pub(crate) struct Graph {
    calls: HashMap<Id, Call>,
    children: HashMap<Id, Vec<Id>>,
    arrivals: VecDeque<Id>,
    pending: Vec<Id>,
    edges: HashMap<Edge, Stats>,
    served: HashMap<String, Arc<str>>,
}

impl Graph {
    pub(crate) fn observe(&mut self, request: &ExportTraceServiceRequest) {
        let now = Instant::now();
        for (origin, span) in rows::traced(request) {
            self.insert(origin.service, span, now);
        }
    }

    fn insert(&mut self, service: &str, span: &Span, now: Instant) {
        let Ok(id) = Id::try_from(span.span_id.as_slice()) else {
            return;
        };
        let parent = Id::try_from(span.parent_span_id.as_slice()).ok();
        let kind = SpanKind::try_from(span.kind).unwrap_or(SpanKind::Unspecified);
        let call = Call {
            node: Node {
                service: Arc::from(service),
                label: label(span),
            },
            parent,
            kind,
            host: host(&span.attributes),
            server: rows::server(&span.attributes)
                .unwrap_or_default()
                .into_owned(),
            address: rows::peer(&span.attributes),
            failed: rows::status(span) == "error"
                || rows::http_status(&span.attributes).is_some_and(|code| code >= 500),
            duration_ms: rows::duration_ms(span),
            arrived: now,
        };
        if let Some(parent) = parent {
            self.children.entry(parent).or_default().push(id);
        }
        match kind {
            SpanKind::Server | SpanKind::Consumer if !call.address.is_empty() => {
                for key in [&call.server, &call.address] {
                    self.served
                        .insert(key.clone(), Arc::clone(&call.node.service));
                }
            }
            SpanKind::Client | SpanKind::Producer => self.pending.push(id),
            _ => {}
        }
        self.arrivals.push_back(id);
        self.calls.insert(id, call);
    }

    pub(crate) fn sweep(&mut self, now: Instant) -> Vec<Json> {
        let mut changed = HashSet::new();
        let mut waiting = Vec::new();
        for id in std::mem::take(&mut self.pending) {
            let Some(call) = self.calls.get(&id) else {
                continue;
            };
            match self.outbound(id, call, now.duration_since(call.arrived)) {
                Step::Wait => waiting.push(id),
                Step::Done(None) => {}
                Step::Done(Some(edge)) => {
                    let stats = self.edges.entry(edge.clone()).or_default();
                    stats.calls += 1;
                    stats.errors += i64::from(call.failed);
                    stats.duration_ms += call.duration_ms;
                    changed.insert(edge);
                }
            }
        }
        self.pending = waiting;
        self.evict(now);
        changed.iter().filter_map(|edge| self.row(edge)).collect()
    }

    pub(crate) fn snapshot(&self) -> Vec<Json> {
        self.edges
            .keys()
            .filter_map(|edge| self.row(edge))
            .collect()
    }

    fn row(&self, edge: &Edge) -> Option<Json> {
        let stats = self.edges.get(edge)?;
        Some(json!({
            "edge": format!(
                "{}|{}>{}|{}",
                edge.source.service, edge.source.label, edge.target.service, edge.target.label
            ),
            "from_service": &*edge.source.service,
            "from": &*edge.source.label,
            "to_service": &*edge.target.service,
            "to": &*edge.target.label,
            "calls": stats.calls,
            "errors": stats.errors,
            "duration_ms": stats.duration_ms,
        }))
    }

    fn outbound(&self, id: Id, call: &Call, age: Duration) -> Step {
        let children: Vec<&Call> = self
            .children
            .get(&id)
            .into_iter()
            .flatten()
            .filter_map(|child| self.calls.get(child))
            .collect();
        if children.iter().any(|child| {
            child.node.service == call.node.service
                && matches!(child.kind, SpanKind::Client | SpanKind::Producer)
        }) {
            return Step::Done(None);
        }
        let remote = children
            .iter()
            .find(|child| child.node.service != call.node.service)
            .map(|child| child.node.clone());
        let wait = match call.host.as_deref() {
            Some(host) if !LOCAL_HOSTS.contains(&host) => REMOTE_WAIT,
            _ => LOCAL_WAIT,
        };
        let addressed = self
            .served
            .get(&call.address)
            .or_else(|| self.served.get(&call.server))
            .filter(|service| *service != &call.node.service);
        let target = match (remote, addressed, &call.host) {
            (Some(node), _, _) => node,
            (None, Some(service), _) if age >= wait => Node {
                service: Arc::clone(service),
                label: Arc::clone(&call.node.label),
            },
            (None, None, Some(host)) if age >= wait => Node {
                service: Arc::clone(host),
                label: Arc::clone(&call.node.label),
            },
            (None, None, None) if age >= wait => return Step::Done(None),
            _ => return Step::Wait,
        };
        match self.entry(call, age) {
            Some(source) => Step::Done(Some(Edge { source, target })),
            None => Step::Wait,
        }
    }

    fn entry(&self, call: &Call, age: Duration) -> Option<Node> {
        let mut top = call;
        while let Some(parent) = top.parent {
            match self.calls.get(&parent) {
                Some(next) if next.node.service == call.node.service => top = next,
                Some(_) => break,
                None if matches!(top.kind, SpanKind::Server | SpanKind::Consumer)
                    || age >= LOCAL_WAIT =>
                {
                    break;
                }
                None => return None,
            }
        }
        Some(match std::ptr::eq(top, call) {
            true => Node {
                service: Arc::clone(&call.node.service),
                label: Arc::from(""),
            },
            false => top.node.clone(),
        })
    }

    fn evict(&mut self, now: Instant) {
        while let Some(id) = self.arrivals.front().copied() {
            if self
                .calls
                .get(&id)
                .is_some_and(|call| now.duration_since(call.arrived) < KEEP)
            {
                break;
            }
            self.arrivals.pop_front();
            self.calls.remove(&id);
            self.children.remove(&id);
        }
    }
}

pub(crate) fn path(span: &Span) -> Option<String> {
    PATH_KEYS
        .iter()
        .find_map(|key| rows::text(&span.attributes, key))
        .map(route)
}

pub(crate) fn target(span: &Span) -> Option<String> {
    PATH_KEYS[1..]
        .iter()
        .find_map(|key| rows::text(&span.attributes, key))
        .and_then(address::path)
}

pub(crate) fn method(span: &Span) -> Option<&str> {
    METHOD_KEYS
        .iter()
        .find_map(|key| rows::text(&span.attributes, key))
}

pub(crate) fn label(span: &Span) -> Arc<str> {
    Arc::from(match (method(span), path(span)) {
        (Some(method), Some(path)) => format!("{method} {path}"),
        (None, Some(path)) => path,
        (_, None) => span.name.clone(),
    })
}

fn host(attributes: &[KeyValue]) -> Option<Arc<str>> {
    HOST_KEYS
        .iter()
        .find_map(|key| rows::text(attributes, key))
        .map(Arc::from)
}

fn route(raw: &str) -> String {
    let segments = address::segments(raw);
    if segments
        .last()
        .is_some_and(|segment| Path::new(segment).extension().is_some())
    {
        return segments
            .first()
            .filter(|_| segments.len() > 1)
            .map_or_else(|| String::from("/*"), |first| format!("/{first}/*"));
    }
    let mut path = segments
        .iter()
        .take(DEPTH)
        .fold(String::new(), |mut path, segment| {
            let _ = write!(path, "/{}", if dynamic(segment) { ":id" } else { segment });
            path
        });
    if segments.len() > DEPTH {
        path.push_str("/*");
    }
    if path.is_empty() {
        path.push('/');
    }
    path
}

fn dynamic(segment: &str) -> bool {
    let digits = segment.bytes().filter(u8::is_ascii_digit).count();
    digits == segment.len() || (segment.len() >= 16 && digits > 0)
}
