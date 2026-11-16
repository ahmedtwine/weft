mod vendor;

use std::fmt::Write;

use merman::ascii::AsciiRenderOptions;
use merman::{AsciiRequest, OperationControl, RenderOutput, RenderRequest, Renderer, SvgRequest};
use sealmap_mermaid::{Ident, escape_text};
use serde_json::Value;
use vendor::lucide::ICONS;
use weft_parse::{Diagram, Invariant, Now, Op, State};

const WRAP: usize = 24;
const PAD: f64 = 10.0;
const RESERVE: usize = 2;
const CHAR: f64 = 6.7;
const NOTE_LINE: f64 = 19.0;
const NOTE_MARGIN: f64 = 20.0;
const MONO: f64 = 0.6;
const LABEL: usize = 46;
const SUMMARY: usize = 22;
const STYLE: &str = "<style>.pele{--w-bg:var(--pele-bg,#ffffff);--w-fg:var(--pele-fg,#1f1f1f);--w-muted:var(--pele-muted,#6e6e6e);--w-line:var(--pele-line,#1f1f1f);--w-surface:var(--pele-surface,#f3f3f3);--w-border:var(--pele-border,#8a8a8a);--w-critical:var(--pele-critical,#e45756);--w-s1:var(--pele-series-1,#4c78a8);--w-s2:var(--pele-series-2,#f58518);--w-s3:var(--pele-series-3,#54a24b);--w-s4:var(--pele-series-4,#e45756);--w-s5:var(--pele-series-5,#72b7b2);--w-s6:var(--pele-series-6,#eeca3b);--w-s7:var(--pele-series-7,#b279a2);--w-s8:var(--pele-series-8,#9d755d);font-family:var(--pele-font,Inter,ui-sans-serif,system-ui,-apple-system,'Segoe UI',sans-serif)}.pele .m{font-family:var(--pele-font-mono,'JetBrains Mono',ui-monospace,SFMono-Regular,Menlo,monospace)}</style>";
const DIRECTIVE: &str =
    "%%{init: {\"sequence\": {\"mirrorActors\": false, \"actorMargin\": 16, \"width\": 96}}}%%\n";

#[derive(Clone, Copy)]
pub enum Theme {
    Light,
    Dark,
}

const BG: &str = "var(--w-bg)";
const FG: &str = "var(--w-fg)";
const MUTED: &str = "var(--w-muted)";
const LINE: &str = "var(--w-line)";
const SURFACE: &str = "var(--w-surface)";
const BORDER: &str = "var(--w-border)";
const CRITICAL: &str = "var(--w-critical)";
const DARK: &str = "--pele-bg:#161616;--pele-fg:#e6e6e6;--pele-muted:#9a9a9a;--pele-line:#e6e6e6;--pele-surface:#222222;--pele-border:#4a4a4a;--pele-critical:#f07574;color-scheme:dark";

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Initial,
    Failed,
}

pub struct Call {
    pub from: String,
    pub to: String,
    pub text: String,
    pub reply: bool,
}

pub struct Step {
    pub verb: String,
    pub from: String,
    pub to: String,
    pub calls: Vec<Call>,
    pub changed: Vec<Row>,
}

#[derive(Default)]
pub struct Code {
    pub start_calls: Vec<Call>,
    pub steps: Vec<Step>,
    pub failed: Option<(Option<usize>, Vec<String>)>,
    pub initial: Vec<(String, Vec<Row>)>,
}

pub type Row = [String; 3];

enum Message<'a> {
    Step(usize),
    Call(&'a Call),
}

const SERIES: [&str; 8] = [
    "var(--w-s1)",
    "var(--w-s2)",
    "var(--w-s3)",
    "var(--w-s4)",
    "var(--w-s5)",
    "var(--w-s6)",
    "var(--w-s7)",
    "var(--w-s8)",
];

fn colors(d: &Diagram) -> Vec<Option<&'static str>> {
    let mut next = 0;
    d.objects
        .iter()
        .map(|o| match o.kind.is_person() {
            true => None,
            false => {
                next += 1;
                Some(SERIES[(next - 1) % SERIES.len()])
            }
        })
        .collect()
}

enum Note<'a> {
    Text(String, Kind),
    Changed(&'a [Row]),
}

struct Plan<'a> {
    source: String,
    messages: Vec<Message<'a>>,
    heads: Vec<Vec<String>>,
    notes: Vec<Note<'a>>,
    initial: Vec<(usize, Vec<Row>)>,
    svg: bool,
}

fn count(s: &str) -> usize {
    s.chars().count()
}

fn width(s: &str, size: f64) -> f64 {
    s.chars()
        .map(|c| match c {
            'i' | 'l' | 'j' | 'I' | '|' | '.' | ',' | ':' | ';' | '\'' | '!' => 0.3,
            'f' | 't' | 'r' | ' ' | '(' | ')' | '[' | ']' | '/' | '-' => 0.38,
            'm' | 'w' | 'M' | 'W' => 0.88,
            c if c.is_uppercase() => 0.7,
            c if c.is_ascii_digit() => 0.62,
            _ => 0.58,
        })
        .sum::<f64>()
        * size
}

fn wrap(s: &str, max: usize) -> Vec<String> {
    let words: Vec<&str> = s.split_inclusive([' ', '/']).collect();
    let line = |w: &[&str]| w.concat().trim_end().to_string();
    let longest = |k: &usize| count(&line(&words[..*k])).max(count(&line(&words[*k..])));
    match (count(s) > max, (1..words.len()).min_by_key(longest)) {
        (true, Some(k)) => vec![line(&words[..k]), line(&words[k..])],
        _ => vec![s.trim_end().to_string()],
    }
}

