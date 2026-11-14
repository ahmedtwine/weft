mod vendor;

use std::fmt::Write;

use merman::ascii::AsciiRenderOptions;
use merman::{AsciiRequest, OperationControl, RenderOutput, RenderRequest, Renderer, SvgRequest};
use serde_json::Value;
use vendor::lucide::ICONS;
use weft_parse::Diagram;

const WRAP: usize = 24;
const PAD: f64 = 10.0;
const RESERVE: usize = 2;
const CHAR: f64 = 6.7;
const ROW: f64 = 16.0;
const NOTE_LINE: f64 = 19.0;
const NOTE_MARGIN: f64 = 20.0;
const MONO: f64 = 0.6;
const LABEL: usize = 46;
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
    Rule,
    Failed,
}

pub struct Card {
    pub rows: usize,
    pub cols: usize,
    pub html: String,
}

pub struct Call {
    pub from: String,
    pub to: String,
    pub text: String,
    pub reply: bool,
}

#[derive(Default)]
pub struct Code {
    pub start: Option<Card>,
    pub actions: Vec<(String, Card)>,
    pub start_calls: Vec<Call>,
    pub action_calls: Vec<(String, Vec<Call>)>,
    pub failed: Option<(String, Vec<String>)>,
    pub initial: Vec<(String, String)>,
}

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
        .map(|o| match o.kind {
            weft_parse::Kind::Person => None,
            _ => {
                next += 1;
                Some(SERIES[(next - 1) % SERIES.len()])
            }
        })
        .collect()
}

enum Note<'a> {
    Text(String, Kind),
    Card(&'a Card),
}

struct Plan<'a> {
    source: String,
    messages: Vec<Message<'a>>,
    heads: Vec<Vec<String>>,
    notes: Vec<Note<'a>>,
    initial: Vec<(usize, String)>,
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

fn order(d: &Diagram) -> Vec<usize> {
    let mut seen = Vec::new();
    let steps = d.path.iter().flat_map(|s| [col(d, &s.from), col(d, &s.to)]);
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
        Kind::Rule => "«on»",
        Kind::Failed => "«fails»",
    }
}

fn lines(text: &str) -> Vec<String> {
    text.lines().flat_map(|l| wrap(l, WRAP)).collect()
}

