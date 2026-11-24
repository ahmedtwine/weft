mod bundle;
mod frame;
mod index;
mod linker;
mod location;
mod route;

use std::collections::HashMap;
use std::sync::Arc;

use opentelemetry_proto::tonic::collector::trace::v1::ExportTraceServiceRequest;
use opentelemetry_proto::tonic::common::v1::KeyValue;
use opentelemetry_proto::tonic::common::v1::any_value::Value;
use opentelemetry_proto::tonic::trace::v1::Span;
use opentelemetry_proto::tonic::trace::v1::span::SpanKind;

use crate::graph;
use crate::rows;

pub use crate::link::linker::Linker;

const ROUTE_KEYS: [&str; 2] = ["http.route", "next.route"];
const STACK_KEYS: [&str; 2] = ["code.stacktrace", "exception.stacktrace"];
const FILE_KEYS: [&str; 3] = ["code.file.path", "code.filepath", "log.file"];
const LINE_KEYS: [&str; 3] = ["code.line.number", "code.lineno", "log.line"];

pub(crate) type Resolved = HashMap<[u8; 8], Resolution>;

#[derive(Clone, Copy, Debug)]
pub(crate) struct Location<'a> {
    pub(crate) file: &'a str,
    pub(crate) line: Option<u32>,
}

pub(crate) struct Evidence<'a> {
    pub(crate) service: &'a str,
    pub(crate) serves: bool,
    pub(crate) method: Option<&'a str>,
    pub(crate) route: Option<&'a str>,
    pub(crate) target: Option<String>,
    pub(crate) stack: Option<&'a str>,
    pub(crate) code: Option<Location<'a>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Site {
    pub(crate) symbol: Arc<str>,
    pub(crate) source: Arc<str>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct Resolution {
    pub(crate) site: Option<Site>,
    pub(crate) via: &'static str,
    pub(crate) chain: Vec<Site>,
}

pub(crate) fn resolve(linker: &Linker, request: &ExportTraceServiceRequest) -> Resolved {
    rows::traced(request)
        .filter_map(|(origin, span)| {
            Some((id(span)?, linker.resolve(&evidence(origin.service, span))))
        })
        .collect()
}

pub(crate) fn id(span: &Span) -> Option<[u8; 8]> {
    <[u8; 8]>::try_from(span.span_id.as_slice()).ok()
}

fn evidence<'a>(service: &'a str, span: &'a Span) -> Evidence<'a> {
    Evidence {
        service,
        serves: matches!(
            SpanKind::try_from(span.kind),
            Ok(SpanKind::Server | SpanKind::Consumer)
        ),
        method: graph::method(span),
        route: first(&span.attributes, &ROUTE_KEYS),
        target: graph::target(span),
        stack: first(&span.attributes, &STACK_KEYS),
        code: location(&span.attributes),
    }
}

fn first<'a>(attributes: &'a [KeyValue], keys: &[&str]) -> Option<&'a str> {
    keys.iter().find_map(|key| rows::text(attributes, key))
}

pub(crate) fn location(attributes: &[KeyValue]) -> Option<Location<'_>> {
    Some(Location {
        file: first(attributes, &FILE_KEYS)?,
        line: LINE_KEYS.iter().find_map(|key| {
            attributes
                .iter()
                .find(|attribute| attribute.key == *key)
                .and_then(
                    |attribute| match attribute.value.as_ref()?.value.as_ref()? {
                        Value::IntValue(line) => u32::try_from(*line).ok(),
                        Value::StringValue(line) => line.parse().ok(),
                        _ => None,
                    },
                )
        }),
    })
}