fn mermaid_text(lines: &[String]) -> String {
    lines
        .iter()
        .map(|l| match l.trim() {
            "" => "\u{a0}".to_string(),
            _ => l
                .chars()
                .map(|c| match c {
                    ' ' => "\u{a0}".to_string(),
                    '#' => "#35;".to_string(),
                    ';' => "#59;".to_string(),
                    '<' => "#60;".to_string(),
                    '>' => "#62;".to_string(),
                    c => c.to_string(),
                })
                .collect(),
        })
        .collect::<Vec<_>>()
        .join("<br/>")
}

fn col(d: &Diagram, name: &str) -> usize {
    d.objects.iter().position(|o| o.name == name).unwrap_or(0)
}

fn order(d: &Diagram, code: Option<&Code>) -> Vec<usize> {
    let mut seen = Vec::new();
    let steps = code
        .into_iter()
        .flat_map(|c| &c.steps)
        .flat_map(|s| [col(d, &s.from), col(d, &s.to)]);
    for i in steps.chain(0..d.objects.len()) {
        if !seen.contains(&i) {
            seen.push(i);
        }
    }
    seen
}

fn stereotype(kind: Kind) -> &'static str {
    match kind {
        Kind::Initial => "«initial»",
        Kind::Failed => "«fails»",
    }
}

fn lines(text: &str) -> Vec<String> {
    text.lines().flat_map(|l| wrap(l, WRAP)).collect()
}

fn condition(text: &str) -> Vec<String> {
    [" ≠ ", " = ", " ≥ ", " > ", " ≤ ", " < "]
        .iter()
        .find_map(|sign| {
            let (left, right) = text.split_once(sign)?;
            Some(vec![
                left.to_string(),
                format!("{}{right}", sign.trim_start()),
            ])
        })
        .unwrap_or_else(|| lines(text))
}

fn cut(text: &str) -> String {
    match count(text) > LABEL {
        true => text.chars().take(LABEL - 1).chain(['…']).collect(),
        false => text.to_string(),
    }
}

fn label(text: &str) -> Vec<String> {
    let body = text.find(['{', '[']).filter(|_| count(text) > LABEL);
    match body {
        Some(at) => vec![cut(text[..at].trim()), cut(&text[at..])],
        None => vec![cut(text)],
    }
}

fn note(p: &mut Plan, over: &str, text: String, kind: Kind) {
    let shown = match (p.svg, kind) {
        (true, Kind::Failed) => condition(&text),
        (true, _) => [vec![stereotype(kind).to_string()], lines(&text)].concat(),
        (false, _) => text.lines().map(String::from).collect(),
    };
    let _ = writeln!(p.source, "  Note over {over}: {}", mermaid_text(&shown));
    p.notes.push(Note::Text(text, kind));
}

fn calls<'a>(p: &mut Plan<'a>, d: &Diagram, all: &'a [Call]) {
    for c in all {
        let known = |name: &str| d.objects.iter().any(|o| o.name == name);
        if !known(&c.from) || !known(&c.to) {
            continue;
        }
        let _ = writeln!(
            p.source,
            "  o{}{}o{}: {}",
            col(d, &c.from),
            match c.reply {
                true => "-->>",
                false => "->>",
            },
            col(d, &c.to),
            mermaid_text(&label(&c.text))
        );
        p.messages.push(Message::Call(c));
    }
}

fn plan<'a>(d: &Diagram, code: Option<&'a Code>) -> Plan<'a> {
    let svg = code.is_some();
    let mut p = Plan {
        source: String::new(),
        messages: Vec::new(),
        heads: Vec::new(),
        notes: Vec::new(),
        initial: Vec::new(),
        svg,
    };
    if svg {
        p.source.push_str(DIRECTIVE);
    }
    p.source.push_str("sequenceDiagram\n  autonumber\n");
    p.heads = d.objects.iter().map(|o| wrap(&o.name, WRAP)).collect();
    let order = order(d, code);
    for &i in &order {
        let head = &p.heads[i];
        let name = &d.objects[i].name;
        let rows = code
            .and_then(|c| c.initial.iter().find(|(o, _)| o == name))
            .map(|(_, rows)| rows.clone())
            .or_else(|| started(d, i))
            .filter(|_| svg);
        let lines = match &rows {
            Some(rows) => {
                let mut reserve = vec![String::new(); RESERVE];
                reserve[0] = "x".repeat((table(Some(name), rows).0 / CHAR).ceil() as usize);
                [reserve, head.clone()].concat()
            }
            None => head.clone(),
        };
        let _ = writeln!(p.source, "  participant o{i} as {}", mermaid_text(&lines));
        p.initial.extend(rows.map(|rows| (i, rows)));
    }
    if let (true, Some(first), Some(last)) =
        (svg && !p.initial.is_empty(), order.first(), order.last())
    {
        let tallest = p
            .initial
            .iter()
            .map(|(i, rows)| table(Some(&d.objects[*i].name), rows).1)
            .fold(0.0, f64::max);
        let lines = ((tallest - NOTE_MARGIN) / NOTE_LINE).ceil() as usize;
        let reserved = vec!["x".to_string(); lines];
        let _ = writeln!(
            p.source,
            "  Note over o{first},o{last}: {}",
            mermaid_text(&reserved)
        );
        p.notes.push(Note::Text(String::new(), Kind::Initial));
    }
    if let Some(code) = code {
        calls(&mut p, d, &code.start_calls);
        if let (Some((None, evidence)), Some(first)) = (code.failed.as_ref(), order.first()) {
            note(
                &mut p,
                &format!("o{first}"),
                evidence.join("\n"),
                Kind::Failed,
            );
        }
    }
    for (k, s) in code.into_iter().flat_map(|c| c.steps.iter()).enumerate() {
        p.messages.push(Message::Step(k));
        let _ = writeln!(
            p.source,
            "  o{}->>o{}: {}",
            col(d, &s.from),
            col(d, &s.to),
            mermaid_text(&[format!("«{}»", s.verb)])
        );
        calls(&mut p, d, &s.calls);
        if !s.changed.is_empty() {
            let i = col(d, &s.to);
            let (w, h) = table(None, &s.changed);
            let lines = ((h - NOTE_MARGIN) / NOTE_LINE).ceil() as usize;
            let reserved = vec!["x".repeat((w / CHAR).ceil() as usize); lines];
            let _ = writeln!(p.source, "  Note over o{i}: {}", mermaid_text(&reserved));
            p.notes.push(Note::Changed(&s.changed));
        }
        if let Some((Some(at), evidence)) = code.and_then(|c| c.failed.as_ref())
            && *at == k
        {
            note(
                &mut p,
                &format!("o{}", col(d, &s.to)),
                evidence.join("\n"),
                Kind::Failed,
            );
        }
    }
    p
}

