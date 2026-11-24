use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};

use opentelemetry_proto::tonic::collector::trace::v1::ExportTraceServiceRequest;
use opentelemetry_proto::tonic::trace::v1::Span;
use opentelemetry_proto::tonic::trace::v1::span::SpanKind;
use serde_json::{Value as Json, json};

use crate::graph;
use crate::link::{self, Resolution, Resolved, Site};
use crate::rows;

const WAIT: Duration = Duration::from_secs(30);
const KEEP: Duration = Duration::from_secs(120);
const SLACK: u64 = 10_000_000;
const MAX_SKEW: i64 = 60_000_000_000;
const REMOTE_FLAGS: u32 = 0x300;
const HOPS: usize = 4096;
const ROOT: &str = "root";
const CHILD: &str = "child";
const REMOTE: &str = "remote child";
const MISSING: &str = "missing parent";
const LINKED: &str = "span link";
const MATCHED: &str = "time match";

type TraceId = [u8; 16];
type SpanId = [u8; 8];
type Key = (TraceId, SpanId);

struct Record {
    parent: Option<SpanId>,
    remote: bool,
    links: Vec<Key>,
    service: Arc<str>,
    operation: Arc<str>,
    kind: SpanKind,
    status: &'static str,
    target: Option<String>,
    attributes: String,
    events: String,
    site: Option<Site>,
    via: &'static str,
    chain: String,
    start: u64,
    end: u64,
    arrived: Instant,
}

struct Row {
    key: Key,
    trace: TraceId,
    root: Arc<str>,
    parent: Option<SpanId>,
    link: &'static str,
    depth: i64,
    start: i64,
    end: i64,
    skew: i64,
    exclusive: i64,
    origin: i64,
}

#[derive(Default)]
pub(crate) struct Traces {
    spans: HashMap<Key, Record>,
    members: HashMap<TraceId, Vec<SpanId>>,
    servers: HashMap<(Arc<str>, String), Vec<Key>>,
    adopted: HashMap<Key, (Key, &'static str)>,
    host: HashMap<TraceId, TraceId>,
    orphans: Vec<Key>,
    arrivals: VecDeque<Key>,
    touched: HashSet<TraceId>,
}

impl Traces {
    pub(crate) fn observe(&mut self, request: &ExportTraceServiceRequest, resolved: &Resolved) {
        let now = Instant::now();
        for (origin, span) in rows::traced(request) {
            let resolution = link::id(span).and_then(|id| resolved.get(&id));
            self.insert(origin.service, span, resolution, now);
        }
    }

    fn insert(
        &mut self,
        service: &str,
        span: &Span,
        resolution: Option<&Resolution>,
        now: Instant,
    ) {
        let (Ok(trace), Ok(id)) = (
            TraceId::try_from(span.trace_id.as_slice()),
            SpanId::try_from(span.span_id.as_slice()),
        ) else {
            return;
        };
        let key = (trace, id);
        if self.spans.contains_key(&key) {
            return;
        }
        let kind = SpanKind::try_from(span.kind).unwrap_or(SpanKind::Unspecified);
        let record = Record {
            parent: SpanId::try_from(span.parent_span_id.as_slice()).ok(),
            remote: span.flags & REMOTE_FLAGS == REMOTE_FLAGS,
            links: span
                .links
                .iter()
                .filter_map(|link| {
                    Some((
                        TraceId::try_from(link.trace_id.as_slice()).ok()?,
                        SpanId::try_from(link.span_id.as_slice()).ok()?,
                    ))
                })
                .collect(),
            service: Arc::from(service),
            operation: graph::label(span),
            kind,
            status: rows::status(span),
            target: graph::target(span),
            attributes: rows::attributes(&span.attributes),
            events: rows::events(span),
            site: resolution.and_then(|resolution| resolution.site.clone()),
            via: resolution.map_or("", |resolution| resolution.via),
            chain: resolution
                .map(|resolution| {
                    resolution
                        .chain
                        .iter()
                        .map(|site| format!("{} {}", site.symbol, site.source))
                        .collect::<Vec<_>>()
                        .join("\n")
                })
                .unwrap_or_default(),
            start: span.start_time_unix_nano,
            end: span.end_time_unix_nano.max(span.start_time_unix_nano),
            arrived: now,
        };
        if kind == SpanKind::Server
            && let Some(target) = &record.target
        {
            self.servers
                .entry((Arc::clone(&record.service), target.clone()))
                .or_default()
                .push(key);
        }
        if record.parent.is_some() || !entry(kind) || !record.links.is_empty() {
            self.orphans.push(key);
        }
        self.spans.insert(key, record);
        self.members.entry(trace).or_default().push(id);
        self.arrivals.push_back(key);
        self.touched.insert(trace);
    }

    pub(crate) fn sweep(&mut self) -> Vec<Json> {
        let now = Instant::now();
        self.adopt(now);
        let hosts: HashSet<TraceId> = std::mem::take(&mut self.touched)
            .into_iter()
            .map(|trace| self.top(trace))
            .collect();
        let rows = self.rows(&hosts);
        self.evict(now);
        rows
    }

