use std::collections::BTreeMap;

use futures::{StreamExt, stream};
use rig::typesafeai::types::{Answer, Question};
use rig::typesafeai::{DynamicQuery, Evaluate, Jev as Client};
use serde_json::Value;

use crate::Result;
use crate::provider::{cached, secret};

const PARALLEL: usize = 8;

pub struct Jev {
    client: Client,
    runtime: tokio::runtime::Runtime,
}

impl Jev {
    pub fn open() -> Result<Self> {
        let token = secret("JEV_TOKEN")?;
        let runtime = tokio::runtime::Runtime::new().map_err(|e| e.to_string())?;
        Ok(Self {
            client: Client::new(token),
            runtime,
        })
    }

    async fn ask(
        &self,
        state: &Value,
        questions: &BTreeMap<String, Question>,
    ) -> Result<BTreeMap<String, f64>> {
        let answers: BTreeMap<String, Answer> = cached(
            (
                state.to_string(),
                serde_json::to_string(questions).unwrap_or_default(),
            ),
            async {
                let query = DynamicQuery::new(questions.clone()).map_err(|e| e.to_string())?;
                Ok(self
                    .client
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

    pub fn nouls(
        &self,
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
        self.runtime
            .block_on(
                stream::iter(states.iter().map(|state| self.ask(state, &questions)))
                    .buffered(PARALLEL)
                    .collect::<Vec<_>>(),
            )
            .into_iter()
            .collect()
    }
}
