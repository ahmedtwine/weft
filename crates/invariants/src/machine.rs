use serde_json::json;

use crate::provider::gemini::{Gemini, MODEL};
use crate::provider::jev::Jev;
use crate::{Generator, Invariant, Medium, Result, Rule, THRESHOLD, scores};

const STEPS: usize = 12;
const SIZE: usize = 6;

pub struct Alternate {
    pub text: String,
    pub kind: &'static str,
    pub change: String,
    pub invariants: Vec<String>,
}

pub struct Machine {
    pub alternates: Vec<Alternate>,
    reactions: Vec<Option<bool>>,
    pub invariants: Vec<(String, String)>,
}

impl Machine {
    pub fn new(
        (jev, gemini): (&Jev, &Gemini),
        medium: &dyn Medium,
        artifact: &str,
        (marked, note): (&str, &str),
    ) -> Result<Self> {
        let at = artifact
            .find(marked)
            .ok_or(format!("{marked:?} is absent"))?;
        let parts = medium.parts(artifact);
        let part = parts
            .iter()
            .find(|p| p.span.start <= at && at + marked.len() <= p.span.end)
            .ok_or(format!("{marked:?} is in no part"))?;
        let offset = at - part.span.start;
        let model = Rule {
            verb: "alternate".into(),
            drawn: Generator::Model(
                MODEL.into(),
                json!({
                    "artifact": artifact,
                    "part": part.text,
                    "marked": marked,
                    "note": note,
                    "size": SIZE
                }),
            ),
        };
        let mut drawn = vec![(part.text.clone(), "annotation", note.to_string())];
        let mut candidates = vec![(
            marked.to_string(),
            format!("Each part is free of the text {marked:?}."),
        )];
        for rule in std::iter::once(model).chain(medium.rules(
            artifact,
            part,
            offset..offset + marked.len(),
        )) {
            let kind = rule.drawn.kind();
            for value in rule.drawn.values(gemini)? {
                if let Some(text) = value["text"].as_str() {
                    let change = value["measure"].as_str().unwrap_or_default();
                    drawn.push((text.to_string(), kind, change.to_string()));
                    if let (Some(name), Some(measure)) =
                        (value["name"].as_str(), value["measure"].as_str())
                        && !candidates.iter().any(|(known, _)| known == name)
                    {
                        candidates.push((name.to_string(), measure.to_string()));
                    }
                    continue;
                }
                let input = value
                    .as_str()
                    .map_or_else(|| value.to_string(), String::from);
                let edited = medium
                    .run(artifact, part, &rule.verb, &input)
                    .unwrap_or_default();
                drawn.extend(
                    medium
                        .parts(&edited)
                        .into_iter()
                        .filter(|v| !parts.iter().any(|p| p.text == v.text))
                        .map(|v| (v.text, kind, format!("{} {input:?}", rule.verb))),
                );
            }
        }
        drawn.extend(
            parts
                .iter()
                .map(|p| (p.text.clone(), "part", String::new())),
        );
        let mut unique: Vec<(String, &'static str, String)> = Vec::new();
        for alternate in drawn {
            if !unique.iter().any(|(seen, ..)| *seen == alternate.0) {
                unique.push(alternate);
            }
        }
        let invariants = candidates;
        let texts: Vec<&str> = unique.iter().map(|(text, ..)| text.as_str()).collect();
        let alternates: Vec<Alternate> =
            scores(jev, &invariants.iter().cloned().collect(), &texts)?
                .into_iter()
                .zip(unique)
                .map(|(scores, (text, kind, change))| Alternate {
                    text,
                    kind,
                    change,
                    invariants: scores
                        .into_iter()
                        .filter(|(_, score)| *score >= THRESHOLD)
                        .map(|(name, _)| name)
                        .collect(),
                })
                .collect();
        let mut reactions = vec![None; alternates.len()];
        reactions[0] = Some(true);
        Ok(Self {
            alternates,
            reactions,
            invariants,
        })
    }

    fn agreeing(&self) -> Vec<&(String, String)> {
        self.invariants
            .iter()
            .filter(|(name, _)| {
                self.reactions
                    .iter()
                    .zip(&self.alternates)
                    .all(|(reaction, alternate)| {
                        reaction.is_none_or(|reject| reject == alternate.invariants.contains(name))
                    })
            })
            .collect()
    }

    pub fn round(&self) -> Option<usize> {
        let agreeing = self.agreeing();
        let votes = |round: usize| {
            agreeing
                .iter()
                .filter(|(name, _)| self.alternates[round].invariants.contains(name))
                .count()
        };
        (0..self.alternates.len())
            .filter(|&round| {
                self.reactions[round].is_none() && (1..agreeing.len()).contains(&votes(round))
            })
            .min_by_key(|&round| (2 * votes(round)).abs_diff(agreeing.len()))
            .filter(|_| self.reactions.iter().flatten().count() <= STEPS)
    }

    pub fn react(&mut self, round: usize, reject: bool) {
        self.reactions[round] = Some(reject);
    }

    pub fn invariant(&self) -> Option<Invariant> {
        let (name, measure) = self.agreeing().into_iter().min_by_key(|(name, _)| {
            self.alternates
                .iter()
                .filter(|alternate| alternate.invariants.contains(name))
                .count()
        })?;
        let texts = |reject: bool| {
            self.alternates
                .iter()
                .zip(&self.reactions)
                .filter(|(_, reaction)| **reaction == Some(reject))
                .map(|(alternate, _)| alternate.text.clone())
                .collect()
        };
        Some(Invariant {
            name: name.clone(),
            measure: measure.clone(),
            rejected: texts(true),
            accepted: texts(false),
        })
    }
}