fn render(request: RenderRequest<'_>) -> Result<RenderOutput, String> {
    Renderer::new().render(request).map_err(|e| e.to_string())
}

fn layout(source: &str, kind: &str) -> Result<Value, String> {
    let request =
        RenderRequest::layout_json(source, OperationControl::new(), SvgRequest::default());
    let RenderOutput::LayoutJson(Some(l)) = render(request)? else {
        return Err("merman laid out no diagram".into());
    };
    let mut bytes = Vec::new();
    l.layout()
        .write_json_pretty(&mut bytes)
        .map_err(|e| e.to_string())?;
    let value: Value = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    Ok(value["layout"][kind].clone())
}

fn num(v: &Value, k: &str) -> f64 {
    v[k].as_f64().unwrap_or(0.0)
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

pub fn icon(name: &str) -> Option<&'static str> {
    ICONS
        .binary_search_by_key(&name, |(n, _)| n)
        .ok()
        .map(|i| ICONS[i].1)
}

pub fn styled(styles: &[(weft_parse::Kind, String)]) -> Result<(), String> {
    match styles.iter().find(|(_, i)| icon(i).is_none()) {
        Some((kind, i)) => Err(format!("`style \"{kind}\"`: `{i}` is not a Lucide icon")),
        None => Ok(()),
    }
}

fn write_text(out: &mut String, x: f64, y: f64, text: &str, color: &str, weight: u16, size: f64) {
    let _ = write!(
        out,
        r#"<text x="{x:.1}" y="{y:.1}" text-anchor="middle" font-size="{size}" font-weight="{weight}" style="fill:{color}">{}</text>"#,
        escape(text)
    );
}

fn tag(out: &mut String, (x, y): (f64, f64), text: &str) {
    let w = width(text, 12.0) + 14.0;
    let _ = write!(
        out,
        r#"<rect x="{:.1}" y="{:.1}" width="{w:.1}" height="20" rx="10" style="fill:{BG};stroke:{BORDER};stroke-width:0.75"/>"#,
        x - w / 2.0,
        y - 10.0
    );
    write_text(out, x, y + 4.3, text, FG, 500, 12.0);
}

fn mono(out: &mut String, (x, y): (f64, f64), text: &str, size: f64, first: bool) {
    let (head, tail) = match (first, text.split_once(' ')) {
        (true, Some((head, tail))) => (head, format!(" {tail}")),
        (true, None) => (text, String::new()),
        (false, _) => ("", text.to_string()),
    };
    let _ = write!(
        out,
        r#"<text class="m" x="{x:.1}" y="{y:.1}" font-size="{size}" font-weight="400" style="fill:{FG}"><tspan font-weight="600">{}</tspan>{}</text>"#,
        escape(head),
        escape(&tail)
    );
}

fn bubble(out: &mut String, (cx, bottom): (f64, f64), text: &str, color: &str) {
    let lines = label(text);
    let wide = lines.iter().map(|l| count(l)).max().unwrap_or(0) as f64 * 11.0 * MONO;
    let (w, h) = (wide + 24.0, lines.len() as f64 * 15.0 + 10.0);
    let (x, y) = (cx - w / 2.0, bottom - h);
    let _ = write!(
        out,
        r#"<rect x="{x:.1}" y="{y:.1}" width="{w:.1}" height="{h:.1}" rx="6" style="fill:{SURFACE};stroke:{BORDER};stroke-width:0.6"/><rect x="{:.1}" y="{:.1}" width="3" height="{:.1}" rx="1.5" style="fill:{color}"/>"#,
        x + 5.0,
        y + 5.0,
        h - 10.0
    );
    for (i, line) in lines.iter().enumerate() {
        mono(
            out,
            (x + 15.0, y + 18.5 + i as f64 * 15.0),
            line,
            11.0,
            i == 0,
        );
    }
}

