use opentelemetry_proto::tonic::collector::logs::v1::ExportLogsServiceRequest;
use opentelemetry_proto::tonic::collector::trace::v1::ExportTraceServiceRequest;
use opentelemetry_proto::tonic::common::v1::InstrumentationScope;
use opentelemetry_proto::tonic::resource::v1::Resource;
use prost::Message;
use serde::de::DeserializeOwned;
use serde_json::Value as Json;

use crate::link::Resolved;
use crate::rows;

#[derive(Clone, Copy)]
#[repr(u8)]
pub(crate) enum Signal {
    Spans,
    Logs,
}

#[derive(Clone, Copy)]
#[repr(u8)]
pub(crate) enum Format {
    Json,
    Protobuf,
}

impl Format {
    pub(crate) fn of(kind: &str) -> Option<Self> {
        match kind.split(';').next().unwrap_or_default().trim() {
            "application/json" => Some(Self::Json),
            "application/x-protobuf" => Some(Self::Protobuf),
            _ => None,
        }
    }

    pub(crate) const fn of_code(code: u8) -> Option<Self> {
        match code {
            0 => Some(Self::Json),
            1 => Some(Self::Protobuf),
            _ => None,
        }
    }

    pub(crate) const fn kind(self) -> &'static str {
        match self {
            Self::Json => "application/json",
            Self::Protobuf => "application/x-protobuf",
        }
    }

    pub(crate) const fn accepted(self) -> &'static [u8] {
        match self {
            Self::Json => b"{}",
            Self::Protobuf => b"",
        }
    }

    pub(crate) fn decode<T: Message + Default + DeserializeOwned>(self, body: &[u8]) -> Option<T> {
        match self {
            Self::Json => serde_json::from_slice(body).ok(),
            Self::Protobuf => T::decode(body).ok(),
        }
    }
}

pub(crate) type Origins<'a> = Vec<(Option<&'a Resource>, Option<&'a InstrumentationScope>)>;

pub(crate) trait Export: Message + Default + DeserializeOwned {
    const SIGNAL: Signal;
    fn origins(&self) -> Origins<'_>;
    fn stream(&self, resolved: &Resolved) -> Vec<Json>;
    fn trace(&self) -> Option<&ExportTraceServiceRequest> {
        None
    }
}

impl Export for ExportTraceServiceRequest {
    const SIGNAL: Signal = Signal::Spans;
    fn origins(&self) -> Origins<'_> {
        self.resource_spans
            .iter()
            .flat_map(|resource| {
                resource
                    .scope_spans
                    .iter()
                    .map(|scope| (resource.resource.as_ref(), scope.scope.as_ref()))
            })
            .collect()
    }
    fn stream(&self, resolved: &Resolved) -> Vec<Json> {
        rows::span_stream(self, resolved)
    }
    fn trace(&self) -> Option<&ExportTraceServiceRequest> {
        Some(self)
    }
}

impl Export for ExportLogsServiceRequest {
    const SIGNAL: Signal = Signal::Logs;
    fn origins(&self) -> Origins<'_> {
        self.resource_logs
            .iter()
            .flat_map(|resource| {
                resource
                    .scope_logs
                    .iter()
                    .map(|scope| (resource.resource.as_ref(), scope.scope.as_ref()))
            })
            .collect()
    }
    fn stream(&self, _resolved: &Resolved) -> Vec<Json> {
        rows::log_stream(self)
    }
}
