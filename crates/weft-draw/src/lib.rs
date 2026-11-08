mod vendor;

use std::fmt::Write;

use merman::ascii::AsciiRenderOptions;
use merman::{AsciiRequest, OperationControl, RenderOutput, RenderRequest, Renderer, SvgRequest};
use serde_json::Value;
use vendor::lucide;
use weft_parse::{Data, Diagram, Type, Verdict};

const KEEP: usize = 28;
const RESERVE: usize = 3;
const FONT: &str = "Inter, ui-sans-serif, system-ui, -apple-system, 'Segoe UI', sans-serif";

#[derive(Clone, Copy)]
pub enum Theme {
    Light,
    Dark,
}

struct Palette {
    bg: &'static str,
    fg: &'static str,
    muted: &'static str,
    surface: &'static str,
    border: &'static str,
    critical: &'static str,
    machine: &'static str,
    init: &'static str,
    fail: &'static str,
    hold: &'static str,
}

const LIGHT: Palette = Palette {
    bg: "#ffffff",
    fg: "#1f1f1f",
    muted: "#6e6e6e",
    surface: "#f3f3f3",
    border: "#8a8a8a",
    critical: "#e45756",
    machine: "#f7f7f7",
    init: "#e8ecf8",
    fail: "#fbe4e3",
    hold: "#ececec",
};

const DARK: Palette = Palette {
    bg: "#161616",
    fg: "#e6e6e6",
    muted: "#9a9a9a",
    surface: "#262626",
    border: "#6e6e6e",
    critical: "#f06b6a",
    machine: "#1d1d1d",
    init: "#222a3d",
    fail: "#3d2222",
    hold: "#2a2a2a",
};

#[derive(Clone, Copy)]
enum Fill {
    Machine,
    Init,
    Fail,
    Hold,
}

#[derive(Clone)]
struct Line {
    text: String,
    strong: bool,
}

struct Plan {
    source: String,
    heads: Vec<Vec<Line>>,
    labels: Vec<Vec<Line>>,
    notes: Vec<Vec<Line>>,
    rects: Vec<Fill>,
    failed: Option<usize>,
}

fn plain(text: impl Into<String>) -> Line {
    Line {
        text: text.into(),
        strong: false,
    }
}

fn strong(text: impl Into<String>) -> Line {
    Line {
        text: text.into(),
        strong: true,
    }
}

fn cut(s: &str) -> String {
    match s.char_indices().nth(KEEP) {
        Some((i, _)) => format!("{}…", &s[..i]),
        None => s.to_string(),
    }
}

fn field(k: &str, v: &Data) -> Line {
    plain(format!("{k}: {}", cut(&v.flat())))
}

fn lines(d: &Data) -> Vec<Line> {
    match d {
        Data::Record(fields) => match fields.as_slice() {
            [(caption, Data::Record(inner))] => [strong(caption.clone())]
                .into_iter()
                .chain(inner.iter().map(|(k, v)| field(k, v)))
                .collect(),
            _ => fields.iter().map(|(k, v)| field(k, v)).collect(),
        },
        other => other.flat().split('\n').map(|l| plain(cut(l))).collect(),
    }
}

fn mermaid_text(lines: &[Line]) -> String {
    let parts: Vec<String> = lines
        .iter()
        .map(|l| match l.text.trim() {
            "" => "\u{a0}".to_string(),
            t => t
                .replace('#', "#35;")
                .replace(';', "#59;")
                .replace("#35#59;", "#35;"),
        })
        .collect();
    parts.join("<br/>")
}

fn head(ty: &Type) -> Vec<Line> {
    let fields = ty
        .fields()
        .into_iter()
        .map(|(k, v)| plain(format!("{k}: {}", cut(v))));
    [strong(ty.kind())].into_iter().chain(fields).collect()
}

fn col(d: &Diagram, name: &str) -> usize {
    d.objects.iter().position(|o| o.name == name).unwrap_or(0)
}

fn span(d: &Diagram, names: &[&str]) -> String {
    let mut cols: Vec<usize> = names.iter().map(|n| col(d, n)).collect();
    cols.sort_unstable();
    let (lo, hi) = (
        cols.first().copied().unwrap_or(0),
        cols.last().copied().unwrap_or(d.objects.len() - 1),
    );
    if lo == hi {
        format!("o{lo}")
    } else {
        format!("o{lo},o{hi}")
    }
}

fn rect(p: &mut Plan, fill: Fill, notes: Vec<(String, Vec<Line>)>) {
    if notes.is_empty() {
        return;
    }
    let _ = writeln!(p.source, "  rect rgb(0, 0, 0)");
    p.rects.push(fill);
    for (over, lines) in notes {
        let _ = writeln!(p.source, "  Note over {over}: {}", mermaid_text(&lines));
        p.notes.push(lines);
    }
    let _ = writeln!(p.source, "  end");
}