fn failure(out: &mut String, (x, y): (f64, f64), text: &str) {
    let lines = condition(text);
    let wide = lines.iter().map(|l| count(l)).max().unwrap_or(0) as f64 * 12.0 * MONO;
    let (w, h) = (wide + 52.0, lines.len() as f64 * 18.0 + 18.0);
    let (left, top) = (x - w / 2.0, y - h / 2.0);
    let (cx, cy) = (left + 18.0, y);
    let _ = write!(
        out,
        r#"<rect x="{left:.1}" y="{top:.1}" width="{w:.1}" height="{h:.1}" rx="8" style="fill:{BG};stroke:{CRITICAL};stroke-width:1.25"/><circle cx="{cx:.1}" cy="{cy:.1}" r="8" style="fill:{CRITICAL}"/><path d="M {:.1},{:.1} L {:.1},{:.1} M {:.1},{:.1} L {:.1},{:.1}" style="stroke:{BG};stroke-width:1.6;stroke-linecap:round"/>"#,
        cx - 3.0,
        cy - 3.0,
        cx + 3.0,
        cy + 3.0,
        cx + 3.0,
        cy - 3.0,
        cx - 3.0,
        cy + 3.0
    );
    for (i, line) in lines.iter().enumerate() {
        let _ = write!(
            out,
            r#"<text class="m" x="{:.1}" y="{:.1}" font-size="12" font-weight="500" style="fill:{FG}">{}</text>"#,
            left + 34.0,
            top + 22.5 + i as f64 * 18.0,
            escape(line)
        );
    }
}

const HEAD: f64 = 34.0;
const CELL: f64 = 26.0;
const GAP: f64 = 16.0;
const EDGE: f64 = 14.0;

pub fn summary(value: &Value) -> String {
    let many = |n: usize, one: &str| match n {
        1 => format!("1 {one}"),
        n => format!("{n} {one}s"),
    };
    match value {
        Value::Array(all) => many(all.len(), "item"),
        Value::Object(all) => many(all.len(), "entry").replace("entrys", "entries"),
        Value::String(s) if s.is_empty() => "\"\"".to_string(),
        Value::String(s) => {
            let first = s
                .lines()
                .find(|l| !l.trim().is_empty())
                .unwrap_or_default()
                .trim();
            match (count(first) > SUMMARY, s.lines().count() > 1) {
                (true, _) => first.chars().take(SUMMARY - 1).chain(['…']).collect(),
                (false, true) => format!("{first} …"),
                (false, false) => first.to_string(),
            }
        }
        other => other.to_string(),
    }
}

pub fn guessed(value: &Value) -> &'static str {
    match value {
        Value::String(_) => "string",
        Value::Bool(_) => "bool",
        Value::Number(n) if n.is_f64() => "float",
        Value::Number(_) => "int",
        Value::Array(_) => "list",
        Value::Object(_) => "map",
        Value::Null => "null",
    }
}

fn started(d: &Diagram, i: usize) -> Option<Vec<Row>> {
    let start = &d.objects.get(i)?.start;
    (!start.is_empty()).then(|| {
        start
            .iter()
            .map(|(k, v)| [guessed(v).to_string(), k.clone(), summary(v)])
            .collect()
    })
}

fn columns(rows: &[Row]) -> [f64; 3] {
    let widest = |n: usize, size: f64| rows.iter().map(|r| width(&r[n], size)).fold(0.0, f64::max);
    let value = rows
        .iter()
        .map(|r| count(&r[2]) as f64 * 12.0 * MONO)
        .fold(0.0, f64::max);
    [widest(0, 12.0), widest(1, 13.0), value]
}

fn table(name: Option<&str>, rows: &[Row]) -> (f64, f64) {
    let [t, k, v] = columns(rows);
    let head = name.map_or(0.0, |n| width(&n.to_uppercase(), 12.5) * 1.12 + EDGE * 2.0);
    let w = (EDGE * 2.0 + t + k + v + GAP * 2.0).max(head);
    (w, name.map_or(0.0, |_| HEAD) + rows.len() as f64 * CELL)
}

fn entity(out: &mut String, (cx, top): (f64, f64), name: Option<&str>, rows: &[Row]) {
    let (w, h) = table(name, rows);
    let [t, k, _] = columns(rows);
    let left = cx - w / 2.0;
    let _ = write!(
        out,
        r#"<g><rect x="{left:.1}" y="{top:.1}" width="{w:.1}" height="{h:.1}" rx="8" style="fill:{BG};stroke:{BORDER}"/>"#
    );
    if let Some(name) = name {
        let _ = write!(
            out,
            r#"<path d="M{:.1} {:.1}v-{:.1}a8 8 0 0 1 8 -8h{:.1}a8 8 0 0 1 8 8v{:.1}z" style="fill:{SURFACE}"/><line x1="{left:.1}" y1="{:.1}" x2="{:.1}" y2="{:.1}" style="stroke:{BORDER}"/><text x="{cx:.1}" y="{:.1}" text-anchor="middle" font-size="12.5" font-weight="500" letter-spacing="0.06em" style="fill:{FG}">{}</text>"#,
            left + 0.5,
            top + HEAD,
            HEAD - 8.5,
            w - 17.0,
            HEAD - 8.5,
            top + HEAD,
            left + w,
            top + HEAD,
            top + HEAD / 2.0 + 4.5,
            escape(&name.to_uppercase())
        );
    }
    let head = name.map_or(0.0, |_| HEAD);
    for (n, [kind, key, value]) in rows.iter().enumerate() {
        let y = top + head + n as f64 * CELL;
        if n > 0 {
            let _ = write!(
                out,
                r#"<line x1="{:.1}" y1="{y:.1}" x2="{:.1}" y2="{y:.1}" style="stroke:{BORDER};stroke-opacity:0.45"/>"#,
                left + 1.0,
                left + w - 1.0
            );
        }
        let base = y + CELL / 2.0 + 4.3;
        let _ = write!(
            out,
            r#"<text x="{:.1}" y="{base:.1}" font-size="12" style="fill:{MUTED}">{}</text><text x="{:.1}" y="{base:.1}" font-size="13" font-weight="500" style="fill:{FG}">{}</text><text class="m" x="{:.1}" y="{base:.1}" font-size="12" style="fill:{FG}">{}</text>"#,
            left + EDGE,
            escape(kind),
            left + EDGE + t + GAP,
            escape(key),
            left + EDGE + t + GAP + k + GAP,
            escape(value)
        );
    }
    out.push_str("</g>");
}

