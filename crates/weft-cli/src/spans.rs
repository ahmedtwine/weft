use std::collections::BTreeMap;
use std::env;
use std::mem;
use std::sync::OnceLock;
use std::time::{Duration, SystemTime};

use opentelemetry::trace::{
    Span, SpanBuilder, SpanKind, Status, TraceContextExt, Tracer, TracerProvider,
};
use opentelemetry::{Context, KeyValue};
use opentelemetry_sdk::Resource;
use opentelemetry_sdk::trace::{SdkTracer, SdkTracerProvider};
use weft_dashboard::{
    BODY, CALLS, CHECK, CHECKS, CODE, DIAGRAM, FILE, FLOW, FROM, HOPS, INPUT, METHOD, ORDER, PATH, REFUSED,
    REPLY, STATE, STATUS, STEP, STEPS, TEST_CASE, TEST_CASES, TO,
};
use weft_draw::Moment;
use weft_parse::Diagram;
use weft_simulator::{Event, Line};

use crate::trace;

const SERVICE: &str = "weft";
const ERROR: &str = "error";
const START: &str = "start";
const PASSED: char = '✓';
const FAILED: char = '✕';
const ENDPOINTS: [&str; 2] = ["OTEL_EXPORTER_OTLP_ENDPOINT", "OTEL_EXPORTER_OTLP_TRACES_ENDPOINT"];

static PROVIDER: OnceLock<SdkTracerProvider> = OnceLock::new();

pub struct Ended<'a> {
    pub status: &'a str,
    pub test_cases: u64,
    pub steps: u64,
    pub checks: u64,
    pub check: Option<String>,
    pub took: Duration,
    pub test_case: Option<u64>,
}

impl Ended<'_> {
    pub fn error(e: &str) -> Ended<'static> {
        Ended {
            status: ERROR,
            test_cases: 0,
            steps: 0,
            checks: 0,
            check: Some(e.to_string()),
            took: Duration::ZERO,
            test_case: None,
        }
    }
}

pub struct Spans<'a> {
    tracer: SdkTracer,
    d: &'a Diagram,
    lines: Vec<Line>,
    actions: Vec<(String, String)>,
    judged: BTreeMap<String, String>,
    file: String,
    run: i64,
    k: i64,
    order: i64,
    calls: i64,
    check: Option<String>,
    case: Option<Context>,
    step: Option<Context>,
}

pub fn traced(dashboard: bool) -> Result<(), String> {
    let mut builder = SdkTracerProvider::builder()
        .with_resource(Resource::builder().with_service_name(SERVICE).build());
    let otlp = ENDPOINTS.iter().any(|name| env::var_os(name).is_some());
    if dashboard {
        builder = builder.with_span_processor(weft_dashboard::processor());
    }
    if otlp {
        let exporter = opentelemetry_otlp::SpanExporter::builder()
            .with_http()
            .build()
            .map_err(|e| e.to_string())?;
        builder = builder.with_batch_exporter(exporter);
    }
    if dashboard || otlp {
        let _ = PROVIDER.set(builder.build());
    }
    Ok(())
}

pub fn flushed() {
    if let Some(provider) = PROVIDER.get() {
        let _ = provider.shutdown();
    }
}

pub fn ended(file: &str, end: Ended) {
    if let Some(provider) = PROVIDER.get() {
        let mut attributes = vec![
            KeyValue::new(FILE, file.to_string()),
            KeyValue::new(STATUS, end.status.to_string()),
            KeyValue::new(TEST_CASES, end.test_cases as i64),
            KeyValue::new(STEPS, end.steps as i64),
            KeyValue::new(CHECKS, end.checks as i64),
        ];
        attributes.extend(end.check.map(|check| KeyValue::new(CHECK, check)));
        attributes.extend(end.test_case.map(|case| KeyValue::new(TEST_CASE, case as i64)));
        provider
            .tracer(SERVICE)
            .build(
                SpanBuilder::from_name(file.to_string())
                    .with_start_time(SystemTime::now() - end.took)
                    .with_attributes(attributes),
            )
            .end();
    }
}

