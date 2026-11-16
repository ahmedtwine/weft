use std::collections::BTreeSet;

use serde_json::Value;
use weft_parse::State;

const CLOSED: usize = 32;

pub type Row = Vec<usize>;

pub struct Table {
    pub keys: Vec<(String, String)>,
    pub values: Vec<Vec<Value>>,
}

fn read(state: &State, (object, key): &(String, String)) -> Option<Value> {
    state.get(object)?.get(key).cloned()
}

fn written(value: &Value) -> bool {
    value.is_string() || value.is_i64() || value.is_u64()
}

impl Table {
    pub fn of(states: &[&State]) -> Self {
        let mut keys: Vec<(String, String)> = Vec::new();
        let mut values: Vec<Vec<Value>> = Vec::new();
        for (object, pairs) in states.iter().flat_map(|state| state.iter()) {
            for (key, value) in pairs {
                let name = (object.clone(), key.clone());
                let at = keys.iter().position(|k| *k == name).unwrap_or_else(|| {
                    keys.push(name);
                    values.push(Vec::new());
                    keys.len() - 1
                });
                if !values[at].contains(value) {
                    values[at].push(value.clone());
                }
            }
        }
        let closed: Vec<usize> = (0..keys.len())
            .filter(|&at| {
                values[at].len() <= CLOSED
                    && values[at].iter().all(written)
                    && states.iter().all(|state| read(state, &keys[at]).is_some())
            })
            .collect();
        let keys: Vec<(String, String)> = closed.iter().map(|&at| keys[at].clone()).collect();
        let values: Vec<Vec<Value>> = closed.iter().map(|&at| values[at].clone()).collect();
        Self { keys, values }
    }

    pub fn rows(&self, states: &[&State]) -> BTreeSet<Row> {
        states
            .iter()
            .filter_map(|state| {
                self.keys
                    .iter()
                    .zip(&self.values)
                    .map(|(key, domain)| {
                        domain
                            .iter()
                            .position(|v| Some(v) == read(state, key).as_ref())
                    })
                    .collect()
            })
            .collect()
    }

    pub fn size(&self) -> usize {
        self.values.iter().map(Vec::len).product()
    }

    pub fn row(&self, mut at: usize) -> Row {
        self.values
            .iter()
            .map(|domain| {
                let value = at % domain.len();
                at /= domain.len();
                value
            })
            .collect()
    }

    pub fn state(&self, row: &[usize]) -> State {
        let mut state = State::new();
        for (((object, key), domain), value) in self.keys.iter().zip(&self.values).zip(row) {
            state
                .entry(object.clone())
                .or_default()
                .insert(key.clone(), domain[*value].clone());
        }
        state
    }

    pub fn name(&self, key: usize) -> String {
        format!("{}.{}", self.keys[key].0, self.keys[key].1)
    }
}