fn arrow(out: &mut String, (px, py): (f64, f64), (zx, zy): (f64, f64), color: &str) {
    let len = ((zx - px).powi(2) + (zy - py).powi(2)).sqrt().max(1.0);
    let (ux, uy) = ((zx - px) / len, (zy - py) / len);
    let (bx, by) = (zx - ux * 8.0, zy - uy * 8.0);
    let _ = write!(
        out,
        r#"<polygon points="{zx:.1},{zy:.1} {:.1},{:.1} {:.1},{:.1}" style="fill:{color}"/>"#,
        bx - uy * 4.0,
        by + ux * 4.0,
        bx + uy * 4.0,
        by - ux * 4.0
    );
}

fn sorted<'a>(items: &'a [Value], prefix: &str) -> Vec<&'a Value> {
    let mut out: Vec<(usize, &Value)> = items
        .iter()
        .filter_map(|v| Some((v["id"].as_str()?.strip_prefix(prefix)?.parse().ok()?, v)))
        .collect();
    out.sort_by_key(|(i, _)| *i);
    out.into_iter().map(|(_, v)| v).collect()
}

fn points(e: &Value) -> Vec<(f64, f64)> {
    e["points"]
        .as_array()
        .map(|a| a.iter().map(|q| (num(q, "x"), num(q, "y"))).collect())
        .unwrap_or_default()
}

const BREAK: &str = "<br/>";
const DONE: &str = "done";
pub const LAYOUTS: &str = "\n%% layout\n";
const RUNNERS: [usize; 7] = [1, 2, 3, 5, 6, 7, 8];

const FAILED_SERIES: usize = 4;

fn series(n: usize) -> usize {
    RUNNERS[n % RUNNERS.len()]
}
const FAILS: &str = "fails";

const WIDTH: usize = 28;

fn fixed(text: &str) -> String {
    let cut: String = match text.chars().count() > WIDTH {
        true => text.chars().take(WIDTH - 1).chain(['…']).collect(),
        false => text.to_string(),
    };
    escape_text(&cut)
}

fn sign(op: Op) -> Option<&'static str> {
    match op {
        Op::Eq => Some("="),
        Op::Neq => Some("≠"),
        Op::Ilt => Some("<"),
        Op::Ilte => Some("≤"),
        Op::Igt => Some(">"),
        Op::Igte => Some("≥"),
        Op::In => Some("in"),
        _ => None,
    }
}

fn read<'a>(now: &'a Now, keys: &mut Vec<(&'a str, &'a str)>, signs: &mut Vec<&'static str>) {
    match now {
        Now::Key(object, key) | Now::Start(object, key) => {
            if !keys.contains(&(object.as_str(), key.as_str())) {
                keys.push((object, key));
            }
        }
        Now::App(op, args, _) => {
            signs.extend(sign(*op).filter(|s| !signs.contains(s)));
            args.iter().for_each(|a| read(a, keys, signs));
        }
        Now::Each(_, _, set, body, _) => {
            read(set, keys, signs);
            read(body, keys, signs);
        }
        _ => {}
    }
}

fn reads(i: &Invariant) -> (Vec<(&str, &str)>, Vec<&'static str>) {
    let (mut keys, mut signs) = (Vec::new(), Vec::new());
    read(&i.body, &mut keys, &mut signs);
    (keys, signs)
}

pub struct Moment<'a> {
    pub actions: &'a [(String, String)],
    pub done: Option<&'a str>,
    pub state: &'a State,
    pub failed: &'a [String],
    pub broken: Option<&'a str>,
}

