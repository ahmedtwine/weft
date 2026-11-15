use std::collections::BTreeMap;
use std::future::Future;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::PathBuf;
use std::pin::Pin;

use futures::{StreamExt, stream};
use rig::typesafeai::types::{Answer, Question};
use rig::typesafeai::{DynamicQuery, Evaluate, Jev as Client};
use serde_json::{Value, json};

use crate::{Choice, Measure, Result, home};

const SPLIT: f64 = 0.2;
const PARALLEL: usize = 8;

pub struct Jev {
    client: Client,
    cache: PathBuf,
    runtime: tokio::runtime::Runtime,
}

impl Jev {
    pub fn open() -> Result<Self> {
        let secrets = std::fs::read_to_string(home().join("secrets.env")).unwrap_or_default();
        let token = std::env::var("JEV_TOKEN")
            .ok()
            .or_else(|| {
                secrets.lines().find_map(|l| {
                    l.strip_prefix("JEV_TOKEN=")
                        .map(|v| v.trim().trim_matches('"').to_string())
                })
            })
            .filter(|t| !t.is_empty())
            .ok_or("no JEV_TOKEN in the environment or in ~/.config/invariants/secrets.env")?;
        let cache =
            PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".cache/invariants");
        std::fs::create_dir_all(&cache).map_err(|e| e.to_string())?;
        let runtime = tokio::runtime::Runtime::new().map_err(|e| e.to_string())?;
        Ok(Self {
            client: Client::new(token),
            cache,
            runtime,
        })
    }

    async fn ask(
        &self,
        state: &Value,
        questions: BTreeMap<String, Question>,
    ) -> Result<Vec<Answer>> {
        let mut hasher = DefaultHasher::new();
        (
            state.to_string(),
            serde_json::to_string(&questions).unwrap_or_default(),
        )
            .hash(&mut hasher);
        let file = self.cache.join(format!("{:016x}", hasher.finish()));
        if let Some(kept) = std::fs::read(&file)
            .ok()
            .and_then(|b| serde_json::from_slice::<BTreeMap<String, Answer>>(&b).ok())
        {
            return Ok(kept.into_values().collect());
        }
        let query = DynamicQuery::new(questions).map_err(|e| e.to_string())?;
        let answers = self
            .client
            .evaluation()
            .evaluate(state, query)
            .await
            .map_err(|e| format!("decision model: {e}"))?
            .answers;
        std::fs::write(&file, serde_json::to_vec(&answers).unwrap_or_default()).ok();
        Ok(answers.into_values().collect())
    }

    fn choose<'a>(
        &'a self,
        choice: &'a Choice,
        state: &'a Value,
    ) -> Pin<Box<dyn Future<Output = Result<f64>> + 'a>> {
        Box::pin(async move {
            let criteria = choice
                .flag
                .iter()
                .chain(&choice.pass)
                .chain(&choice.split)
                .map(|(k, v)| (k.clone(), Some(json!(v))))
                .collect();
            let question = Question::Choice {
                instructions: json!(choice.question),
                criteria,
            };
            let Some(Answer::Choice { probabilities, .. }) = self
                .ask(state, BTreeMap::from([("c".to_string(), question)]))
                .await?
                .pop()
            else {
                return Err("decision model: no choice in the answer".into());
            };
            let mass = |keys: &BTreeMap<String, String>| {
                keys.keys()
                    .filter_map(|k| probabilities.get(k))
                    .sum::<f64>()
            };
            let split = mass(&choice.split);
            let deeper = match &choice.then {
                Some(then) if split > SPLIT => split * self.choose(then, state).await?,
                _ => 0.0,
            };
            Ok(mass(&choice.flag) + deeper)
        })
    }

    pub fn scores(&self, measure: &Measure, items: &[(String, String)]) -> Result<Vec<f64>> {
        match measure {
            Measure::Pattern(words) => Ok(items
                .iter()
                .map(|(t, _)| f64::from(u8::from(t.to_lowercase().contains(&words.to_lowercase()))))
                .collect()),
            Measure::Choice(choice) => {
                let states: Vec<Value> = items
                    .iter()
                    .map(|(text, context)| json!({"text": text, "context": context}))
                    .collect();
                self.runtime
                    .block_on(
                        stream::iter(states.iter().map(|state| self.choose(choice, state)))
                            .buffered(PARALLEL)
                            .collect::<Vec<_>>(),
                    )
                    .into_iter()
                    .collect()
            }
        }
    }
}
