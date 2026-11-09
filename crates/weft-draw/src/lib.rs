mod vendor;

use std::fmt::Write;

use merman::ascii::AsciiRenderOptions;
use merman::{AsciiRequest, OperationControl, RenderOutput, RenderRequest, Renderer, SvgRequest};
use serde_json::Value;
use vendor::lucide::ICONS;
use weft_parse::{Data, Diagram, Verdict};

const VALUE: usize = 36;
const WRAP: usize = 24;
const LINE: f64 = 17.6;
const INDENT: f64 = 12.0;
const GAP: f64 = 14.0;
const PAD: f64 = 10.0;
const RESERVE: usize = 2;
const FONT: &str = "Inter, ui-sans-serif, system-ui, -apple-system, 'Segoe UI', sans-serif";
const DIRECTIVE: &str =
    "%%{init: {\"sequence\": {\"mirrorActors\": false, \"actorMargin\": 16, \"width\": 96}}}%%\n";

#[derive(Clone, Copy)]
pub enum Theme {
    Light,
    Dark,
}

pub struct Palette {
    pub bg: &'static str,
    pub fg: &'static str,
    pub muted: &'static str,
    pub surface: &'static str,
    pub border: &'static str,
    pub critical: &'static str,
    fail: &'static str,
}

const LIGHT: Palette = Palette {
    bg: "#ffffff",
    fg: "#1f1f1f",
    muted: "#6e6e6e",
    surface: "#f6f6f6",
    border: "#c4c4c4",
    critical: "#e45756",
    fail: "#fbe4e3",
};

const DARK: Palette = Palette {
    bg: "#161616",
    fg: "#e6e6e6",
    muted: "#9a9a9a",
    surface: "#202020",
    border: "#4a4a4a",
    critical: "#f06b6a",
    fail: "#3d2222",
};

struct Field {
    depth: usize,
    bullet: bool,
    key: String,
    value: String,
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Initial,
    State,
    Broken,
}

struct Note {
    text: String,
    kind: Kind,
}

struct Plan {
    source: String,
    heads: Vec<Vec<String>>,
    payloads: Vec<Vec<Field>>,
    notes: Vec<Note>,
    initial: Vec<(usize, String)>,
    rects: usize,
    failed: Option<usize>,
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

fn field(out: &mut Vec<Field>, depth: usize, key: &str, value: &str) {
    for (i, line) in wrap(value, VALUE).into_iter().enumerate() {
        out.push(Field {
            depth,
            bullet: false,
            key: if i == 0 {
                key.to_string()
            } else {
                String::new()
            },
            value: line,
        });
    }
}

fn fields(all: &[(String, Data)], depth: usize, out: &mut Vec<Field>) {
    for (k, v) in all {
        let (mut k, mut v) = (k.clone(), v);
        while let Data::Record(f) = v
            && let [(inner, one)] = f.as_slice()
        {
            k = format!("{k}.{inner}");
            v = one;
        }
        let k = k.as_str();
        let texts: Option<Vec<&str>> = match v {
            Data::List(items) => items
                .iter()
                .map(|i| match i {
                    Data::Text(t) => Some(t.as_str()),
                    _ => None,
                })
                .collect(),
            _ => None,
        };
        match (v, texts) {
            (Data::Text(t), _) => field(out, depth, k, t),
            (_, Some(texts)) => field(out, depth, k, &texts.join(", ")),
            (Data::Record(f), _) => {
                field(out, depth, k, "");
                fields(f, depth + 1, out);
            }
            (Data::List(items), _) => {
                field(out, depth, k, "");
                items.iter().for_each(|i| item(i, depth + 1, out));
            }
        }
    }
}

fn item(d: &Data, depth: usize, out: &mut Vec<Field>) {
    let start = out.len();
    match d {
        Data::Text(t) => field(out, depth + 1, "", t),
        Data::Record(f) => fields(f, depth + 1, out),
        Data::List(items) => items.iter().for_each(|i| item(i, depth + 1, out)),
    }
    if let Some(f) = out.get_mut(start) {
        f.bullet = true;
    }
}

fn payload(all: &[(String, Data)]) -> Vec<Field> {
    let mut out = Vec::new();
    fields(all, 0, &mut out);
    out
}

fn column(rows: &[Field], key: impl Fn(&Field) -> f64) -> f64 {
    rows.iter()
        .filter(|f| !f.value.is_empty())
        .map(key)
        .fold(0.0, f64::max)
}

fn plain(rows: &[Field]) -> Vec<String> {
    let width = column(rows, |f| (f.depth * 2 + count(&f.key)) as f64) as usize;
    rows.iter()
        .map(|f| {
            let lead = match f.bullet {
                true => format!("{}- ", "  ".repeat(f.depth - 1)),
                false => "  ".repeat(f.depth),
            };
            let key = format!("{lead}{}", f.key);
            format!("{key:width$}  {}", f.value).trim_end().to_string()
        })
        .collect()
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
    let steps = d.steps.iter().flat_map(|s| {
        [col(d, &s.from), col(d, &s.to)]
            .into_iter()
            .chain(s.set.iter().map(|(o, _)| col(d, o)))
    });
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
        Kind::State | Kind::Broken => "«state»",
    }
}