fn condition(text: &str) -> Vec<String> {
    match text.split_once(" ≠ ") {
        Some((left, right)) => vec![left.to_string(), format!("≠ {right}")],
        None => lines(text),
    }
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

fn note(p: &mut Plan, over: String, text: String, kind: Kind) {
    let shown = match (p.svg, kind) {
        (true, Kind::Failed) => condition(&text),
        (true, _) => [vec![stereotype(kind).to_string()], lines(&text)].concat(),
        (false, _) => text.lines().map(String::from).collect(),
    };
    let _ = writeln!(p.source, "  Note over {over}: {}", mermaid_text(&shown));
    p.notes.push(Note::Text(text, kind));
}

fn card<'a>(p: &mut Plan<'a>, over: String, card: &'a Card) {
    let lines = ((card.rows as f64 * ROW - NOTE_MARGIN) / NOTE_LINE).ceil() as usize;
    let reserved = vec!["x".repeat(card.cols); lines];
    let _ = writeln!(p.source, "  Note over {over}: {}", mermaid_text(&reserved));
    p.notes.push(Note::Card(card));
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
    let order = order(d);
    for &i in &order {
        let head = &p.heads[i];
        let lines = match svg {
            true => {
                let mut reserve = vec![String::new(); RESERVE];
                let name = &d.objects[i].name;
                reserve[0] = code
                    .and_then(|c| c.initial.iter().find(|(o, _)| o == name))
                    .map(|(_, s)| s.lines().map(String::from).max_by_key(|l| count(l)))
                    .unwrap_or_else(|| d.initial(name))
                    .map_or(String::new(), |s| format!("«initial» {s}"));
                [reserve, head.clone()].concat()
            }
            false => head.clone(),
        };
        let _ = writeln!(p.source, "  participant o{i} as {}", mermaid_text(&lines));
        let name = &d.objects[i].name;
        let real = code
            .and_then(|c| c.initial.iter().find(|(o, _)| o == name))
            .map(|(_, s)| s.clone());
        p.initial
            .extend(real.or_else(|| d.initial(name)).map(|s| (i, s)));
    }
    if let (true, Some(first), Some(last)) =
        (svg && !p.initial.is_empty(), order.first(), order.last())
    {
        let over = format!("o{first},o{last}");
        let widest = p
            .initial
            .iter()
            .flat_map(|(_, s)| lines(s))
            .max_by_key(|l| count(l))
            .unwrap_or_default();
        let tallest = p
            .initial
            .iter()
            .map(|(_, s)| lines(s).len())
            .max()
            .unwrap_or(1);
        note(
            &mut p,
            over,
            vec![widest; tallest].join("\n"),
            Kind::Initial,
        );
    }
    if let (Some(start), Some(first), Some(last)) = (
        code.and_then(|c| c.start.as_ref()),
        order.first(),
        order.last(),
    ) {
        card(&mut p, format!("o{first},o{last}"), start);
    }
    if let Some(code) = code {
        calls(&mut p, d, &code.start_calls);
        if let (Some(("start", evidence)), Some(first)) = (
            code.failed.as_ref().map(|(v, e)| (v.as_str(), e)),
            order.first(),
        ) {
            note(
                &mut p,
                format!("o{first}"),
                evidence.join("\n"),
                Kind::Failed,
            );
        }
    }
    let mut placed: Vec<&str> = Vec::new();
    let mut called: Vec<&str> = Vec::new();
    let mut failed_drawn = false;
    for (k, s) in d.path.iter().enumerate() {
        p.messages.push(Message::Step(k));
        let _ = writeln!(
            p.source,
            "  o{}->>o{}: {}",
            col(d, &s.from),
            col(d, &s.to),
            mermaid_text(&[format!("«{}»", s.verb)])
        );
        if let Some((_, found)) = code
            .into_iter()
            .flat_map(|c| &c.actions)
            .find(|(verb, _)| *verb == s.verb && !placed.contains(&verb.as_str()))
        {
            placed.push(&s.verb);
            card(
                &mut p,
                format!("o{},o{}", col(d, &s.from), col(d, &s.to)),
                found,
            );
        }
        if let Some((_, found)) = code
            .into_iter()
            .flat_map(|c| &c.action_calls)
            .find(|(verb, _)| *verb == s.verb && !called.contains(&verb.as_str()))
        {
            called.push(&s.verb);
            calls(&mut p, d, found);
        }
        if let Some((_, evidence)) = code
            .and_then(|c| c.failed.as_ref())
            .filter(|(verb, _)| *verb == s.verb)
            .filter(|_| !failed_drawn)
        {
            failed_drawn = true;
            note(
                &mut p,
                format!("o{}", col(d, &s.to)),
                evidence.join("\n"),
                Kind::Failed,
            );
        }
        let rules: Vec<String> = d
            .rules(&s.to, &s.verb)
            .into_iter()
            .filter(|rule| rule.checks(&s.to))
            .map(|rule| rule.source())
            .collect();
        if !rules.is_empty() {
            note(
                &mut p,
                format!("o{}", col(d, &s.to)),
                rules.join("\n"),
                Kind::Rule,
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

fn node(out: &mut String, (x, y): (f64, f64), kind: Kind, state: &str) {
    let top = stereotype(kind);
    let lines = lines(state);
    let wide = lines
        .iter()
        .map(|l| width(l, 13.0) * 1.06)
        .fold(0.0, f64::max);
    let w = width(top, 11.5).max(wide) + 24.0;
    let h = 26.0 + lines.len() as f64 * 16.0;
    let dash = match kind {
        Kind::Initial => r#" stroke-dasharray="4 3""#,
        _ => "",
    };
    let _ = write!(
        out,
        r#"<rect x="{:.1}" y="{:.1}" width="{w:.1}" height="{h:.1}" rx="6" style="fill:{SURFACE};stroke:{BORDER}"{dash}/>"#,
        x - w / 2.0,
        y - h / 2.0
    );
    let top_y = y - h / 2.0;
    write_text(out, x, top_y + 17.0, top, MUTED, 400, 11.5);
    for (i, line) in lines.iter().enumerate() {
        write_text(out, x, top_y + 34.0 + i as f64 * 16.0, line, FG, 600, 13.0);
    }
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
        let Some(s) = d.path.get(k) else {
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
            Note::Card(card) => {
                let (w, h) = (card.cols as f64 * CHAR, card.rows as f64 * ROW);
                let _ = write!(
                    out,
                    r#"<foreignObject x="{:.1}" y="{:.1}" width="{w:.1}" height="{h:.1}"><div xmlns="http://www.w3.org/1999/xhtml">{}</div></foreignObject>"#,
                    x - w / 2.0,
                    y - h / 2.0,
                    card.html
                );
            }
            Note::Text(_, Kind::Initial) => {
                for (i, state) in &plan.initial {
                    node(&mut out, (column[*i], y), Kind::Initial, state);
                }
            }
            Note::Text(text, Kind::Failed) => failure(&mut out, (x, y), text),
            Note::Text(text, kind) => node(&mut out, (x, y), *kind, text),
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
  browser "page" snapshot="snap_0" {
    on "click" snapshot="snap_1"
  }
  element "origin" value="San Diego" {
    on "type" value="Lahore, Pakistan, Lahore, Pakistan, Lahore, Pakistan"
    invariant "shown" {
      neq "origin.value" ""
    }
  }
}
action {
  type from="agent" to="origin"
  click from="agent" to="page"
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
style "person" icon="user"
style "browser" icon="app-window"
style "element" icon="square-dashed-mouse-pointer""#;

    fn diagram() -> Diagram {
        weft_parse::parse(DIAGRAM, &weft_parse::config(CONFIG).unwrap()).unwrap()
    }

    fn drawn() -> String {
        svg(&diagram(), None, &Code::default()).unwrap()
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
