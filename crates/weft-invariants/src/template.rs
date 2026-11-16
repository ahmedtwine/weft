use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use weft_samples::Candidate;

use crate::table::{Row, Table};

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Term {
    Is(usize, usize),
    Not(usize, usize),
    In(usize, Vec<usize>),
    Same(usize, usize),
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Implication {
    when: Vec<Term>,
    then: Term,
}

fn block(head: &str, lines: &[String]) -> String {
    let inner: Vec<String> = lines
        .iter()
        .flat_map(|line| line.lines())
        .map(|line| format!("  {line}"))
        .collect();
    format!("{head} {{\n{}\n}}", inner.join("\n"))
}

fn word(value: &Value) -> String {
    value
        .as_str()
        .map_or_else(|| value.to_string(), String::from)
}

impl Term {
    fn key(&self) -> usize {
        match self {
            Term::Is(key, _) | Term::Not(key, _) | Term::In(key, _) | Term::Same(key, _) => *key,
        }
    }

    fn holds(&self, table: &Table, row: &[usize]) -> bool {
        match self {
            Term::Is(key, value) => row[*key] == *value,
            Term::Not(key, value) => row[*key] != *value,
            Term::In(key, values) => values.contains(&row[*key]),
            Term::Same(key, other) => {
                table.values[*key][row[*key]] == table.values[*other][row[*other]]
            }
        }
    }

    fn rest(&self, table: &Table) -> Option<Vec<usize>> {
        match self {
            Term::In(key, values) if values.len() * 2 > table.values[*key].len() => Some(
                (0..table.values[*key].len())
                    .filter(|value| !values.contains(value))
                    .collect(),
            ),
            _ => None,
        }
    }

    fn kdl(&self, table: &Table) -> String {
        let list = |key: usize, values: &[usize]| {
            values
                .iter()
                .fold(format!("in {:?}", table.name(key)), |out, value| {
                    format!("{out} {}", table.values[key][*value])
                })
        };
        match (self, self.rest(table)) {
            (Term::In(key, _), Some(rest)) => block("not", &[list(*key, &rest)]),
            (Term::In(key, values), None) => list(*key, values),
            (Term::Is(key, value), _) => {
                format!("eq {:?} {}", table.name(*key), table.values[*key][*value])
            }
            (Term::Not(key, value), _) => {
                format!("neq {:?} {}", table.name(*key), table.values[*key][*value])
            }
            (Term::Same(key, other), _) => {
                format!("eq {:?} {:?}", table.name(*key), table.name(*other))
            }
        }
    }

    fn said(&self, table: &Table) -> String {
        let key = |key: usize| table.keys[key].1.as_str();
        let list = |at: usize, values: &[usize]| {
            values
                .iter()
                .map(|value| word(&table.values[at][*value]))
                .collect::<Vec<_>>()
                .join(" or ")
        };
        match (self, self.rest(table)) {
            (Term::In(at, _), Some(rest)) => format!("{} not {}", key(*at), list(*at, &rest)),
            (Term::In(at, values), None) => format!("{} {}", key(*at), list(*at, values)),
            (Term::Is(at, value), _) => format!("{} {}", key(*at), list(*at, &[*value])),
            (Term::Not(at, value), _) => format!("{} not {}", key(*at), list(*at, &[*value])),
            (Term::Same(at, other), _) => format!("{} equal to {}", key(*at), key(*other)),
        }
    }
}

impl Implication {
    fn name(&self, table: &Table) -> String {
        let when: Vec<String> = self.when.iter().map(|term| term.said(table)).collect();
        match when.is_empty() {
            true => self.then.said(table),
            false => format!("{} gives {}", when.join(" and "), self.then.said(table)),
        }
    }

    pub fn candidate(&self, table: &Table) -> Candidate {
        let when: Vec<String> = self.when.iter().map(|term| term.kdl(table)).collect();
        let then = self.then.kdl(table);
        Candidate {
            name: self.name(table),
            measure: match when.as_slice() {
                [] => then,
                [one] => block("implies", &[one.clone(), then]),
                many => block("implies", &[block("and", many), then]),
            },
        }
    }
}

pub fn implications(table: &Table, rows: &BTreeSet<Row>) -> Vec<Implication> {
    let rows: Vec<&Row> = rows.iter().collect();
    let keys = table.keys.len();
    let terms: Vec<Term> = (0..keys)
        .filter(|&key| table.values[key].len() > 1)
        .flat_map(|key| {
            (0..table.values[key].len())
                .flat_map(move |value| [Term::Is(key, value), Term::Not(key, value)])
        })
        .collect();
    let pairs = terms.iter().enumerate().flat_map(|(at, first)| {
        terms[at + 1..]
            .iter()
            .filter(move |second| second.key() != first.key())
            .map(move |second| vec![first.clone(), second.clone()])
    });
    let whens = std::iter::once(Vec::new())
        .chain(terms.iter().map(|term| vec![term.clone()]))
        .chain(pairs);
    let mut held: BTreeSet<Implication> = BTreeSet::new();
    for when in whens {
        let matched: Vec<&Row> = rows
            .iter()
            .copied()
            .filter(|row| when.iter().all(|term| term.holds(table, row)))
            .collect();
        let Some(first) = matched.first() else {
            continue;
        };
        let fixed = |key: usize| {
            when.iter()
                .any(|term| matches!(term, Term::Is(at, _) if *at == key))
        };
        let thens = (0..keys)
            .filter(|&key| !fixed(key))
            .map(|key| Term::Is(key, first[key]))
            .chain(
                (0..keys).flat_map(|key| (key + 1..keys).map(move |other| Term::Same(key, other))),
            )
            .filter(|then| matched.iter().all(|row| then.holds(table, row)));
        for then in thens {
            let narrower = when.iter().any(|drop| {
                let when: Vec<Term> = when.iter().filter(|term| *term != drop).cloned().collect();
                held.contains(&Implication {
                    when,
                    then: then.clone(),
                })
            });
            let always = held.contains(&Implication {
                when: Vec::new(),
                then: then.clone(),
            });
            if !narrower && !always {
                held.insert(Implication {
                    when: when.clone(),
                    then,
                });
            }
        }
    }
    let mut groups: BTreeMap<(Vec<Term>, usize, Term), Vec<usize>> = BTreeMap::new();
    for candidate in &held {
        for term in &candidate.when {
            if let Term::Is(key, value) = term {
                let rest = candidate
                    .when
                    .iter()
                    .filter(|other| *other != term)
                    .cloned()
                    .collect();
                groups
                    .entry((rest, *key, candidate.then.clone()))
                    .or_default()
                    .push(*value);
            }
        }
    }
    let merged = groups
        .into_iter()
        .filter(|(_, values)| values.len() > 1)
        .map(|((mut when, key, then), values)| {
            when.push(Term::In(key, values));
            Implication { when, then }
        });
    held.iter().cloned().chain(merged).collect()
}
