use std::collections::BTreeMap;

use futures::{StreamExt, stream};
use rig::typesafeai::types::{Answer, Question};
use rig::typesafeai::{DynamicQuery, Evaluate, Jev};
use serde_json::Value;

use super::{Store, secret};
use crate::Result;

const PARALLEL: usize = 8;

async fn ask(
    store: &Store,
    state: &Value,
    questions: &BTreeMap<String, Question>,
) -> Result<BTreeMap<String, f64>> {
    let answers: BTreeMap<String, Answer> = store
        .cached(
            (
                state.to_string(),
                serde_json::to_string(questions).unwrap_or_default(),
            ),
            async {
                let query = DynamicQuery::new(questions.clone()).map_err(|e| e.to_string())?;
                let client = Jev::new(secret("JEV_TOKEN")?);
                Ok(client
                    .evaluation()
                    .evaluate(state, query)
                    .await
                    .map_err(|e| format!("decision model: {e}"))?
                    .answers)
            },
        )
        .await?;
    answers
        .into_iter()
        .map(|(id, answer)| match answer {
            Answer::Noul { noul } => Ok((id, noul)),
            _ => Err(format!("decision model: no noul for {id}")),
        })
        .collect()
}

pub(super) async fn nouls(
    store: &Store,
    states: &[Value],
    instructions: &BTreeMap<String, Value>,
) -> Result<Vec<BTreeMap<String, f64>>> {
    if instructions.is_empty() {
        return Ok(vec![BTreeMap::new(); states.len()]);
    }
    let questions: BTreeMap<String, Question> = instructions
        .iter()
        .map(|(id, instructions)| {
            (
                id.clone(),
                Question::Noul {
                    instructions: instructions.clone(),
                    criteria: None,
                },
            )
        })
        .collect();
    stream::iter(states.iter().map(|state| ask(store, state, &questions)))
        .buffered(PARALLEL)
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .collect()
}
