use std::{cmp::Reverse, ffi::OsStr, fs, iter, path::Path};

use weft_draw::{Card, Code};
use weft_parse::{Ran, Span, State};
use weft_simulator::Outcome;

use crate::escape;

const RESET: &str = "reset";
const PASS: &str = "circle-check";
const FAIL: &str = "circle-x";
const GUTTER: usize = 8;
const NARROWEST: usize = 24;
const WIDEST: usize = 52;
const LINES: usize = 6;

pub fn kinds(ran: &Ran) -> Vec<(&str, &[Span])> {
    iter::once((RESET, ran.reset.as_slice()))
        .chain(ran.actions.iter().map(|(v, s)| (v.as_str(), s.as_slice())))
        .collect()
}

pub fn size(s: &Span) -> usize {
    s.to + 1 - s.from
}

fn place(s: &Span) -> String {
    match size(s) {
        1 => format!("{}:{}", s.file, s.from),
        _ => format!("{}:{}-{}", s.file, s.from, s.to),
    }
}

fn far(file: &str, path: &str) -> usize {
    let dirs = |p: &str| -> Vec<String> {
        Path::new(p)
            .parent()
            .into_iter()
            .flat_map(Path::components)
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect()
    };
    let (a, b) = (dirs(file), dirs(path));
    let same = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    a.len() + b.len() - 2 * same
}

fn ranked<'a>(spans: &'a [Span], ran: &Ran, path: &str) -> Vec<&'a Span> {
    let all = kinds(ran);
    let shared = |s: &Span| {
        all.iter()
            .filter(|(_, other)| {
                other
                    .iter()
                    .any(|o| o.file == s.file && o.from <= s.to && s.from <= o.to)
            })
            .count()
    };
    let mut out: Vec<&Span> = spans.iter().collect();
    out.sort_by_cached_key(|s| {
        (
            shared(s),
            far(&s.file, path),
            Reverse(size(s)),
            s.file.clone(),
            s.from,
        )
    });
    out
}

fn snippet(s: &Span) -> Result<Option<(usize, String)>, String> {
    let text = fs::read_to_string(&s.file).map_err(|e| format!("`ran` names `{}`: {e}", s.file))?;
    let lines: Vec<&str> = text.lines().skip(s.from - 1).take(size(s)).collect();
    if lines.len() != size(s) {
        return Err(format!(
            "`ran` names `{}`, and the file has {} lines",
            place(s),
            text.lines().count()
        ));
    }
    let indent = |l: &&str| l.len() - l.trim_start().len();
    let first = lines
        .iter()
        .enumerate()
        .filter(|(_, l)| l.chars().any(char::is_alphabetic))
        .min_by_key(|(_, l)| indent(l))
        .map(|(i, _)| i);
    Ok(first.map(|first| {
        let shown = &lines[first..lines.len().min(first + LINES)];
        let shown = &shown[..shown
            .iter()
            .rposition(|l| !l.trim().is_empty())
            .map_or(0, |i| i + 1)];
        let cut = shown
            .iter()
            .filter(|l| !l.trim().is_empty())
            .map(indent)
            .min()
            .unwrap_or(0);
        let code: Vec<String> = shown
            .iter()
            .map(|l| l.get(cut..).unwrap_or_default().replace('\t', "  "))
            .collect();
        (s.from + first, code.join("\n"))
    }))
}

fn wrapped(path: &str, cols: usize) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for part in path.split_inclusive('/') {
        match lines.last_mut() {
            Some(last) if last.chars().count() + part.chars().count() <= cols => {
                last.push_str(part)
            }
            _ => lines.push(part.to_string()),
        }
    }
    lines
}

fn card(spans: &[Span], ran: &Ran, path: &str) -> Result<Option<Card>, String> {
    ranked(spans, ran, path)
        .into_iter()
        .find_map(|s| snippet(s).map(|found| found.map(|f| (s, f))).transpose())
        .transpose()
        .map(|found| {
            found.map(|(s, (from, code))| {
                let cols = code
                    .lines()
                    .map(|l| l.chars().count() + GUTTER)
                    .max()
                    .unwrap_or(0)
                    .clamp(NARROWEST, WIDEST);
                let caption = wrapped(&s.file, cols);
                Card {
                    rows: code.lines().count() + caption.len() + 1,
                    cols,
                    html: format!(
                        r#"<figure><figcaption>{}</figcaption><pre data-lang="{}" data-from="{from}">{}</pre></figure>"#,
                        caption
                            .iter()
                            .map(|l| escape(l))
                            .collect::<Vec<_>>()
                            .join("<br>"),
                        Path::new(&s.file)
                            .extension()
                            .and_then(OsStr::to_str)
                            .filter(|e| e.chars().all(char::is_alphanumeric))
                            .unwrap_or("text"),
                        escape(&code)
                    ),
                }
            })
        })
}

pub fn code(ran: &Ran, path: &str) -> Result<Code, String> {
    Ok(Code {
        reset: card(&ran.reset, ran, path)?,
        actions: ran
            .actions
            .iter()
            .map(|(verb, spans)| Ok(card(spans, ran, path)?.map(|c| (verb.clone(), c))))
            .collect::<Result<Vec<_>, String>>()?
            .into_iter()
            .flatten()
            .collect(),
    })
}

fn row(name: &str, value: &str) -> String {
    format!(
        r#"<tr><td class="text-light">{}</td><td>{value}</td></tr>"#,
        escape(name)
    )
}

fn state(now: &State, before: Option<&State>) -> String {
    now.iter()
        .flat_map(|(object, pairs)| {
            pairs.iter().map(move |(key, value)| {
                let was = before.and_then(|b| b.get(object)?.get(key));
                let shown = escape(&value.to_string());
                row(
                    &format!("{object}.{key}"),
                    &match was.is_some_and(|w| w != value) {
                        true => format!("<mark>{shown}</mark>"),
                        false => shown,
                    },
                )
            })
        })
        .collect()
}

pub fn panel(o: &Outcome) -> Result<String, String> {
    let icon =
        |name: &str| weft_draw::icon(name).ok_or_else(|| format!("`{name}` is not a Lucide icon"));
    let (pass, fail) = (icon(PASS)?, icon(FAIL)?);
    let last = o.trace.len().saturating_sub(1);
    let mut steps: Vec<String> = o
        .trace
        .iter()
        .enumerate()
        .map(|(k, l)| {
            let (class, drawn) = match o.failed && k == last {
                true => (r#" class="e""#, fail),
                false => ("", pass),
            };
            let label = match k {
                0 => "Start".to_string(),
                k => format!("Step {k}"),
            };
            let action = l.action.as_ref().map_or(String::new(), |(verb, object)| {
                row("step", &format!("{} {}", escape(verb), escape(object)))
            });
            let input = match l.input.is_empty() {
                true => String::new(),
                false => row("input", &escape(&l.input)),
            };
            let check = match l.evidence.is_empty() {
                true => "true".to_string(),
                false => l
                    .evidence
                    .iter()
                    .map(|e| escape(e))
                    .collect::<Vec<_>>()
                    .join("<br>"),
            };
            format!(
                r#"<details{class}><summary><span class="hstack gap-2"><svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">{drawn}</svg>{label}</span></summary><div class="table"><table><tbody>{action}{input}{}{}</tbody></table></div></details>"#,
                state(&l.state, k.checked_sub(1).map(|b| &o.trace[b].state)),
                row("check", &check)
            )
        })
        .collect();
    steps.rotate_right(usize::from(o.failed));
    Ok(format!("<aside>{}</aside>", steps.concat()))
}
