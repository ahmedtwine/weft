use std::collections::BTreeSet;
use std::iter;
use std::time::Duration;

use serde_json::Value as Json;
use weft_parse::{Config, Diagram, Ran, Span};

use crate::proxy::Hop;
use crate::stateful::{Outcome, Tally, view};

const INPUT: usize = 80;
const DRAWN: &str = "* draw ";
const SHARE: &str = " of test cases";
const START: &str = "start";

fn kinds(ran: &Ran) -> Vec<(&str, &[Span])> {
    iter::once((START, ran.start.as_slice()))
        .chain(ran.actions.iter().map(|(v, s)| (v.as_str(), s.as_slice())))
        .collect()
}

fn size(s: &Span) -> usize {
    s.to + 1 - s.from
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
                    Some("neq") => " = ",
                    Some("ilt") => " ≥ ",
                    Some("ilte") => " > ",
                    Some("igt") => " ≤ ",
                    Some("igte") => " < ",
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

fn checked(d: &Diagram) -> BTreeSet<(String, String)> {
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

struct Table {
    head: Vec<String>,
    rows: Vec<Vec<String>>,
}

impl Table {
    fn new<const N: usize>(head: [&str; N], rows: Vec<Vec<String>>) -> Table {
        Table {
            head: head.map(String::from).to_vec(),
            rows,
        }
    }

    fn text(&self) -> Vec<String> {
        let line = |cells: Vec<&str>| format!("  {}", cells.join(" · "));
        iter::once(line(self.head.iter().map(String::as_str).collect()))
            .chain(self.rows.iter().map(|cells| {
                line(
                    cells
                        .iter()
                        .map(|c| match c.is_empty() {
                            true => "-",
                            false => c,
                        })
                        .collect(),
                )
            }))
            .collect()
    }
}

pub fn checks(o: &Outcome) -> u64 {
    o.tally.iter().map(|t| t.checks).sum()
}

fn headline(d: &Diagram, config: &Config, o: &Outcome, took: Duration) -> String {
    format!(
        "{}{} · {} cases · {} runs · {} steps · {} checks{}{}{} [{:.2}s]",
        d.impls
            .iter()
            .map(|i| i.adapter.as_str())
            .collect::<Vec<_>>()
            .join(", "),
        config
            .transport
            .as_ref()
            .map_or(String::new(), |t| format!(" · transport {t}")),
        o.cases,
        o.runs,
        o.tally.iter().map(|t| t.steps).sum::<u64>(),
        checks(o),
        match (o.failed, o.trace.len() - 1) {
            (false, _) => String::new(),
            (true, 0) => " · fails at start".to_string(),
            (true, 1) => " · fails after 1 step".to_string(),
            (true, n) => format!(" · fails after {n} steps"),
        },
        o.origin
            .as_ref()
            .map_or(String::new(), |origin| format!(" · {origin}")),
        o.caveat
            .as_ref()
            .map_or(String::new(), |c| format!(" · {c}")),
        took.as_secs_f64()
    )
}

fn tables(d: &Diagram, o: &Outcome) -> Vec<Table> {
    let named = checked(d);
    let shown = |state: &weft_parse::State| {
        let kept: weft_parse::State = state
            .iter()
            .map(|(object, pairs)| {
                (
                    object.clone(),
                    pairs
                        .iter()
                        .filter(|(key, _)| {
                            named.is_empty() || named.contains(&(object.clone(), key.to_string()))
                        })
                        .map(|(k, v)| (k.clone(), v.clone()))
                        .collect(),
                )
            })
            .filter(|(_, pairs): &(String, weft_parse::Pairs)| !pairs.is_empty())
            .collect();
        view(&kept)
    };
    let cut = |text: String| {
        let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
        match text.chars().count() > INPUT {
            true => text.chars().take(INPUT).chain(['…']).collect(),
            false => text,
        }
    };
    let trace = Table::new(
        ["step", "action", "input", "state", "check"],
        o.trace
            .iter()
            .enumerate()
            .flat_map(|(k, l)| {
                iter::once(vec![
                    k.to_string(),
                    l.step(),
                    cut(l.input()),
                    shown(&l.state),
                    brief(&l.evidence).join(" · "),
                ])
                .chain(sorted(&l.hops).into_iter().map(|h| {
                    vec![
                        String::new(),
                        format!("↳ {} → {} {} {}", h.from, h.to, h.method, h.path),
                        cut(compact(&h.body)),
                        cut(format!("{} {}", h.status, compact(&h.reply))),
                        String::new(),
                    ]
                }))
            })
            .collect(),
    );
    let case = Table::new(
        ["case", "runs"],
        o.counts
            .statuses
            .iter()
            .map(|(status, runs)| vec![status.to_string(), runs.to_string()])
            .collect(),
    );
    let tally = Table::new(
        ["action", "runs", "steps", "refused", "checks"],
        o.tally
            .iter()
            .map(|t| {
                vec![
                    t.step.clone(),
                    t.runs.to_string(),
                    t.steps.to_string(),
                    t.refused
                        .iter()
                        .map(|(cause, tries)| format!("{cause} {tries}"))
                        .collect::<Vec<_>>()
                        .join(", "),
                    t.checks.to_string(),
                ]
            })
            .collect(),
    );
    let verb = |t: &Tally| t.step.split(' ').next().unwrap_or_default().to_string();
    let then = Table {
        head: iter::once("then".to_string())
            .chain(o.tally.iter().map(verb))
            .collect(),
        rows: o
            .tally
            .iter()
            .filter(|_| o.tally.len() > 1)
            .map(|t| {
                iter::once(verb(t))
                    .chain(t.then.iter().map(u64::to_string))
                    .collect()
            })
            .collect(),
    };
    let call = Table::new(
        ["call", "status", "count"],
        o.counts
            .calls
            .iter()
            .map(|((call, status), count)| {
                vec![call.clone(), status.to_string(), count.to_string()]
            })
            .collect(),
    );
    let pool = Table::new(
        ["pool", "add", "generate", "consume", "empty"],
        o.counts
            .pools
            .iter()
            .map(|(pool, p)| {
                iter::once(pool.clone())
                    .chain([p.add, p.generate, p.consume, p.empty].map(|n| n.to_string()))
                    .collect()
            })
            .collect(),
    );
    let invariant = Table::new(
        ["invariant", "checks", "items"],
        o.counts
            .invariants
            .iter()
            .map(|i| {
                vec![
                    i.name.clone(),
                    i.checks.to_string(),
                    i.items.map_or(String::new(), |items| items.to_string()),
                ]
            })
            .collect(),
    );
    let code = Table::new(
        ["code", "files", "lines"],
        kinds(&o.ran)
            .iter()
            .map(|(name, spans)| {
                let mut files: Vec<&String> = spans.iter().map(|s| &s.file).collect();
                files.sort();
                files.dedup();
                vec![
                    name.to_string(),
                    files.len().to_string(),
                    spans.iter().map(size).sum::<usize>().to_string(),
                ]
            })
            .collect(),
    );
    let drawn = Table::new(
        ["draw", "cases"],
        o.output
            .iter()
            .filter_map(|line| line.trim().strip_prefix(DRAWN)?.rsplit_once(": "))
            .map(|(value, share)| {
                vec![value.to_string(), share.trim_end_matches(SHARE).to_string()]
            })
            .collect(),
    );
    [trace, case, tally, then, call, pool, invariant, code, drawn]
        .into_iter()
        .filter(|t| !t.rows.is_empty())
        .collect()
}

fn invariants(d: &Diagram) -> Vec<String> {
    d.objects
        .iter()
        .flat_map(|o| {
            o.invariants
                .iter()
                .map(|invariant| format!("{} · {}", o.name, invariant.source()))
        })
        .collect()
}

pub fn summary(path: &str, d: &Diagram, config: &Config, o: &Outcome, took: Duration) -> String {
    let head = format!(
        "{} {path}{}",
        match o.failed {
            true => '✕',
            false => '✓',
        },
        match (o.failed, o.trace.last()) {
            (true, Some(last)) => format!(" · {}", brief(&last.evidence).join(" · ")),
            _ => String::new(),
        }
    );
    [head, format!("  {}", headline(d, config, o, took))]
        .into_iter()
        .chain(invariants(d).into_iter().map(|rule| format!("  {rule}")))
        .chain(tables(d, o).iter().flat_map(Table::text))
        .collect::<Vec<_>>()
        .join("\n")
}