impl Moment<'_> {
    fn done(&self, verb: &str) -> bool {
        self.done == Some(verb)
    }

    fn target<'d>(&self, d: &'d Diagram) -> Option<&'d str> {
        let to = self
            .done
            .and_then(|did| self.actions.iter().find(|(verb, _)| verb == did))
            .map(|(_, to)| to.as_str());
        d.objects
            .iter()
            .map(|o| o.name.as_str())
            .find(|name| Some(*name) == to)
            .or(d.caller())
    }

    fn check(&self) -> Option<String> {
        (!self.failed.is_empty()).then(|| {
            format!(
                "{}{BREAK}{}",
                stereotype(Kind::Failed),
                self.failed.join(BREAK)
            )
        })
    }

    fn lines(&self, name: &str) -> Vec<String> {
        self.state
            .get(name)
            .into_iter()
            .flatten()
            .map(|(key, value)| fixed(&format!("{key} = {}", summary(value))))
            .collect()
    }

    pub fn sequence(&self, d: &Diagram) -> String {
        let mut out = vec!["sequenceDiagram".to_string()];
        let ran: Vec<&str> = d
            .impls
            .iter()
            .flat_map(|i| i.modules.iter().map(|(object, _)| object.as_str()))
            .collect();
        for o in d.objects.iter().filter(|o| !ran.contains(&o.name.as_str())) {
            out.push(format!("  participant {}", Ident::new(&o.name)));
        }
        for (n, i) in d.impls.iter().enumerate() {
            for (object, _) in &i.modules {
                out.push(format!("  participant {}", Ident::new(object)));
                out.push(format!("  %% runner {} {}", Ident::new(object), series(n)));
            }
        }
        for (verb, to) in self.actions {
            let arrow = match self.done(verb) {
                true => "->>",
                false => "-->>",
            };
            out.push(format!(
                "  {}{arrow}{}: «{}»",
                Ident::new(d.caller().unwrap_or(to)),
                Ident::new(to),
                escape_text(verb)
            ));
        }
        for o in &d.objects {
            let lines = self.lines(&o.name);
            if !lines.is_empty() {
                out.push(format!(
                    "  Note over {}: {}",
                    Ident::new(&o.name),
                    lines.join(BREAK)
                ));
            }
        }
        if let (Some(to), Some(check)) = (self.target(d), self.check()) {
            out.push(format!("  Note over {}: {check}", Ident::new(to)));
        }
        out.join("\n")
    }

    fn group(&self, d: &Diagram, object: &str, adapter: &str) -> (Vec<String>, Vec<String>) {
        let state = Ident::new(&format!("s_{object}"));
        let section = Ident::new(&format!("o_{object}"));
        let mut out = vec![
            format!(
                "  subgraph {section} [\"{} · {}\"]",
                escape_text(object),
                escape_text(adapter)
            ),
            "    direction LR".into(),
        ];
        let mut lines = self.lines(object);
        if lines.is_empty() {
            for i in d.invariants() {
                for (_, key) in reads(i).0.into_iter().filter(|(o, _)| *o == object) {
                    if !lines.contains(&fixed(key)) {
                        lines.push(fixed(key));
                    }
                }
            }
        }
        if lines.is_empty() {
            lines.push(fixed("state"));
        }
        out.push(format!("    {state}(\"{}\")", lines.join(BREAK)));
        let mut marked = Vec::new();
        for (verb, _) in self.actions.iter().filter(|(_, to)| to == object) {
            let action = Ident::new(&format!("a_{object}_{verb}"));
            out.push(format!(
                "    {state} --> {action}([\"{}\"])",
                escape_text(verb)
            ));
            if self.done(verb) {
                marked.push(format!("  class {action} {DONE}"));
            }
        }
        out.push("  end".into());
        (out, marked)
    }

    pub fn machine(&self, d: &Diagram) -> String {
        let groups: Vec<&str> = d
            .impls
            .iter()
            .flat_map(|i| i.modules.iter().map(|(object, _)| object.as_str()))
            .collect();
        let mut body = vec![
            "flowchart TB".to_string(),
            format!("  classDef {DONE} stroke-width:2px"),
            format!("  classDef {FAILS} stroke-width:2px"),
        ];
        let mut marked = Vec::new();
        for i in &d.impls {
            for (object, _) in &i.modules {
                let (lines, marks) = self.group(d, object, &i.adapter);
                body.extend(lines);
                marked.extend(marks);
            }
        }
        for (n, i) in d.invariants().into_iter().enumerate() {
            let broken = self.broken == Some(i.place().as_str());
            let color = if broken { FAILED_SERIES } else { series(n) };
            let (keys, signs) = reads(i);
            let mut objects: Vec<&str> = Vec::new();
            for (object, key) in &keys {
                let state = Ident::new(&format!("s_{object}"));
                body.push(format!("  %% check {state} {key} {color}"));
                if !objects.contains(object) {
                    objects.push(object);
                    if broken {
                        marked.push(format!("  class {state} {FAILS}"));
                    }
                }
            }
            for (j, pair) in objects.windows(2).enumerate() {
                let link = format!("c_{n}_{j}");
                body.push(format!(
                    "  {} {link}@---|\"{}\"| {}",
                    Ident::new(&format!("s_{}", pair[0])),
                    signs.join(" "),
                    Ident::new(&format!("s_{}", pair[1]))
                ));
                body.push(format!("  %% link {link} {color}"));
            }
        }
        body.extend(marked);
        (1..=groups.len().max(1))
            .map(|columns| {
                let below = groups
                    .iter()
                    .zip(groups.iter().skip(columns))
                    .map(|(up, down)| {
                        format!(
                            "  {} ~~~ {}",
                            Ident::new(&format!("o_{up}")),
                            Ident::new(&format!("o_{down}"))
                        )
                    });
                body.iter()
                    .cloned()
                    .chain(below)
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .collect::<Vec<_>>()
            .join(LAYOUTS)
    }
}

pub fn svg(d: &Diagram, theme: Option<Theme>, code: &Code) -> Result<String, String> {
    let icons = d
        .objects
        .iter()
        .map(|o| {
            icon(&o.icon)
                .ok_or_else(|| format!("`style {}`: `{}` is not a Lucide icon", o.kind, o.icon))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let plan = plan(d, Some(code));
    let l = layout(&plan.source, "SequenceDiagram")?;
    let nodes = l["nodes"].as_array().cloned().unwrap_or_default();
    let edges = l["edges"].as_array().cloned().unwrap_or_default();
    let b = &l["bounds"];
    let rows = plan.heads.iter().map(Vec::len).max().unwrap_or(1) as f64;
    let heads = nodes
        .iter()
        .find(|n| {
            n["id"]
                .as_str()
                .is_some_and(|id| id.starts_with("actor-top-o"))
        })
        .map_or(0.0, |n| num(n, "y") + num(n, "height") / 2.0)
        - 84.0
        - 16.0 * (rows - 1.0);
    let (x0, y0) = (num(b, "min_x"), num(b, "min_y").max(heads - PAD));
    let (w, h) = (num(b, "max_x") - x0, num(b, "max_y") - y0);
    let fixed = match theme {
        Some(Theme::Dark) => DARK,
        Some(Theme::Light) => "color-scheme:light",
        None => "",
    };
    let mut out = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" class="pele weft" viewBox="{x0:.1} {y0:.1} {w:.1} {h:.1}" width="{w:.0}" height="{h:.0}" style="max-width:100%;height:auto;{fixed}" font-size="13">{STYLE}<rect x="{x0:.1}" y="{y0:.1}" width="{w:.1}" height="{h:.1}" style="fill:{BG}"/>"#
    );
    let tint = colors(d);
    for e in edges.iter().filter(|e| {
        e["id"]
            .as_str()
            .is_some_and(|id| id.starts_with("lifeline-"))
    }) {
        let color = e["id"]
            .as_str()
            .and_then(|id| {
                id.trim_start_matches("lifeline-")
                    .trim_start_matches('o')
                    .parse::<usize>()
                    .ok()
            })
            .and_then(|i| tint.get(i).copied().flatten())
            .unwrap_or(BORDER);
        if let [(ax, ay), .., (zx, zy)] = points(e).as_slice() {
            let _ = write!(
                out,
                r#"<line x1="{ax:.1}" y1="{ay:.1}" x2="{zx:.1}" y2="{zy:.1}" style="stroke:{color};stroke-opacity:0.5;stroke-width:1.25"/>"#
            );
        }
    }
    let column: Vec<f64> = (0..d.objects.len())
        .map(|i| {
            nodes
                .iter()
                .find(|n| n["id"].as_str() == Some(&format!("actor-top-o{i}")))
                .map_or(0.0, |n| num(n, "x"))
        })
        .collect();
    for (e, m) in sorted(&edges, "msg-").into_iter().zip(&plan.messages) {
        let pts = points(e);
        let k = match m {
            Message::Step(k) => *k,
            Message::Call(c) => {
                if let ([(ax, ay), ..], [.., prev, last]) = (pts.as_slice(), pts.as_slice()) {
                    let color = tint.get(col(d, &c.from)).copied().flatten().unwrap_or(LINE);
                    let dash = match c.reply {
                        true => ";stroke-dasharray:5 4",
                        false => "",
                    };
                    let line = pts
                        .iter()
                        .map(|(x, y)| format!("{x:.1},{y:.1}"))
                        .collect::<Vec<_>>()
                        .join(" L ");
                    let _ = write!(
                        out,
                        r#"<path d="M {line}" style="fill:none;stroke:{color};stroke-width:1.25{dash}"/>"#
                    );
                    arrow(&mut out, *prev, *last, color);
                    bubble(&mut out, ((ax + last.0) / 2.0, ay - 6.0), &c.text, color);
                }
                continue;
            }
        };
        let Some(&(ax, ay)) = pts.first() else {
            continue;
        };
        let Some(s) = code.steps.get(k) else {
            continue;
        };
        let (path, prev, end, mid) = match (s.from == s.to, pts.as_slice()) {
            (true, _) => (
                format!(
                    "M {ax:.1},{ay:.1} C {:.1},{:.1} {:.1},{:.1} {ax:.1},{:.1}",
                    ax + 60.0,
                    ay - 10.0,
                    ax + 60.0,
                    ay + 30.0,
                    ay + 20.0
                ),
                (ax + 60.0, ay + 30.0),
                (ax, ay + 20.0),
                (ax + 45.0, ay + 10.0),
            ),
            (false, [.., prev, last]) => (
                format!(
                    "M {}",
                    pts.iter()
                        .map(|(x, y)| format!("{x:.1},{y:.1}"))
                        .collect::<Vec<_>>()
                        .join(" L ")
                ),
                *prev,
                *last,
                ((ax + last.0) / 2.0, ay),
            ),
            (false, _) => continue,
        };
        let _ = write!(
            out,
            r#"<path d="{path}" style="fill:none;stroke:{LINE};stroke-width:1.25"/>"#
        );
        arrow(&mut out, prev, end, LINE);
        tag(&mut out, mid, &s.verb);
        let _ = write!(
            out,
            r#"<circle cx="{ax:.1}" cy="{ay:.1}" r="8.5" style="fill:{LINE}"/><text x="{ax:.1}" y="{:.1}" text-anchor="middle" font-size="10.5" font-weight="700" style="fill:{BG}">{}</text>"#,
            ay + 3.8,
            k + 1
        );
    }
    for (n, note) in sorted(&nodes, "note-").into_iter().zip(&plan.notes) {
        let (x, y) = (num(n, "x"), num(n, "y"));
        match note {
            Note::Changed(rows) => {
                entity(&mut out, (x, y - table(None, rows).1 / 2.0), None, rows);
            }
            Note::Text(_, Kind::Initial) => {
                let tallest = plan
                    .initial
                    .iter()
                    .map(|(i, rows)| table(Some(&d.objects[*i].name), rows).1)
                    .fold(0.0, f64::max);
                for (i, rows) in &plan.initial {
                    entity(
                        &mut out,
                        (column[*i], y - tallest / 2.0),
                        Some(&d.objects[*i].name),
                        rows,
                    );
                }
            }
            Note::Text(text, Kind::Failed) => failure(&mut out, (x, y), text),
        }
    }
    for n in &nodes {
        let Some(i) = n["id"]
            .as_str()
            .and_then(|id| id.strip_prefix("actor-top-o"))
            .and_then(|i| i.parse::<usize>().ok())
        else {
            continue;
        };
        let (Some(o), Some(head), Some(drawn)) =
            (d.objects.get(i), plan.heads.get(i), icons.get(i))
        else {
            continue;
        };
        let (x, top) = (num(n, "x"), heads);
        write_text(
            &mut out,
            x,
            top + 12.0,
            &format!("«{}»", o.kind),
            MUTED,
            400,
            11.5,
        );
        let _ = write!(
            out,
            r#"<rect x="{:.1}" y="{:.1}" width="40" height="40" rx="10" style="fill:{SURFACE};stroke:{}"/><g transform="translate({:.1} {:.1})" style="fill:none;stroke:{};stroke-width:1.6;stroke-linecap:round;stroke-linejoin:round">{drawn}</g>"#,
            x - 20.0,
            top + 20.0,
            tint.get(i).copied().flatten().unwrap_or(BORDER),
            x - 12.0,
            top + 28.0,
            tint.get(i).copied().flatten().unwrap_or(FG)
        );
        for (k, line) in head.iter().enumerate() {
            write_text(
                &mut out,
                x,
                top + 76.0 + k as f64 * 16.0,
                line,
                FG,
                600,
                13.0,
            );
        }
    }
    out.push_str("</svg>");
    Ok(out)
}

pub fn ascii(d: &Diagram) -> Result<String, String> {
    let source = plan(d, None).source;
    let request = AsciiRequest {
        options: AsciiRenderOptions::unicode(),
        ..Default::default()
    };
    match render(RenderRequest::ascii(
        &source,
        OperationControl::new(),
        request,
    ))? {
        RenderOutput::Ascii(Some(report)) => Ok(report.text),
        _ => Err("merman drew no diagram".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LINE: f64 = 17.6;

    const STATES: [&str; 4] = [
        "search form",
        "Lahore, Pakistan, Lahore, Pakistan, Lahore, Pakistan",
        "San Diego",
        "Lahore, Pakistan",
    ];

    const DIAGRAM: &str = r#"
setup {
  person "agent"
  browser "page" snapshot="snap_0"
  element "origin" value="San Diego" {
    invariant {
      neq "origin.value" ""
    }
  }
}
impl "bun" {
  module "origin" "../src/origin.ts"
  module "page" "../src/page.ts"
}
"#;

    fn attr(tag: &str, name: &str) -> Option<String> {
        let from = tag.find(&format!(" {name}=\""))? + name.len() + 3;
        tag[from..].split('"').next().map(String::from)
    }

    fn number(tag: &str, name: &str) -> f64 {
        attr(tag, name).and_then(|v| v.parse().ok()).unwrap_or(0.0)
    }

    #[test]
    fn each_state_wraps_to_two_lines_and_shows_each_word() {
        for s in STATES {
            let lines = wrap(s, WRAP);
            assert!(lines.len() <= 2, "{s} wraps to {} lines", lines.len());
            let joined = lines.iter().fold(String::new(), |all, l| {
                match all.is_empty() || all.ends_with('/') {
                    true => all + l,
                    false => all + " " + l,
                }
            });
            assert_eq!(joined, s, "{s} loses text");
        }
    }

    const CONFIG: &str = r#"include "**/*.weft.kdl"
run cases=100 steps=50 check="sampled"
style "person" icon="user"
style "browser" icon="app-window"
style "element" icon="square-dashed-mouse-pointer""#;

    fn diagram() -> Diagram {
        weft_parse::parse(DIAGRAM, &weft_parse::config(CONFIG).unwrap()).unwrap()
    }

    fn step(verb: &str, to: &str, key: &str, value: &str) -> Step {
        Step {
            verb: verb.into(),
            from: "agent".into(),
            to: to.into(),
            calls: Vec::new(),
            changed: vec![["string".into(), key.into(), value.into()]],
        }
    }

    fn drawn() -> String {
        let code = Code {
            steps: vec![
                step("type", "origin", "value", STATES[1]),
                step("click", "page", "snapshot", "snap_1"),
            ],
            ..Code::default()
        };
        svg(&diagram(), None, &code).unwrap()
    }

    #[test]
    fn each_head_touches_the_top_and_its_lifeline() {
        let out = drawn();
        let head = &out[..out.find('>').unwrap()];
        let top: f64 = attr(head, "viewBox")
            .unwrap()
            .split(' ')
            .nth(1)
            .unwrap()
            .parse()
            .unwrap();
        let start = out
            .split("<line")
            .skip(1)
            .map(|tag| number(tag, "y1"))
            .fold(f64::MAX, f64::min);
        let texts: Vec<(f64, f64)> = out
            .split("<text")
            .skip(1)
            .map(|tag| (number(tag, "y"), number(tag, "font-size")))
            .filter(|(y, _)| *y < start)
            .collect();
        let first = texts
            .iter()
            .map(|(y, size)| y - size)
            .fold(f64::MAX, f64::min);
        let last = texts.iter().map(|(y, _)| *y).fold(f64::MIN, f64::max);
        assert!(
            first - top <= LINE,
            "{:.0} px of padding above the heads",
            first - top
        );
        assert!(
            start - last <= LINE,
            "{:.0} px of padding below the heads",
            start - last
        );
    }

    #[test]
    fn each_text_is_inside_the_diagram() {
        let d = diagram();
        let code = Code::default();
        let out = svg(&d, None, &code).unwrap();
        let head = &out[..out.find('>').unwrap()];
        assert!(head.contains(r#"style="max-width:100%;height:auto"#));
        let view: Vec<f64> = attr(head, "viewBox")
            .unwrap()
            .split(' ')
            .map(|v| v.parse().unwrap())
            .collect();
        let plan = plan(&d, Some(&code));
        assert!(plan.heads.iter().all(|h| h.len() <= 2));
        for chunk in out.split("<text").skip(1) {
            let (tag, rest) = chunk.split_once('>').unwrap();
            let content = rest.split("</text>").next().unwrap();
            let size = number(tag, "font-size");
            let bold = match number(tag, "font-weight") >= 600.0 {
                true => 1.06,
                false => 1.0,
            };
            let w = width(content, size) * bold;
            let x = number(tag, "x");
            let (left, right) = match tag.contains(r#"text-anchor="middle""#) {
                true => (x - w / 2.0, x + w / 2.0),
                false => (x, x + w),
            };
            assert!(
                left >= view[0] && right <= view[0] + view[2],
                "`{content}` spans {left:.0}..{right:.0} outside {:.0}..{:.0}",
                view[0],
                view[0] + view[2]
            );
        }
    }
}