fn machine(o: &weft_parse::Object) -> Vec<Line> {
    let mut states: Vec<(&str, Vec<&str>)> = Vec::new();
    for (a, b) in &o.machine {
        match states.iter_mut().find(|(s, _)| s == a) {
            Some((_, next)) => next.push(b),
            None => states.push((a, vec![b])),
        }
    }
    let rows = states
        .into_iter()
        .map(|(a, next)| plain(format!("{a}: {}", next.join(", "))));
    [strong("states")].into_iter().chain(rows).collect()
}

fn plan(d: &Diagram, reserve: bool) -> Plan {
    let mut p = Plan {
        source: String::from("sequenceDiagram\n  autonumber\n"),
        heads: Vec::new(),
        labels: Vec::new(),
        notes: Vec::new(),
        rects: Vec::new(),
        failed: None,
    };
    for (i, o) in d.objects.iter().enumerate() {
        let lines = head(&o.ty);
        let pad = if reserve {
            vec![plain(""); RESERVE]
        } else {
            Vec::new()
        };
        let _ = writeln!(
            p.source,
            "  participant o{i} as {}",
            mermaid_text(&[pad, lines.clone()].concat())
        );
        p.heads.push(lines);
    }
    let machines = d
        .objects
        .iter()
        .filter(|o| !o.machine.is_empty())
        .map(|o| (format!("o{}", col(d, &o.name)), machine(o)))
        .collect();
    rect(&mut p, Fill::Machine, machines);
    let init = d
        .init
        .iter()
        .map(|(k, v)| (format!("o{}", col(d, k)), lines(v)))
        .collect();
    rect(&mut p, Fill::Init, init);
    let verdict = d.verdict();
    if let Some(Verdict::Fails { step, .. }) = &verdict {
        p.failed = Some(*step);
    }
    for (k, s) in d.steps.iter().enumerate() {
        let failed = p.failed == Some(k);
        if failed {
            let _ = writeln!(p.source, "  rect rgb(0, 0, 0)");
            p.rects.push(Fill::Fail);
        }
        let label = s
            .send
            .as_ref()
            .map(lines)
            .unwrap_or_else(|| vec![plain("")]);
        let _ = writeln!(
            p.source,
            "  o{}->>o{}: {}",
            col(d, &s.from),
            col(d, &s.to),
            mermaid_text(&label)
        );
        p.labels.push(label);
        for (object, value) in &s.set {
            let lines = lines(value);
            let _ = writeln!(
                p.source,
                "  Note over o{}: {}",
                col(d, object),
                mermaid_text(&lines)
            );
            p.notes.push(lines);
        }
        if failed {
            let _ = writeln!(p.source, "  end");
        }
    }
    match &verdict {
        Some(Verdict::Fails {
            step,
            objects,
            reason,
        }) => {
            let names: Vec<&str> = objects.iter().map(String::as_str).collect();
            let note = strong(format!("✕ {reason} · fails at step {}", step + 1));
            rect(&mut p, Fill::Fail, vec![(span(d, &names), vec![note])]);
        }
        Some(Verdict::Holds) => {
            let names: Vec<&str> = d.checks.iter().map(|c| c.name.as_str()).collect();
            let title = if names.is_empty() {
                "Each state follows its state machine".into()
            } else {
                names.join(" · ")
            };
            let all: Vec<&str> = d.objects.iter().map(|o| o.name.as_str()).collect();
            rect(
                &mut p,
                Fill::Hold,
                vec![(span(d, &all), vec![strong(format!("✓ {title} · holds"))])],
            );
        }
        None => {}
    }
    p
}

fn render(request: RenderRequest<'_>) -> Result<RenderOutput, String> {
    Renderer::new().render(request).map_err(|e| e.to_string())
}

fn layout(source: &str) -> Result<Value, String> {
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
    Ok(value["layout"]["SequenceDiagram"].clone())
}

fn num(v: &Value, k: &str) -> f64 {
    v[k].as_f64().unwrap_or(0.0)
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn icon(ty: &Type) -> &'static str {
    match ty {
        Type::Person(_) => lucide::USER,
        Type::Api(_) => lucide::GLOBE,
        Type::Page(_) => lucide::APP_WINDOW,
        Type::Code(_) => lucide::CODE,
        Type::Db(_) => lucide::DATABASE,
        Type::File(_) => lucide::FILE,
        Type::Browser(_) => lucide::ROTATE_CCW,
    }
}

fn text(out: &mut String, x: f64, y: f64, line: &Line, color: &str) {
    let weight = if line.strong { 600 } else { 400 };
    let _ = write!(
        out,
        r#"<text x="{x:.1}" y="{y:.1}" text-anchor="middle" font-weight="{weight}" fill="{color}">{}</text>"#,
        escape(&line.text)
    );
}

