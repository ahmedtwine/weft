use std::borrow::Cow;
use std::fmt::Write as _;
use std::time::Duration;

use opentelemetry_proto::tonic::collector::logs::v1::ExportLogsServiceRequest;
use opentelemetry_proto::tonic::collector::trace::v1::ExportTraceServiceRequest;
use opentelemetry_proto::tonic::common::v1::any_value::Value;
use opentelemetry_proto::tonic::common::v1::{AnyValue, InstrumentationScope, KeyValue};
use opentelemetry_proto::tonic::logs::v1::{LogRecord, SeverityNumber};
use opentelemetry_proto::tonic::resource::v1::Resource;
use opentelemetry_proto::tonic::trace::v1::Span;
use opentelemetry_proto::tonic::trace::v1::span::SpanKind;
use opentelemetry_proto::tonic::trace::v1::status::StatusCode;
use serde_json::{Map, Value as Json, json};

use crate::address;
use crate::graph;
use crate::link::{self, Resolved};

const HOST_KEYS: [&str; 2] = ["server.address", "net.peer.name"];
const PORT_KEYS: [&str; 2] = ["server.port", "net.peer.port"];
const STATUS_KEYS: [&str; 2] = ["http.response.status_code", "http.status_code"];

pub(crate) struct Origin<'a> {
    pub(crate) service: &'a str,
    pub(crate) scope: &'a str,
}

impl<'a> Origin<'a> {
    pub(crate) fn new(
        resource: Option<&'a Resource>,
        scope: Option<&'a InstrumentationScope>,
    ) -> Self {
        Self {
            service: resource
                .and_then(|resource| text(&resource.attributes, "service.name"))
                .unwrap_or("unknown_service"),
            scope: scope.map_or("", |scope| scope.name.as_str()),
        }
    }
}

pub(crate) fn traced(
    request: &ExportTraceServiceRequest,
) -> impl Iterator<Item = (Origin<'_>, &Span)> {
    request.resource_spans.iter().flat_map(|resource| {
        resource.scope_spans.iter().flat_map(move |scope| {
            scope.spans.iter().map(move |span| {
                (
                    Origin::new(resource.resource.as_ref(), scope.scope.as_ref()),
                    span,
                )
            })
        })
    })
}

fn logged(request: &ExportLogsServiceRequest) -> impl Iterator<Item = (Origin<'_>, &LogRecord)> {
    request.resource_logs.iter().flat_map(|resource| {
        resource.scope_logs.iter().flat_map(move |scope| {
            scope.log_records.iter().map(move |record| {
                (
                    Origin::new(resource.resource.as_ref(), scope.scope.as_ref()),
                    record,
                )
            })
        })
    })
}

pub(crate) fn span_stream(request: &ExportTraceServiceRequest, resolved: &Resolved) -> Vec<Json> {
    traced(request)
        .map(|(origin, span)| {
            json!({
                "time": millis(span.start_time_unix_nano),
                "service": origin.service,
                "kind": match SpanKind::try_from(span.kind).unwrap_or(SpanKind::Unspecified) {
                    SpanKind::Server | SpanKind::Consumer => "in",
                    SpanKind::Client | SpanKind::Producer => "out",
                    _ => "work",
                },
                "what": &*graph::label(span),
                "peer": peer(&span.attributes),
                "code": link::id(span)
                    .and_then(|id| resolved.get(&id))
                    .and_then(|resolution| resolution.site.as_ref())
                    .map(|site| format!("{} {}", site.symbol, site.source))
                    .unwrap_or_default(),
                "status": http_status(&span.attributes)
                    .map_or_else(|| status(span).to_owned(), |code| code.to_string()),
                "duration_ms": duration_ms(span),
                "trace_id": hex(&span.trace_id),
            })
        })
        .collect()
}

pub(crate) fn log_stream(request: &ExportLogsServiceRequest) -> Vec<Json> {
    logged(request)
        .map(|(origin, record)| {
            json!({
                "time": millis(match record.time_unix_nano {
                    0 => record.observed_time_unix_nano,
                    nanos => nanos,
                }),
                "service": origin.service,
                "kind": "log",
                "what": record.body.as_ref().map(display).unwrap_or_default(),
                "peer": "",
                "code": link::location(&record.attributes)
                    .map(|location| match location.line {
                        Some(line) => format!("{}:{line}", location.file),
                        None => location.file.to_owned(),
                    })
                    .unwrap_or_default(),
                "status": severity(record),
                "duration_ms": null,
                "trace_id": hex(&record.trace_id),
            })
        })
        .collect()
}

pub(crate) fn millis(nanos: u64) -> u64 {
    nanos / 1_000_000
}