    fn adopt(&mut self, now: Instant) {
        let mut waiting = Vec::new();
        for key in std::mem::take(&mut self.orphans) {
            let Some(record) = self.spans.get(&key) else {
                continue;
            };
            if record
                .parent
                .is_some_and(|parent| self.spans.contains_key(&(key.0, parent)))
            {
                continue;
            }
            let young = now.duration_since(record.arrived) < WAIT;
            if record.parent.is_some() && young {
                waiting.push(key);
                continue;
            }
            match self.candidate(key, record) {
                Some((parent, how)) => {
                    self.adopted.insert(key, (parent, how));
                    if parent.0 != key.0 {
                        self.host.insert(key.0, parent.0);
                    }
                    self.touched.insert(key.0);
                }
                None if young => waiting.push(key),
                None => {}
            }
        }
        self.orphans = waiting;
    }

    fn candidate(&self, key: Key, record: &Record) -> Option<(Key, &'static str)> {
        record
            .links
            .iter()
            .find(|link| self.spans.contains_key(link) && !self.reaches(**link, key))
            .map(|link| (*link, LINKED))
            .or_else(|| self.matched(key, record).map(|parent| (parent, MATCHED)))
    }

    fn matched(&self, key: Key, record: &Record) -> Option<Key> {
        if entry(record.kind) {
            return None;
        }
        let target = record.target.clone()?;
        let found: Vec<(Key, u64)> = self
            .servers
            .get(&(Arc::clone(&record.service), target))?
            .iter()
            .filter(|candidate| candidate.0 != key.0)
            .filter_map(|candidate| self.spans.get(candidate).map(|span| (*candidate, span)))
            .filter(|(_, span)| {
                (span.start..=span.end).contains(&record.start) && record.end <= span.end + SLACK
            })
            .filter(|(candidate, _)| !self.reaches(*candidate, key))
            .map(|(candidate, span)| (candidate, span.end - span.start))
            .collect();
        let trace = found.first()?.0.0;
        if found.iter().any(|(candidate, _)| candidate.0 != trace) {
            return None;
        }
        found
            .into_iter()
            .min_by_key(|(_, duration)| *duration)
            .map(|(candidate, _)| candidate)
    }

    fn up(&self, key: Key) -> Option<(Key, &'static str)> {
        let record = self.spans.get(&key)?;
        record
            .parent
            .and_then(|parent| {
                let above = self.spans.get(&(key.0, parent))?;
                let how = if record.remote || above.service != record.service {
                    REMOTE
                } else {
                    CHILD
                };
                Some(((key.0, parent), how))
            })
            .or_else(|| self.adopted.get(&key).copied())
    }

    fn reaches(&self, from: Key, to: Key) -> bool {
        let mut at = from;
        for _ in 0..HOPS {
            if at == to {
                return true;
            }
            match self.up(at) {
                Some((parent, _)) => at = parent,
                None => return false,
            }
        }
        true
    }

    fn top(&self, trace: TraceId) -> TraceId {
        let mut at = trace;
        for _ in 0..HOPS {
            match self.host.get(&at) {
                Some(host) if *host != trace => at = *host,
                _ => break,
            }
        }
        at
    }

    fn group(&self, host: TraceId, guests: &HashMap<TraceId, Vec<TraceId>>) -> Vec<Key> {
        let mut seen = HashSet::new();
        let mut stack = vec![host];
        let mut keys = Vec::new();
        while let Some(trace) = stack.pop() {
            if !seen.insert(trace) {
                continue;
            }
            keys.extend(
                self.members
                    .get(&trace)
                    .into_iter()
                    .flatten()
                    .map(|id| (trace, *id)),
            );
            stack.extend(guests.get(&trace).into_iter().flatten().copied());
        }
        keys
    }