fn block(out: &mut String, x: f64, top: f64, height: f64, lines: &[Line], color: &str) {
    let first = top + (height - 16.0 * lines.len() as f64) / 2.0 + 12.0;
    for (k, l) in lines.iter().enumerate() {
        text(out, x, first + 16.0 * k as f64, l, color);
    }
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

pub fn svg(d: &Diagram, theme: Theme) -> Result<String, String> {
    let p = match theme {
        Theme::Light => &LIGHT,
        Theme::Dark => &DARK,
    };
    let plan = plan(d, true);
    let l = layout(&plan.source)?;
    let nodes = l["nodes"].as_array().cloned().unwrap_or_default();
    let edges = l["edges"].as_array().cloned().unwrap_or_default();
    let b = &l["bounds"];
    let (x0, y0) = (num(b, "min_x"), num(b, "min_y"));
    let (w, h) = (num(b, "max_x") - x0, num(b, "max_y") - y0);
    let mut out = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="{x0:.1} {y0:.1} {w:.1} {h:.1}" width="{w:.0}" height="{h:.0}" font-family="{FONT}" font-size="13"><rect x="{x0:.1}" y="{y0:.1}" width="{w:.1}" height="{h:.1}" fill="{}"/>"#,
        p.bg
    );
    for (n, fill) in sorted(&nodes, "rect-").into_iter().zip(&plan.rects) {
        let color = match fill {
            Fill::Machine => p.machine,
            Fill::Init => p.init,
            Fill::Fail => p.fail,
            Fill::Hold => p.hold,
        };
        let (nw, nh) = (num(n, "width"), num(n, "height"));
        let (x, y) = (num(n, "x") - nw / 2.0, num(n, "y") - nh / 2.0);
        let _ = write!(
            out,
            r#"<rect x="{x:.1}" y="{y:.1}" width="{nw:.1}" height="{nh:.1}" rx="6" fill="{color}"/>"#
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
    for (k, e) in sorted(&edges, "msg-").into_iter().enumerate() {
        let pts = points(e);
        let [(ax, ay), .., (zx, zy)] = pts.as_slice() else {
            continue;
        };
        let (px, py) = &pts[pts.len() - 2];
        let path: Vec<String> = pts.iter().map(|(x, y)| format!("{x:.1},{y:.1}")).collect();
        let dash = if e["stroke_dasharray"].is_null() {
            ""
        } else {
            r#" stroke-dasharray="4 3""#
        };
        let _ = write!(
            out,
            r#"<polyline points="{}" fill="none" stroke="{}" stroke-width="1.25"{dash}/>"#,
            path.join(" "),
            p.fg
        );
        let len = ((zx - px).powi(2) + (zy - py).powi(2)).sqrt().max(1.0);
        let (ux, uy) = ((zx - px) / len, (zy - py) / len);
        let (bx, by) = (zx - ux * 8.0, zy - uy * 8.0);
        let _ = write!(
            out,
            r#"<polygon points="{zx:.1},{zy:.1} {:.1},{:.1} {:.1},{:.1}" fill="{}"/>"#,
            bx - uy * 4.0,
            by + ux * 4.0,
            bx + uy * 4.0,
            by - ux * 4.0,
            p.fg
        );
        let label = &e["label"];
        if let Some(lines) = plan.labels.get(k)
            && label.is_object()
        {
            block(
                &mut out,
                num(label, "x"),
                num(label, "y"),
                num(label, "height"),
                lines,
                p.fg,
            );
        }
        let color = if plan.failed == Some(k) {
            p.critical
        } else {
            p.fg
        };
        let _ = write!(
            out,
            r#"<circle cx="{ax:.1}" cy="{ay:.1}" r="8.5" fill="{color}"/><text x="{ax:.1}" y="{:.1}" text-anchor="middle" font-size="10.5" font-weight="700" fill="{}">{}</text>"#,
            ay + 3.8,
            p.bg,
            k + 1
        );
    }
    for (n, lines) in sorted(&nodes, "note-").into_iter().zip(&plan.notes) {
        let (nw, nh) = (num(n, "width"), num(n, "height"));
        let (x, top) = (num(n, "x"), num(n, "y") - nh / 2.0);
        let _ = write!(
            out,
            r#"<rect x="{:.1}" y="{top:.1}" width="{nw:.1}" height="{nh:.1}" rx="4" fill="{}" stroke="{}"/>"#,
            x - nw / 2.0,
            p.surface,
            p.border
        );
        block(&mut out, x, top, nh, lines, p.fg);
    }
    for n in &nodes {
        let id = n["id"].as_str().unwrap_or_default();
        let Some(i) = id
            .strip_prefix("actor-top-o")
            .or_else(|| id.strip_prefix("actor-bottom-o"))
            .and_then(|i| i.parse::<usize>().ok())
        else {
            continue;
        };
        let (Some(o), Some(lines)) = (d.objects.get(i), plan.heads.get(i)) else {
            continue;
        };
        let (x, top) = (num(n, "x"), num(n, "y") - num(n, "height") / 2.0);
        let _ = write!(
            out,
            r#"<rect x="{:.1}" y="{:.1}" width="40" height="40" rx="8" fill="{}" stroke="{}"/><g transform="translate({:.1} {:.1})" fill="none" stroke="{}" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round">{}</g>"#,
            x - 20.0,
            top + 4.0,
            p.surface,
            p.border,
            x - 12.0,
            top + 12.0,
            p.fg,
            icon(&o.ty)
        );
        for (k, l) in lines.iter().enumerate() {
            text(
                &mut out,
                x,
                top + 62.0 + 16.0 * k as f64,
                l,
                if l.strong { p.fg } else { p.muted },
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