impl<'a> Spans<'a> {
    pub fn new(file: &str, d: &'a Diagram) -> Option<Spans<'a>> {
        PROVIDER.get().map(|provider| Spans {
            tracer: provider.tracer(SERVICE),
            d,
            lines: Vec::new(),
            actions: Vec::new(),
            judged: BTreeMap::new(),
            file: file.to_string(),
            run: 0,
            k: 0,
            order: 0,
            calls: 0,
            check: None,
            case: None,
            step: None,
        })
    }

    fn placed(&mut self) -> Vec<KeyValue> {
        self.order += 1;
        vec![
            KeyValue::new(FILE, self.file.clone()),
            KeyValue::new(TEST_CASE, self.run),
            KeyValue::new(STEP, self.k),
            KeyValue::new(ORDER, self.order),
        ]
    }

    fn start(&self, parent: &Context, builder: SpanBuilder) -> Context {
        parent.with_span(self.tracer.build_with_context(builder, parent))
    }

    fn close(&mut self) {
        if let Some(step) = self.step.take() {
            let span = step.span();
            span.set_attributes(self.drawn());
            let judged = mem::take(&mut self.judged);
            span.set_attribute(KeyValue::new(CHECK, judged.into_values().collect::<Vec<_>>().join("  ")));
            span.end();
        }
    }

    fn judge(&mut self, name: &str, result: String) {
        let kept = self.judged.entry(name.to_string()).or_default();
        if kept.is_empty() || result.starts_with(FAILED) {
            *kept = result;
        }
    }

    fn drawn(&self) -> [KeyValue; 2] {
        let Some(line) = self.lines.last() else {
            return [KeyValue::new(DIAGRAM, String::new()), KeyValue::new(FLOW, String::new())];
        };
        let failed: Vec<String> = trace::brief(&line.evidence).pop().into_iter().collect();
        let moment = Moment {
            actions: &self.actions,
            done: line.action.as_ref().map(|(verb, _)| verb.as_str()),
            state: &line.state,
            failed: &failed,
        };
        [KeyValue::new(DIAGRAM, moment.sequence(self.d)), KeyValue::new(FLOW, moment.flow(self.d))]
    }

    fn stepped(&mut self, line: &Line, refused: Option<&str>) {
        let Some(case) = self.case.clone() else {
            return;
        };
        let hops = trace::sorted(&line.hops);
        let mut attributes = self.placed();
        attributes.push(KeyValue::new(INPUT, serde_json::to_string(&line.input).unwrap_or_default()));
        attributes.push(KeyValue::new(
            HOPS,
            hops.iter()
                .map(|h| format!("{} {} → {} {}", h.method, h.path, h.status, trace::compact(&h.reply)).trim_end().to_string())
                .collect::<Vec<_>>()
                .join(" · "),
        ));
        attributes.push(match refused {
            Some(cause) => KeyValue::new(REFUSED, cause.to_string()),
            None => KeyValue::new(STATE, serde_json::to_string(&line.state).unwrap_or_default()),
        });
        let name = match &line.action {
            Some((verb, to)) => {
                attributes.push(KeyValue::new(FROM, self.d.caller().unwrap_or(to).to_string()));
                attributes.push(KeyValue::new(TO, to.clone()));
                verb.clone()
            }
            None => START.to_string(),
        };
        let step = self.start(&case, SpanBuilder::from_name(name).with_attributes(attributes));
        for h in hops {
            let mut attributes = self.placed();
            attributes.extend([
                KeyValue::new(METHOD, h.method.clone()),
                KeyValue::new(PATH, h.path.clone()),
                KeyValue::new(CODE, i64::from(h.status)),
                KeyValue::new(FROM, h.from.clone()),
                KeyValue::new(TO, h.to.clone()),
                KeyValue::new(BODY, trace::compact(&h.body)),
                KeyValue::new(REPLY, trace::compact(&h.reply)),
            ]);
            self.calls += 1;
            self.start(
                &step,
                SpanBuilder::from_name(format!("{} {}", h.method, h.path))
                    .with_kind(SpanKind::Client)
                    .with_attributes(attributes),
            )
            .span()
            .end();
        }
        match refused {
            Some(_) => step.span().end(),
            None => self.step = Some(step),
        }
    }

    pub fn hear(&mut self, event: Event) {
        match event {
            Event::Hegel(_) => {}
            Event::Actions(actions) => self.actions = actions.to_vec(),
            Event::Run(run) => {
                self.close();
                (self.run, self.k, self.order, self.calls, self.check) = (run as i64, 0, 0, 0, None);
                self.lines.clear();
                let attributes = vec![
                    KeyValue::new(FILE, self.file.clone()),
                    KeyValue::new(TEST_CASE, self.run),
                ];
                self.case = Some(self.start(
                    &Context::new(),
                    SpanBuilder::from_name(format!("test case {run}")).with_attributes(attributes),
                ));
            }
            Event::Step(line) => {
                self.close();
                self.stepped(line, None);
                self.lines.push(line.clone());
                self.k += 1;
            }
            Event::Refused(line, cause) => self.stepped(line, Some(cause)),
            Event::Check(name, None) => self.judge(name, PASSED.to_string()),
            Event::Check(name, Some(evidence)) => {
                if let Some(line) = self.lines.last_mut() {
                    line.evidence = evidence.to_vec();
                }
                let check = trace::brief(evidence).pop().unwrap_or_default();
                if let Some(step) = &self.step {
                    step.span().set_status(Status::error(check.clone()));
                }
                self.judge(name, format!("{FAILED} {check}"));
                self.check = Some(check);
            }
            Event::End(..) => {
                self.close();
                if let Some(case) = self.case.take() {
                    let span = case.span();
                    span.set_attribute(KeyValue::new(STEPS, self.k.saturating_sub(1)));
                    span.set_attribute(KeyValue::new(CALLS, self.calls));
                    if let Some(check) = self.check.take() {
                        span.set_status(Status::error(check));
                    }
                    span.end();
                }
            }
        }
    }
}
