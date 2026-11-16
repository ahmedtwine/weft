use std::{collections::BTreeSet, iter};

use serde_json::Value as Json;
use weft_parse::{Diagram, Ran, Span};
use weft_simulator::Hop;

const START: &str = "start";

pub fn kinds(ran: &Ran) -> Vec<(&str, &[Span])> {
    iter::once((START, ran.start.as_slice()))
        .chain(ran.actions.iter().map(|(v, s)| (v.as_str(), s.as_slice())))
        .collect()
}

pub fn size(s: &Span) -> usize {
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