    fn assemble(&self, host: TraceId, keys: &[Key]) -> Vec<Row> {
        let present: HashSet<Key> = keys.iter().copied().collect();
        let mut children: HashMap<Key, Vec<(Key, &'static str)>> = HashMap::new();
        let mut roots = Vec::new();
        for key in keys {
            match self.up(*key).filter(|(parent, _)| present.contains(parent)) {
                Some((parent, how)) => children.entry(parent).or_default().push((*key, how)),
                None => roots.push(*key),
            }
        }
        let start = |key: &Key| self.spans.get(key).map_or(0, |record| record.start);
        roots.sort_by_key(start);
        for list in children.values_mut() {
            list.sort_by_key(|(key, _)| start(key));
        }
        let origin = roots.first().map_or(0, |key| nanos(start(key)));
        let root = roots
            .first()
            .and_then(|key| self.spans.get(key))
            .map_or_else(
                || Arc::from(""),
                |record| Arc::from(format!("{} {}", record.service, record.operation)),
            );
        let mut rows = Vec::with_capacity(keys.len());
        let mut stack: Vec<(Key, Option<SpanId>, &'static str, i64, i64)> = roots
            .iter()
            .rev()
            .map(|key| {
                (
                    *key,
                    None,
                    self.spans.get(key).map_or(ROOT, |record| {
                        if record.parent.is_some() {
                            MISSING
                        } else {
                            ROOT
                        }
                    }),
                    0,
                    0,
                )
            })
            .collect();
        while let Some((key, parent, link, depth, shift)) = stack.pop() {
            let Some(record) = self.spans.get(&key) else {
                continue;
            };
            let (begin, end) = (nanos(record.start) + shift, nanos(record.end) + shift);
            for (child, how) in children.get(&key).into_iter().flatten().rev() {
                let delta = self.spans.get(child).map_or(0, |inner| {
                    if record.kind == SpanKind::Client
                        && inner.kind == SpanKind::Server
                        && inner.service != record.service
                    {
                        skew(
                            (begin, end),
                            (nanos(inner.start) + shift, nanos(inner.end) + shift),
                        )
                    } else {
                        0
                    }
                });
                stack.push((*child, Some(key.1), how, depth + 1, shift + delta));
            }
            rows.push(Row {
                key,
                trace: host,
                root: Arc::clone(&root),
                parent,
                link,
                depth,
                start: begin,
                end,
                skew: shift,
                exclusive: 0,
                origin,
            });
        }
        let mut inner: HashMap<SpanId, Vec<(i64, i64)>> = HashMap::new();
        for row in &rows {
            if let Some(parent) = row.parent {
                inner.entry(parent).or_default().push((row.start, row.end));
            }
        }
        for row in &mut rows {
            row.exclusive = exclusive(
                (row.start, row.end),
                inner.remove(&row.key.1).unwrap_or_default(),
            );
        }
        rows
    }

    fn rows(&self, hosts: &HashSet<TraceId>) -> Vec<Json> {
        let mut guests: HashMap<TraceId, Vec<TraceId>> = HashMap::new();
        for (guest, host) in &self.host {
            guests.entry(*host).or_default().push(*guest);
        }
        let millis = |value: i64| value as f64 / 1e6;
        hosts
            .iter()
            .flat_map(|host| self.assemble(*host, &self.group(*host, &guests)))
            .filter_map(|row| {
                let record = self.spans.get(&row.key)?;
                Some(json!({
                    "key": format!("{}{}", rows::hex(&row.key.0), rows::hex(&row.key.1)),
                    "trace": rows::hex(&row.trace),
                    "root": &*row.root,
                    "span_id": rows::hex(&row.key.1),
                    "parent": row.parent.map(|parent| rows::hex(&parent)).unwrap_or_default(),
                    "service": &*record.service,
                    "operation": &*record.operation,
                    "kind": rows::kind(record.kind),
                    "status": record.status,
                    "link": row.link,
                    "depth": row.depth,
                    "time": rows::millis(record.start),
                    "offset_ms": millis(row.start - row.origin),
                    "duration_ms": millis(row.end - row.start),
                    "self_ms": millis(row.exclusive),
                    "skew_ms": millis(row.skew),
                    "attributes": record.attributes,
                    "events": record.events,
                    "symbol": record.site.as_ref().map_or("", |site| &*site.symbol),
                    "source": record.site.as_ref().map_or("", |site| &*site.source),
                    "via": record.via,
                    "chain": record.chain,
                }))
            })
            .collect()
    }

    fn evict(&mut self, now: Instant) {
        while let Some(key) = self.arrivals.front().copied() {
            if self
                .spans
                .get(&key)
                .is_some_and(|record| now.duration_since(record.arrived) < KEEP)
            {
                break;
            }
            self.arrivals.pop_front();
            if let Some(record) = self.spans.remove(&key)
                && let Some(target) = record.target
            {
                let server = (record.service, target);
                if let Some(list) = self.servers.get_mut(&server) {
                    list.retain(|candidate| *candidate != key);
                    if list.is_empty() {
                        self.servers.remove(&server);
                    }
                }
            }
            self.adopted.remove(&key);
            if let Some(ids) = self.members.get_mut(&key.0) {
                ids.retain(|id| *id != key.1);
                if ids.is_empty() {
                    self.members.remove(&key.0);
                    self.host.remove(&key.0);
                }
            }
        }
    }
}

const fn entry(kind: SpanKind) -> bool {
    matches!(kind, SpanKind::Server | SpanKind::Consumer)
}

fn nanos(value: u64) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

fn skew(parent: (i64, i64), child: (i64, i64)) -> i64 {
    let ((parent_start, parent_end), (child_start, child_end)) = (parent, child);
    let (outer, inner) = (parent_end - parent_start, child_end - child_start);
    let delta = if child_start >= parent_start && child_end <= parent_end {
        0
    } else if inner <= outer {
        parent_start + (outer - inner) / 2 - child_start
    } else if child_start < parent_start {
        parent_start - child_start
    } else {
        0
    };
    if delta.abs() > MAX_SKEW { 0 } else { delta }
}

fn exclusive(span: (i64, i64), mut inner: Vec<(i64, i64)>) -> i64 {
    inner.sort_unstable();
    let (mut covered, mut reach) = (0, span.0);
    for (from, to) in inner {
        let (from, to) = (from.max(reach), to.min(span.1));
        if to > from {
            covered += to - from;
            reach = to;
        }
    }
    (span.1 - span.0 - covered).max(0)
}
