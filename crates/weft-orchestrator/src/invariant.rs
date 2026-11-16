use weft_hegel::{Case, Halt};
use weft_parse::{Check, Diagram, Invariant, State};

use crate::stateful::{Checked, Event, Stream};

#[derive(Clone, Copy)]
pub(crate) enum Moment {
    Initial,
    Round,
    Final,
}

pub(crate) struct Broken {
    pub origin: String,
    pub evidence: Vec<String>,
}

pub(crate) struct Judged {
    pub checks: u64,
    pub broken: Option<Broken>,
}

pub(crate) struct Invariants<'a> {
    all: Vec<&'a Invariant>,
    places: Vec<String>,
    file: String,
    pub checked: Vec<Checked>,
}

impl<'a> Invariants<'a> {
    pub(crate) fn new(d: &'a Diagram, file: &str) -> Invariants<'a> {
        let all = d.invariants();
        let checked = all
            .iter()
            .map(|i| Checked {
                name: i.origin(file),
                checks: 0,
                items: None,
            })
            .collect();
        Invariants {
            places: all.iter().map(|i| i.place()).collect(),
            all,
            file: file.to_string(),
            checked,
        }
    }

    pub(crate) fn declared(&self) -> Vec<(&str, bool)> {
        self.all
            .iter()
            .zip(&self.places)
            .map(|(i, place)| (place.as_str(), i.check == Check::Always))
            .collect()
    }

    pub(crate) fn judged(
        &mut self,
        case: &Case,
        moment: Moment,
        first: &State,
        real: &State,
        stream: &mut Stream<'_>,
    ) -> Result<Judged, Halt> {
        let mut checks = 0;
        for (index, invariant) in self.all.iter().enumerate() {
            if matches!(moment, Moment::Round) && !case.invariant(index)? {
                continue;
            }
            checks += 1;
            let checked = &mut self.checked[index];
            checked.checks += 1;
            checked.items = invariant
                .items(real)
                .map(|items| checked.items.unwrap_or(0) + items as u64);
            let failure = invariant.judge(first, real);
            stream(Event::Check(
                &self.places[index],
                failure.as_ref().map(|f| f.evidence.as_slice()),
            ));
            if let Some(failure) = failure {
                return Ok(Judged {
                    checks,
                    broken: Some(Broken {
                        origin: failure.assert.origin(&self.file),
                        evidence: failure.evidence,
                    }),
                });
            }
        }
        Ok(Judged {
            checks,
            broken: None,
        })
    }
}
