use std::fmt::Write;

use merman::ascii::AsciiRenderOptions;
use merman::{AsciiRequest, OperationControl, RenderOutput, RenderRequest, Renderer, SvgRequest};
use weft_parse::{Diagram, Type, Verdict, record};

const KEEP: usize = 24;
const MACHINE: &str = "rgb(243, 244, 246)";
const INIT: &str = "rgb(232, 240, 252)";
const FAIL: &str = "rgb(253, 228, 225)";
const HOLD: &str = "rgb(226, 244, 230)";

fn text(s: &str) -> String {
    let lines: Vec<String> = match record(s) {
        Some(r) => [r.caption]
            .into_iter()
            .filter(|c| !c.is_empty())
            .chain(
                r.fields
                    .iter()
                    .map(|(k, v)| match v.char_indices().nth(KEEP) {
                        Some((i, _)) => format!("{k}: {}…", &v[..i]),
                        None => format!("{k}: {v}"),
                    }),
            )
            .collect(),
        None => s.split('\n').map(String::from).collect(),
    };
    lines
        .join("<br/>")
        .replace('#', "#35;")
        .replace(';', "#59;")
        .replace("#35#59;", "#35;")
}

fn shape(ty: &Type) -> &'static str {
    match ty {
        Type::Person { .. } => "actor",
        Type::Api { .. } => "boundary",
        Type::Page { .. } => "participant",
        Type::Code { .. } => "control",
        Type::Db { .. } => "database",
        Type::File { .. } => "collections",
        Type::Browser { .. } => "entity",
    }
}

fn id(d: &Diagram, name: &str) -> String {
    format!(
        "o{}",
        d.objects.iter().position(|o| o.name == name).unwrap_or(0)
    )
}

fn span(d: &Diagram, names: &[&str]) -> String {
    let mut cols: Vec<usize> = names
        .iter()
        .filter_map(|n| d.objects.iter().position(|o| o.name == *n))
        .collect();
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

fn rect(out: &mut String, color: &str, notes: &[(String, String)]) {
    if notes.is_empty() {
        return;
    }
    let _ = writeln!(out, "  rect {color}");
    for (over, note) in notes {
        let _ = writeln!(out, "  Note over {over}: {note}");
    }
    let _ = writeln!(out, "  end");
}

pub fn mermaid(d: &Diagram) -> String {
    let mut out = String::from("sequenceDiagram\n  autonumber\n");
    for (i, o) in d.objects.iter().enumerate() {
        let _ = match &o.ty {
            Type::Person { name } => writeln!(out, "  actor o{i} as {}", text(name)),
            ty => writeln!(
                out,
                "  participant o{i}@{{ \"type\": \"{}\" }} as {}<br/>[{}]",
                shape(ty),
                text(&ty.label()),
                text(&ty.tag())
            ),
        };
    }
    let machines: Vec<(String, String)> = d
        .objects
        .iter()
        .filter(|o| !o.machine.is_empty())
        .map(|o| {
            let lines: Vec<String> = o
                .machine
                .iter()
                .map(|(a, b)| format!("{} → {}", text(a), text(b)))
                .collect();
            (
                id(d, &o.name),
                format!("state machine<br/>{}", lines.join("<br/>")),
            )
        })
        .collect();
    rect(&mut out, MACHINE, &machines);
    let init: Vec<(String, String)> = d
        .init
        .iter()
        .map(|a| (id(d, &a.object), text(&a.value)))
        .collect();
    rect(&mut out, INIT, &init);
    let verdict = d.verdict();
    for (k, s) in d.steps.iter().enumerate() {
        let failed = matches!(&verdict, Some(Verdict::Fails { step, .. }) if *step == k);
        if failed {
            let _ = writeln!(out, "  rect {FAIL}");
        }
        let label = if s.text.is_empty() {
            " ".to_string()
        } else {
            text(&s.text)
        };
        let _ = writeln!(out, "  {}->>{}: {label}", id(d, &s.from), id(d, &s.to));
        for a in &s.sets {
            let _ = writeln!(out, "  Note over {}: {}", id(d, &a.object), text(&a.value));
        }
        if failed {
            let _ = writeln!(out, "  end");
        }
    }
    match &verdict {
        Some(Verdict::Fails {
            step,
            objects,
            reason,
        }) => {
            let names: Vec<&str> = objects.iter().map(String::as_str).collect();
            let note = text(&format!("✕ {reason} · fails at step {}", step + 1));
            rect(&mut out, FAIL, &[(span(d, &names), note)]);
        }
        Some(Verdict::Holds) => {
            let title = d
                .check
                .as_ref()
                .map_or("Each state change follows its state machine", |c| {
                    c.title.as_str()
                });
            let names: Vec<&str> = d.objects.iter().map(|o| o.name.as_str()).collect();
            rect(
                &mut out,
                HOLD,
                &[(span(d, &names), text(&format!("✓ {title} · holds")))],
            );
        }
        None => {}
    }
    out
}

fn render(request: RenderRequest<'_>) -> Result<RenderOutput, String> {
    Renderer::new().render(request).map_err(|e| e.to_string())
}

pub fn svg(d: &Diagram) -> Result<String, String> {
    let source = mermaid(d);
    match render(RenderRequest::svg(
        &source,
        OperationControl::new(),
        SvgRequest::default(),
    ))? {
        RenderOutput::Svg(Some(svg)) => Ok(svg.svg().to_string()),
        _ => Err("merman drew no diagram".into()),
    }
}

pub fn ascii(d: &Diagram) -> Result<String, String> {
    let source = mermaid(d);
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