fn attribute<'a>(attributes: &'a [KeyValue], key: &str) -> Option<&'a AnyValue> {
    attributes
        .iter()
        .find(|attribute| attribute.key == key)
        .and_then(|attribute| attribute.value.as_ref())
}

pub(crate) fn text<'a>(attributes: &'a [KeyValue], key: &str) -> Option<&'a str> {
    attribute(attributes, key).and_then(|value| match &value.value {
        Some(Value::StringValue(text)) => Some(text.as_str()),
        _ => None,
    })
}

pub(crate) fn duration_ms(span: &Span) -> f64 {
    Duration::from_nanos(
        span.end_time_unix_nano
            .saturating_sub(span.start_time_unix_nano),
    )
    .as_secs_f64()
        * 1000.0
}

pub(crate) fn server(attributes: &[KeyValue]) -> Option<Cow<'_, str>> {
    HOST_KEYS
        .iter()
        .find_map(|key| attribute(attributes, key))
        .map(display)
}

pub(crate) fn peer(attributes: &[KeyValue]) -> String {
    let port = PORT_KEYS
        .iter()
        .find_map(|key| attribute(attributes, key))
        .map(display);
    server(attributes).map_or_else(String::new, |host| {
        address::endpoint(&host, port.as_deref())
    })
}

pub(crate) const fn kind(kind: SpanKind) -> &'static str {
    match kind {
        SpanKind::Server => "server",
        SpanKind::Client => "client",
        SpanKind::Producer => "producer",
        SpanKind::Consumer => "consumer",
        SpanKind::Internal => "internal",
        SpanKind::Unspecified => "unspecified",
    }
}

pub(crate) fn status(span: &Span) -> &'static str {
    match span
        .status
        .as_ref()
        .map(|status| StatusCode::try_from(status.code))
    {
        Some(Ok(StatusCode::Ok)) => "ok",
        Some(Ok(StatusCode::Error)) => "error",
        _ => "",
    }
}

pub(crate) fn http_status(attributes: &[KeyValue]) -> Option<i64> {
    STATUS_KEYS
        .iter()
        .find_map(|key| attribute(attributes, key))
        .and_then(|value| match &value.value {
            Some(Value::IntValue(code)) => Some(*code),
            Some(Value::StringValue(code)) => code.parse().ok(),
            _ => None,
        })
}

fn severity(record: &LogRecord) -> &str {
    match record.severity_text.as_str() {
        "" => SeverityNumber::try_from(record.severity_number).map_or("", |number| {
            number.as_str_name().trim_start_matches("SEVERITY_NUMBER_")
        }),
        text => text,
    }
}

fn display(value: &AnyValue) -> Cow<'_, str> {
    match &value.value {
        Some(Value::StringValue(text)) => Cow::Borrowed(text),
        _ => Cow::Owned(shown(value).to_string()),
    }
}

fn shown(value: &AnyValue) -> Json {
    match &value.value {
        Some(Value::StringValue(text)) => Json::from(text.as_str()),
        Some(Value::BoolValue(flag)) => Json::from(*flag),
        Some(Value::IntValue(number)) => Json::from(*number),
        Some(Value::DoubleValue(number)) => Json::from(*number),
        Some(Value::ArrayValue(array)) => array.values.iter().map(shown).collect(),
        Some(Value::KvlistValue(list)) => Json::Object(object(&list.values)),
        Some(Value::BytesValue(bytes)) => Json::from(hex(bytes)),
        Some(Value::StringValueStrindex(_)) | None => Json::Null,
    }
}

fn object(attributes: &[KeyValue]) -> Map<String, Json> {
    attributes
        .iter()
        .map(|attribute| {
            (
                attribute.key.clone(),
                attribute.value.as_ref().map_or(Json::Null, shown),
            )
        })
        .collect()
}

pub(crate) fn attributes(attributes: &[KeyValue]) -> String {
    Json::Object(object(attributes)).to_string()
}

pub(crate) fn events(span: &Span) -> String {
    span.events
        .iter()
        .map(|event| {
            let mut entry = object(&event.attributes);
            entry.insert(String::from("event"), Json::from(event.name.as_str()));
            entry.insert(
                String::from("at_ms"),
                Json::from(
                    Duration::from_nanos(
                        event
                            .time_unix_nano
                            .saturating_sub(span.start_time_unix_nano),
                    )
                    .as_secs_f64()
                        * 1000.0,
                ),
            );
            Json::Object(entry)
        })
        .collect::<Json>()
        .to_string()
}

pub(crate) fn hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut out, byte| {
            let _ = write!(out, "{byte:02x}");
            out
        })
}
