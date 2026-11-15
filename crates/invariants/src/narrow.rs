use std::ops::Range;

use crate::jev::Jev;
use crate::{Measure, Medium, Part, Result, Rule, changed, scores};

const LIMIT: usize = 12;
const LEAST: usize = 2;

#[derive(Clone, Debug)]
pub struct Probe {
    pub text: String,
    pub from: String,
}

#[derive(Clone, Debug)]
pub struct Check {
    pub measure: Measure,
    pub lowest: f64,
    pub highest: f64,
    pub held: bool,
}

#[derive(Clone, Debug)]
pub enum Step {
    Show(Probe),
    Found(Rule),
    Failed,
}

pub struct Narrowing {
    pub measures: Vec<Measure>,
    pub probes: Vec<Probe>,
    pub shown: Option<Probe>,
    pub rejected: Vec<String>,
    pub accepted: Vec<String>,
    pub checks: Vec<Check>,
}

fn split(scores: &[f64], rejected: usize, skip: Option<usize>) -> (f64, f64) {
    let kept = |i: &usize| Some(*i) != skip;
    let lowest = (0..rejected)
        .filter(kept)
        .map(|i| scores[i])
        .fold(1.0, f64::min);
    let highest = (rejected..scores.len())
        .filter(kept)
        .map(|i| scores[i])
        .fold(0.0, f64::max);
    (lowest, highest)
}

impl Narrowing {
    pub fn new(
        medium: &dyn Medium,
        artifact: &str,
        parts: &[Part],
        index: usize,
        marked: Range<usize>,
        golden: Vec<Measure>,
    ) -> Self {
        let part = &parts[index];
        let mut probes: Vec<Probe> = Vec::new();
        for action in medium.actions(artifact, part, marked.clone()) {
            for input in action.drawn.values() {
                if let Some(text) = medium
                    .run(artifact, part, &action.verb, &input)
                    .map(|edited| changed(medium, artifact, &edited))
                    && !probes.iter().any(|p| p.text == text)
                {
                    probes.push(Probe {
                        text,
                        from: format!("{} {input:?}", action.verb),
                    });
                }
            }
        }
        probes.extend(
            parts
                .iter()
                .enumerate()
                .filter(|(i, p)| *i != index && p.text != part.text)
                .map(|(i, p)| Probe {
                    text: p.text.clone(),
                    from: format!("part {}", i + 1),
                }),
        );
        let measures = std::iter::once(Measure::Pattern(part.text[marked].to_string()))
            .chain(golden)
            .collect();
        Self {
            measures,
            probes,
            shown: None,
            rejected: vec![part.text.clone()],
            accepted: Vec::new(),
            checks: Vec::new(),
        }
    }

    pub fn react(&mut self, accept: bool) {
        if let Some(probe) = self.shown.take() {
            if accept {
                self.accepted.push(probe.text)
            } else {
                self.rejected.push(probe.text)
            }
        }
    }

    fn check(
        &self,
        jev: &Jev,
        medium: &dyn Medium,
        context: &str,
        measure: &Measure,
    ) -> Result<(Check, Option<Rule>)> {
        let evidence: Vec<String> = self
            .rejected
            .iter()
            .chain(&self.accepted)
            .cloned()
            .collect();
        let scores = scores(jev, medium, measure, &evidence, context)?;
        let n = self.rejected.len();
        let (lowest, highest) = split(&scores, n, None);
        let threshold = (lowest + highest) / 2.0;
        let held = lowest > highest
            && (0..scores.len()).all(|k| {
                let (low, high) = split(&scores, n, Some(k));
                let t = (low + high) / 2.0;
                if k < n { scores[k] >= t } else { scores[k] < t }
            });
        let enough = n >= LEAST && self.accepted.len() >= LEAST;
        let rule = (held && enough).then(|| Rule {
            measure: measure.clone(),
            threshold,
            rejected: self.rejected.clone(),
            accepted: self.accepted.clone(),
        });
        Ok((
            Check {
                measure: measure.clone(),
                lowest,
                highest,
                held,
            },
            rule,
        ))
    }

    pub fn step(&mut self, jev: &Jev, medium: &dyn Medium, context: &str) -> Result<Step> {
        self.checks.clear();
        for measure in self.measures.clone() {
            let (check, rule) = self.check(jev, medium, context, &measure)?;
            self.checks.push(check);
            if let Some(rule) = rule {
                return Ok(Step::Found(rule));
            }
        }
        let unseen: Vec<Probe> = self
            .probes
            .iter()
            .filter(|p| !self.rejected.contains(&p.text) && !self.accepted.contains(&p.text))
            .cloned()
            .collect();
        let alive: Vec<Measure> = self
            .checks
            .iter()
            .filter(|c| c.held || self.accepted.is_empty() && c.lowest >= 0.5)
            .map(|c| c.measure.clone())
            .collect();
        if unseen.is_empty()
            || alive.is_empty()
            || self.rejected.len() + self.accepted.len() > LIMIT
        {
            return Ok(Step::Failed);
        }
        let texts: Vec<String> = unseen.iter().map(|p| p.text.clone()).collect();
        let mut votes = vec![0usize; texts.len()];
        for measure in &alive {
            for (vote, score) in votes
                .iter_mut()
                .zip(scores(jev, medium, measure, &texts, context)?)
            {
                *vote += usize::from(score >= 0.5);
            }
        }
        let half = alive.len() as f64 / 2.0;
        let best = (0..texts.len())
            .min_by(|a, b| {
                (votes[*a] as f64 - half)
                    .abs()
                    .total_cmp(&(votes[*b] as f64 - half).abs())
            })
            .unwrap_or(0);
        self.shown = Some(unseen[best].clone());
        Ok(Step::Show(unseen[best].clone()))
    }
}
