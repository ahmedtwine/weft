use std::collections::{BTreeSet, HashMap, HashSet};

use opentelemetry_proto::tonic::common::v1::InstrumentationScope;
use opentelemetry_proto::tonic::resource::v1::Resource;
use serde_json::{Value as Json, json};

use crate::rows::{self, Origin};

#[derive(Default)]
struct Source {
    language: String,
    runtime: String,
    sdk: String,
    scopes: BTreeSet<String>,
}

#[derive(Default)]
pub(crate) struct Sources {
    entries: HashMap<String, Source>,
    touched: HashSet<String>,
}

impl Sources {
    pub(crate) fn record(
        &mut self,
        resource: Option<&Resource>,
        scope: Option<&InstrumentationScope>,
    ) {
        let origin = Origin::new(resource, scope);
        let source = self.entries.entry(origin.service.to_owned()).or_default();
        let attributes = resource.map_or(&[][..], |resource| resource.attributes.as_slice());
        let read = |keys: &[&str]| {
            keys.iter()
                .filter_map(|key| rows::text(attributes, key))
                .collect::<Vec<_>>()
                .join(" ")
        };
        source.language = read(&["telemetry.sdk.language"]);
        source.runtime = read(&["process.runtime.name", "process.runtime.version"]);
        source.sdk = read(&["telemetry.sdk.name", "telemetry.sdk.version"]);
        if !origin.scope.is_empty() {
            source.scopes.insert(origin.scope.to_owned());
        }
        self.touched.insert(origin.service.to_owned());
    }

    pub(crate) fn take(&mut self) -> Vec<Json> {
        let touched = std::mem::take(&mut self.touched);
        self.rows(touched.iter())
    }

    pub(crate) fn snapshot(&self) -> Vec<Json> {
        self.rows(self.entries.keys())
    }

    fn rows<'a>(&self, services: impl Iterator<Item = &'a String>) -> Vec<Json> {
        services
            .filter_map(|service| {
                let source = self.entries.get(service)?;
                Some(json!({
                    "service": service,
                    "language": source.language,
                    "runtime": source.runtime,
                    "sdk": source.sdk,
                    "scopes": source.scopes.iter().map(String::as_str).collect::<Vec<_>>().join(", "),
                }))
            })
            .collect()
    }
}
