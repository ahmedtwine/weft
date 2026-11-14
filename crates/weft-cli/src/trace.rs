use std::{cmp::Reverse, collections::BTreeSet, ffi::OsStr, fs, iter, path::Path};

use serde_json::{Value as Json, json};
use weft_draw::{Call, Card, Code};
use weft_parse::{Diagram, Ran, Span, State};
use weft_simulator::{Hop, Outcome};

use crate::escape;

const START: &str = "start";
const PASS: &str = "circle-check";
const FAIL: &str = "circle-x";
const GUTTER: usize = 8;
const NARROWEST: usize = 24;
const WIDEST: usize = 52;
const LINES: usize = 6;
const SHORT: usize = 32;

pub fn kinds(ran: &Ran) -> Vec<(&str, &[Span])> {
    iter::once((START, ran.start.as_slice()))
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

fn fit(line: &str) -> String {
    let room = WIDEST - GUTTER;
    match line.chars().count() > room {
        true => line.chars().take(room - 1).chain(['…']).collect(),
        false => line.to_string(),
    }
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
            .map(|l| fit(&l.get(cut..).unwrap_or_default().replace('\t', "  ")))
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

fn summary(value: &Json) -> String {
    let many = |n: usize, one: &str| match n {
        1 => format!("1 {one}"),
        n => format!("{n} {one}s"),
    };
    match value {
        Json::Array(all) => many(all.len(), "item"),
        Json::Object(all) => many(all.len(), "key"),
        Json::String(s) if s.is_empty() => "\"\"".to_string(),
        Json::String(s) => {
            let first = s
                .lines()
                .find(|l| !l.trim().is_empty())
                .unwrap_or_default()
                .trim();
            match (first.chars().count() > SUMMARY, s.lines().count() > 1) {
                (true, _) => first.chars().take(SUMMARY - 1).chain(['…']).collect(),
                (false, true) => format!("{first} …"),
                (false, false) => first.to_string(),
            }
        }
        other => other.to_string(),
    }
}

pub fn code(d: &Diagram, o: &Outcome, path: &str) -> Result<Code, String> {
    let ran = &o.ran;
    let mut action_calls: Vec<(String, Vec<Call>)> = Vec::new();
    for l in o.trace.iter().skip(1) {
        if let Some((verb, _)) = &l.action
            && !action_calls.iter().any(|(v, _)| v == verb)
        {
            action_calls.push((verb.clone(), calls(&l.hops)));
        }
    }
    let failed = o.failed.then(|| o.trace.last()).flatten().map(|l| {
        (
            l.action
                .as_ref()
                .map_or(START.to_string(), |(verb, _)| verb.clone()),
            brief(&l.evidence).pop().into_iter().collect(),
        )
    });
    let initial = o
        .trace
        .first()
        .map(|l| {
            d.objects
                .iter()
                .filter_map(|object| {
                    let keys = l.state.get(&object.name)?;
                    let text: Vec<String> = keys
                        .iter()
                        .map(|(k, v)| format!("{k} = {}", summary(v)))
                        .collect();
                    (!text.is_empty()).then(|| (object.name.clone(), text.join("\n")))
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(Code {
        initial,
        failed,
        start_calls: o.trace.first().map_or(Vec::new(), |l| calls(&l.hops)),
        action_calls,
        start: card(&ran.start, ran, path)?,
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

fn at<'a>(state: &'a State, object: &str, key: &str) -> Option<&'a Json> {
    state.get(object)?.get(key)
}

fn cell(value: &Json, changed: bool) -> String {
    let shown = match value {
        Json::String(s) if s.is_empty() => "\"\"".to_string(),
        Json::String(s) if s.chars().count() <= SHORT => escape(s),
        Json::String(_) | Json::Array(_) | Json::Object(_) => format!(
            r#"<pretty-json expand="0" truncate-string="{SHORT}">{}</pretty-json>"#,
            escape(&value.to_string())
        ),
        other => other.to_string(),
    };
    match changed {
        true => format!("<td><mark>{shown}</mark></td>"),
        false => format!("<td>{shown}</td>"),
    }
}

pub fn panel(d: &Diagram, o: &Outcome) -> Result<String, String> {
    let named = checked(d);
    let icon =
        |name: &str| weft_draw::icon(name).ok_or_else(|| format!("`{name}` is not a Lucide icon"));
    let (pass, fail) = (icon(PASS)?, icon(FAIL)?);
    let last = o.trace.len().saturating_sub(1);
    let keys: BTreeSet<(&String, &String)> = o
        .trace
        .iter()
        .flat_map(|l| &l.state)
        .flat_map(|(object, pairs)| pairs.keys().map(move |key| (object, key)))
        .filter(|(object, key)| {
            named.is_empty() || named.contains(&(object.to_string(), key.to_string()))
        })
        .collect();
    let head: String = keys
        .iter()
        .map(|(object, key)| format!("<th>{}</th>", escape(&format!("{object}.{key}"))))
        .collect();
    let rows: String = o
        .trace
        .iter()
        .enumerate()
        .map(|(k, l)| {
            let (class, drawn) = match o.failed && k == last {
                true => (r#" class="e""#, fail),
                false => ("", pass),
            };
            let inputs: Vec<&Json> = l.input.iter().filter(|i| !i.is_null()).collect();
            let input = match inputs.as_slice() {
                [] => "<td></td>".to_string(),
                [one] => cell(one, false),
                many => cell(&json!(many), false),
            };
            let before = k.checked_sub(1).map(|b| &o.trace[b].state);
            let state: String = keys
                .iter()
                .map(|(object, key)| {
                    match at(&l.state, object, key) {
                        Some(value) => cell(
                            value,
                            before.is_some_and(|before| at(before, object, key) != Some(value)),
                        ),
                        None => "<td></td>".to_string(),
                    }
                })
                .collect();
            format!(
                r#"<tr{class}><td><svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">{drawn}</svg> {k}</td><td>{}</td>{input}{state}<td>{}</td></tr>"#,
                escape(&l.step()),
                brief(&l.evidence)
                    .iter()
                    .map(|e| escape(e))
                    .collect::<Vec<_>>()
                    .join("<br>")
            )
        })
        .collect();
    Ok(format!(
        r#"<div class="table t"><table><thead><tr><th>step</th><th>action</th><th>input</th>{head}<th>check</th></tr></thead><tbody>{rows}</tbody></table></div>"#
    ))
}

fn parts(text: &str) -> Vec<String> {
    let inner = text
        .split_once('(')
        .map_or(text, |(_, rest)| rest.strip_suffix(')').unwrap_or(rest));
    let (mut out, mut depth, mut now) = (Vec::new(), 0, String::new());
    for c in inner.chars() {
        match c {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth -= 1,
            ',' if depth == 0 => {
                out.push(now.trim().to_string());
                now.clear();
                continue;
            }
            _ => {}
        }
        now.push(c);
    }
    out.push(now.trim().to_string());
    out
}

pub fn brief(evidence: &[String]) -> Vec<String> {
    let Some(name) = evidence
        .first()
        .and_then(|first| first.strip_prefix("invariant \""))
        .and_then(|rest| rest.split_once('"'))
        .map(|(name, _)| name.to_string())
    else {
        return evidence.to_vec();
    };
    let shown = evidence
        .iter()
        .rev()
        .find_map(|line| {
            let mut sides = line.split(" = ");
            let (terms, values) = (sides.next()?, sides.next()?);
            (line.ends_with("= false") && sides.next() == Some("false")).then(|| {
                let sign = match terms.split('(').next() {
                    Some("eq") => " ≠ ",
                    _ => " · ",
                };
                parts(terms)
                    .iter()
                    .zip(parts(values))
                    .map(|(term, value)| match *term == value {
                        true => value,
                        false => format!("{term} = {value}"),
                    })
                    .collect::<Vec<_>>()
                    .join(sign)
            })
        })
        .unwrap_or_else(|| evidence.last().cloned().unwrap_or_default());
    vec![format!("\"{name}\""), shown]
}

pub fn checked(d: &Diagram) -> BTreeSet<(String, String)> {
    let mut out = BTreeSet::new();
    for o in &d.objects {
        for r in o.rules.iter().filter(|r| r.checks(&o.name)) {
            out.extend(r.next.iter().map(|(k, _)| (o.name.clone(), k.clone())));
        }
    }
    let sources: Vec<String> = d.invariants().iter().map(|i| i.source()).collect();
    for (object, key) in d.keys() {
        if sources
            .iter()
            .any(|s| s.contains(&format!("{object}.{key}")))
        {
            out.insert((object.clone(), key.clone()));
        }
    }
    out
}

pub fn compact(text: &str) -> String {
    serde_json::from_str::<Json>(text).map_or_else(|_| text.trim().to_string(), |j| j.to_string())
}

pub fn sorted(hops: &[Hop]) -> Vec<&Hop> {
    let mut all: Vec<&Hop> = hops.iter().collect();
    all.sort_by_key(|h| h.sent);
    all
}

const LABEL: usize = 160;
const SUMMARY: usize = 22;

fn short(text: String) -> String {
    match text.chars().count() > LABEL {
        true => text.chars().take(LABEL).chain(['…']).collect(),
        false => text,
    }
}

fn calls(hops: &[Hop]) -> Vec<Call> {
    let mut timed: Vec<(usize, Call)> = Vec::new();
    for h in hops {
        let reply = compact(&h.reply);
        let empty = matches!(reply.as_str(), "" | "{}");
        let ended = match empty {
            true => format!(" → {}", h.status),
            false => String::new(),
        };
        timed.push((
            h.sent,
            Call {
                from: h.from.clone(),
                to: h.to.clone(),
                text: format!(
                    "{}{ended}",
                    short(format!("{} {} {}", h.method, h.path, compact(&h.body)))
                ),
                reply: false,
            },
        ));
        if !empty {
            timed.push((
                h.answered,
                Call {
                    from: h.to.clone(),
                    to: h.from.clone(),
                    text: short(format!("{} {reply}", h.status)),
                    reply: true,
                },
            ));
        }
    }
    timed.sort_by_key(|(at, _)| *at);
    timed.into_iter().map(|(_, c)| c).collect()
}