fn note(p: &mut Plan, over: String, text: String, kind: Kind) {
    let lines = match (p.svg, kind) {
        (true, _) => [vec![stereotype(kind).to_string()], wrap(&text, WRAP)].concat(),
        (false, _) => vec![text.clone()],
    };
    let _ = writeln!(p.source, "  Note over {over}: {}", mermaid_text(&lines));
    p.notes.push(Note { text, kind });
}

fn plan(d: &Diagram, svg: bool) -> Plan {
    let mut p = Plan {
        source: String::new(),
        heads: Vec::new(),
        payloads: Vec::new(),
        notes: Vec::new(),
        initial: Vec::new(),
        rects: 0,
        failed: None,
        svg,
    };
    if svg {
        p.source.push_str(DIRECTIVE);
    }
    p.source.push_str("sequenceDiagram\n  autonumber\n");
    p.heads = d.objects.iter().map(|o| wrap(&o.label, WRAP)).collect();
    let order = order(d);
    for &i in &order {
        let head = &p.heads[i];
        let label = match svg {
            true => [vec![String::new(); RESERVE], head.clone()].concat(),
            false => head.clone(),
        };
        let _ = writeln!(p.source, "  participant o{i} as {}", mermaid_text(&label));
        p.initial
            .extend(d.initial(&d.objects[i].name).map(|s| (i, s.to_string())));
    }
    if let (true, Some(first), Some(last)) =
        (svg && !p.initial.is_empty(), order.first(), order.last())
    {
        let over = format!("o{first},o{last}");
        let widest = p
            .initial
            .iter()
            .map(|(_, s)| s.clone())
            .max_by_key(|s| count(s));
        note(&mut p, over, widest.unwrap_or_default(), Kind::Initial);
    }
    let (failed, marks) = match d.verdict() {
        Some(Verdict::Fails { step, marks, .. }) => (Some(step), marks),
        _ => (None, Vec::new()),
    };
    p.failed = failed;
    for (k, s) in d.steps.iter().enumerate() {
        let here = failed == Some(k);
        if here {
            let _ = writeln!(p.source, "  rect rgb(0, 0, 0)");
            p.rects += 1;
        }
        let rows = payload(&s.send.fields);
        let text: Vec<String> = match svg {
            true => plain(&rows)
                .into_iter()
                .chain([format!("«{}»", s.send.name)])
                .collect(),
            false => vec![
                [format!("«{}»", s.send.name)]
                    .into_iter()
                    .chain(plain(&rows).iter().map(|r| r.trim().to_string()))
                    .collect::<Vec<_>>()
                    .join(" · "),
            ],
        };
        let _ = writeln!(
            p.source,
            "  o{}->>o{}: {}",
            col(d, &s.from),
            col(d, &s.to),
            mermaid_text(&text)
        );
        p.payloads.push(rows);
        for (object, value) in &s.set {
            note(
                &mut p,
                format!("o{}", col(d, object)),
                value.clone(),
                Kind::State,
            );
        }
        if here {
            for m in &marks {
                note(
                    &mut p,
                    format!("o{}", col(d, &m.object)),
                    m.text.clone(),
                    Kind::Broken,
                );
            }
            let _ = writeln!(p.source, "  end");
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

fn icon(name: &str) -> Option<&'static str> {
    ICONS
        .binary_search_by_key(&name, |(n, _)| n)
        .ok()
        .map(|i| ICONS[i].1)
}

pub fn styled(styles: &[(String, String)]) -> Result<(), String> {
    match styles.iter().find(|(_, i)| icon(i).is_none()) {
        Some((kind, i)) => Err(format!("`style {kind}`: `{i}` is not a Lucide icon")),
        None => Ok(()),
    }
}

fn label(out: &mut String, x: f64, y: f64, text: &str, color: &str, weight: u16, size: f64) {
    let _ = write!(
        out,
        r#"<text x="{x:.1}" y="{y:.1}" text-anchor="middle" font-size="{size}" font-weight="{weight}" fill="{color}">{}</text>"#,
        escape(text)
    );
}

fn text(out: &mut String, x: f64, y: f64, text: &str, color: &str) {
    let _ = write!(
        out,
        r#"<text x="{x:.1}" y="{y:.1}" font-size="12.5" fill="{color}" style="white-space:pre">{}</text>"#,
        escape(text)
    );
}

fn tag(out: &mut String, (x, y): (f64, f64), text: &str, ink: &str, bg: &str) {
    let w = width(text, 12.0) + 10.0;
    let _ = write!(
        out,
        r#"<rect x="{:.1}" y="{:.1}" width="{w:.1}" height="19" rx="3" fill="{bg}"/>"#,
        x - w / 2.0,
        y - 9.5
    );
    label(out, x, y + 4.3, text, ink, 400, 12.0);
}

fn node(
    out: &mut String,
    (x, y): (f64, f64),
    kind: Kind,
    state: &str,
    (stroke, ink): (&str, &str),
    p: &Palette,
) {
    let top = stereotype(kind);
    let lines = wrap(state, WRAP);
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
        r#"<rect x="{:.1}" y="{:.1}" width="{w:.1}" height="{h:.1}" rx="4" fill="{}" stroke="{stroke}"{dash}/>"#,
        x - w / 2.0,
        y - h / 2.0,
        p.surface
    );
    let top_y = y - h / 2.0;
    label(out, x, top_y + 17.0, top, p.muted, 400, 11.5);
    for (i, line) in lines.iter().enumerate() {
        label(out, x, top_y + 34.0 + i as f64 * 16.0, line, ink, 700, 13.0);
    }
}

fn arrow(out: &mut String, (px, py): (f64, f64), (zx, zy): (f64, f64), color: &str) {
    let len = ((zx - px).powi(2) + (zy - py).powi(2)).sqrt().max(1.0);
    let (ux, uy) = ((zx - px) / len, (zy - py) / len);
    let (bx, by) = (zx - ux * 8.0, zy - uy * 8.0);
    let _ = write!(
        out,
        r#"<polygon points="{zx:.1},{zy:.1} {:.1},{:.1} {:.1},{:.1}" fill="{color}"/>"#,
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

fn paint_fields(out: &mut String, at: &Value, rows: &[Field], p: &Palette) {
    let key = |f: &Field| f.depth as f64 * INDENT + width(&f.key, 12.5);
    let column = column(rows, key);
    let w = PAD * 2.0
        + rows
            .iter()
            .map(|f| key(f).max(column + GAP + width(&f.value, 12.5)))
            .fold(0.0, f64::max);
    let h = rows.len() as f64 * LINE + 12.0;
    let (x0, top) = (num(at, "x") - w / 2.0, num(at, "y") - 2.0);
    let _ = write!(
        out,
        r#"<rect x="{x0:.1}" y="{top:.1}" width="{w:.1}" height="{h:.1}" rx="4" fill="{}" stroke="{}"/>"#,
        p.surface, p.border
    );
    for (k, f) in rows.iter().enumerate() {
        let (x, y) = (
            x0 + PAD + f.depth as f64 * INDENT,
            top + 19.0 + LINE * k as f64,
        );
        if f.bullet {
            text(out, x - INDENT, y, "–", p.muted);
        }
        text(out, x, y, &f.key, p.muted);
        text(out, x0 + PAD + column + GAP, y, &f.value, p.fg);
    }
}

pub fn svg(d: &Diagram, styles: &[(String, String)], theme: Theme) -> Result<String, String> {
    let p = theme.palette();
    let icons = d
        .objects
        .iter()
        .map(|o| {
            styles
                .iter()
                .find(|(k, _)| *k == o.kind)
                .ok_or_else(|| format!("`{}` has no `style {}`", o.name, o.kind))
                .and_then(|(k, i)| {
                    icon(i).ok_or_else(|| format!("`style {k}`: `{i}` is not a Lucide icon"))
                })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let plan = plan(d, true);
    let l = layout(&plan.source, "SequenceDiagram")?;
    let nodes = l["nodes"].as_array().cloned().unwrap_or_default();
    let edges = l["edges"].as_array().cloned().unwrap_or_default();
    let b = &l["bounds"];
    let (x0, y0) = (num(b, "min_x"), num(b, "min_y"));
    let (w, h) = (num(b, "max_x") - x0, num(b, "max_y") - y0);
    let mut out = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="{x0:.1} {y0:.1} {w:.1} {h:.1}" width="{w:.0}" height="{h:.0}" style="max-width:100%;height:auto" font-family="{FONT}" font-size="13"><rect x="{x0:.1}" y="{y0:.1}" width="{w:.1}" height="{h:.1}" fill="{}"/>"#,
        p.bg
    );
    for n in sorted(&nodes, "rect-").into_iter().take(plan.rects) {
        let (nw, nh) = (num(n, "width"), num(n, "height"));
        let _ = write!(
            out,
            r#"<rect x="{:.1}" y="{:.1}" width="{nw:.1}" height="{nh:.1}" rx="6" fill="{}"/>"#,
            num(n, "x") - nw / 2.0,
            num(n, "y") - nh / 2.0,
            p.fail
        );
    }
    for e in edges.iter().filter(|e| {
        e["id"]
            .as_str()
            .is_some_and(|id| id.starts_with("lifeline-"))
    }) {
        if let [(ax, ay), .., (zx, zy)] = points(e).as_slice() {
            let _ = write!(
                out,
                r#"<line x1="{ax:.1}" y1="{ay:.1}" x2="{zx:.1}" y2="{zy:.1}" stroke="{}" stroke-width="1"/>"#,
                p.border
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
    for (k, e) in sorted(&edges, "msg-").into_iter().enumerate() {
        let pts = points(e);
        let (Some(&(ax, ay)), Some(s)) = (pts.first(), d.steps.get(k)) else {
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
        let failed = plan.failed == Some(k);
        let ink = if failed { p.critical } else { p.fg };
        let _ = write!(
            out,
            r#"<path d="{path}" fill="none" stroke="{ink}" stroke-width="1.25"/>"#
        );
        arrow(&mut out, prev, end, ink);
        if let Some(rows) = plan.payloads.get(k)
            && !rows.is_empty()
            && e["label"].is_object()
        {
            paint_fields(&mut out, &e["label"], rows, p);
        }
        let ink_tag = if failed { p.critical } else { p.muted };
        tag(&mut out, mid, &format!("«{}»", s.send.name), ink_tag, p.bg);
        let _ = write!(
            out,
            r#"<circle cx="{ax:.1}" cy="{ay:.1}" r="8.5" fill="{ink}"/><text x="{ax:.1}" y="{:.1}" text-anchor="middle" font-size="10.5" font-weight="700" fill="{}">{}</text>"#,
            ay + 3.8,
            p.bg,
            k + 1
        );
    }
    for (n, note) in sorted(&nodes, "note-").into_iter().zip(&plan.notes) {
        let (x, y) = (num(n, "x"), num(n, "y"));
        match note.kind {
            Kind::Initial => {
                for (i, state) in &plan.initial {
                    node(
                        &mut out,
                        (column[*i], y),
                        note.kind,
                        state,
                        (p.muted, p.fg),
                        p,
                    );
                }
            }
            Kind::State => node(&mut out, (x, y), note.kind, &note.text, (p.border, p.fg), p),
            Kind::Broken => node(
                &mut out,
                (x, y),
                note.kind,
                &note.text,
                (p.critical, p.critical),
                p,
            ),
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
        let (x, top) = (num(n, "x"), num(n, "y") - num(n, "height") / 2.0);
        label(
            &mut out,
            x,
            top + 12.0,
            &format!("«{}»", o.kind),
            p.muted,
            400,
            11.5,
        );
        let _ = write!(
            out,
            r#"<rect x="{:.1}" y="{:.1}" width="40" height="40" rx="8" fill="{}" stroke="{}"/><g transform="translate({:.1} {:.1})" fill="none" stroke="{}" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round">{drawn}</g>"#,
            x - 20.0,
            top + 20.0,
            p.surface,
            p.border,
            x - 12.0,
            top + 28.0,
            p.fg
        );
        for (k, line) in head.iter().enumerate() {
            label(
                &mut out,
                x,
                top + 76.0 + k as f64 * 16.0,
                line,
                p.fg,
                600,
                13.0,
            );
        }
    }
    out.push_str("</svg>");
    Ok(out)
}

pub fn ascii(d: &Diagram) -> Result<String, String> {
    let source = plan(d, false).source;
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

impl Theme {
    pub fn parse(s: &str) -> Result<Theme, String> {
        match s {
            "light" => Ok(Theme::Light),
            "dark" => Ok(Theme::Dark),
            other => Err(format!("`{other}` is not light or dark")),
        }
    }

    pub fn palette(self) -> &'static Palette {
        match self {
            Theme::Light => &LIGHT,
            Theme::Dark => &DARK,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LABELS: [&str; 6] = [
        "/sandbox/www.google.com/R9HTtq0IEYLMLIPOdYMMr",
        "input[aria-label^=\"Where from?\"]",
        "POST /api/correlation",
        "refunded jordan the full $125 on #102, copper downgraded to starter before the renewal so they shouldnt have been charged. closing it out",
        "Agent",
        "R9HTtq0IEYLMLIPOdYMMrR9HTtq0IEYLMLIPOdYMMr",
    ];

    const DIAGRAM: &str = r#"
setup {
  person agent Agent
  browser page "/sandbox/www.google.com/R9HTtq0IEYLMLIPOdYMMr" {
    state "search form"
    state "Lahore, Pakistan, Lahore, Pakistan, Lahore, Pakistan"
    initial "search form"
    on click from=agent to=origin next="Lahore, Pakistan, Lahore, Pakistan, Lahore, Pakistan"
  }
  element origin "input[aria-label^=\"Where from?\"] input[aria-label^=\"Where to?\"]" {
    state "San Diego"
    state "Lahore, Pakistan"
    initial "San Diego"
    on click from=agent to=origin next="Lahore, Pakistan"
  }
}
action {
  click from=agent to=origin {
    message "refunded jordan the full $125 on #102, copper downgraded to starter before the renewal so they shouldnt have been charged. closing it out"
  }
}
check {
  invariant OriginEqualsSanDiego {
    forall value origin {
      eq value "San Diego"
    }
  }
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
    fn each_label_wraps_to_two_lines_and_keeps_each_word() {
        for s in LABELS {
            for max in [VALUE, WRAP] {
                let lines = wrap(s, max);
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
    }

    #[test]
    fn each_text_is_inside_the_diagram() {
        let d = weft_parse::parse(DIAGRAM).unwrap();
        let styles = [
            ("person", "user"),
            ("browser", "app-window"),
            ("element", "square-dashed-mouse-pointer"),
        ]
        .map(|(k, i)| (k.to_string(), i.to_string()));
        let out = svg(&d, &styles, Theme::Light).unwrap();
        let head = &out[..out.find('>').unwrap()];
        assert!(head.contains(r#"style="max-width:100%;height:auto""#));
        let view: Vec<f64> = attr(head, "viewBox")
            .unwrap()
            .split(' ')
            .map(|v| v.parse().unwrap())
            .collect();
        let plan = plan(&d, true);
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
