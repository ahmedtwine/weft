use std::{collections::BTreeSet, iter};

use serde_json::{Value as Json, json};
use weft_draw::{Call, Code, Row, Step, guessed, summary};
use weft_parse::{Diagram, Pairs, Ran, Span, State};
use weft_simulator::{Hop, Outcome};

use crate::escape;

const START: &str = "start";
const PASS: &str = "circle-check";
const FAIL: &str = "circle-x";
const SHORT: usize = 32;

pub fn kinds(ran: &Ran) -> Vec<(&str, &[Span])> {
    iter::once((START, ran.start.as_slice()))
        .chain(ran.actions.iter().map(|(v, s)| (v.as_str(), s.as_slice())))
        .collect()
}

pub fn size(s: &Span) -> usize {
    s.to + 1 - s.from
}

fn row(o: &Outcome, object: &str, key: &str, value: &Json) -> Row {
    [
        o.types
            .get(&format!("{object}.{key}"))
            .cloned()
            .unwrap_or_else(|| guessed(value).to_string()),
        key.to_string(),
        summary(value),
    ]
}

pub fn code(d: &Diagram, o: &Outcome) -> Code {
    let steps: Vec<Step> = o
        .trace
        .windows(2)
        .filter_map(|pair| {
            let (before, after) = (&pair[0], &pair[1]);
            let (verb, to) = after.action.clone()?;
            let empty = Pairs::new();
            let then = before.state.get(&to).unwrap_or(&empty);
            let changed = after
                .state
                .get(&to)
                .unwrap_or(&empty)
                .iter()
                .filter(|(k, v)| then.get(*k) != Some(*v))
                .map(|(k, v)| row(o, &to, k, v))
                .collect();
            Some(Step {
                from: d.caller().unwrap_or(&to).to_string(),
                calls: calls(&after.hops),
                changed,
                verb,
                to,
            })
        })
        .collect();
    let failed = o.failed.then(|| o.trace.last()).flatten().map(|l| {
        (
            l.action.as_ref().map(|_| steps.len().saturating_sub(1)),
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
                    let rows: Vec<Row> = keys
                        .iter()
                        .map(|(k, v)| row(o, &object.name, k, v))
                        .collect();
                    (!rows.is_empty()).then(|| (object.name.clone(), rows))
                })
                .collect()
        })
        .unwrap_or_default();
    Code {
        initial,
        failed,
        start_calls: o.trace.first().map_or(Vec::new(), |l| calls(&l.hops)),
        steps,
    }
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
