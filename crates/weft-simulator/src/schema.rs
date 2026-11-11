use serde_json::{Value as Json, json};
use weft_parse::{Draw, Value};

const SHAPED: [&str; 7] = ["type", "enum", "const", "anyOf", "oneOf", "allOf", "not"];
const ANY: &str = "";
const ROOT: &str = "#";
const SAFE: i64 = (1 << 53) - 1;

#[derive(Clone, PartialEq)]
pub enum Choice {
    Bool,
    Int(i64, i64),
    Text(u64, Option<u64>),
    Pattern(String),
    Format(Format),
    Number(Bound, Bound),
    Enum(Vec<Json>),
    AnyOf(Vec<Choice>),
    Array(Box<Choice>, u64, Option<u64>, bool),
    Tuple(Vec<Choice>),
    Record(Box<Choice>, Box<Choice>),
    Object(Vec<(String, Choice, bool)>),
    Rec(String, Box<Choice>, Box<Choice>),
    Var(String),
}

#[derive(Clone, Copy, PartialEq)]
pub enum Bound {
    Open,
    At(f64),
    Past(f64),
}

#[derive(Clone, Copy, PartialEq)]
pub enum Format {
    Email,
    Uri,
    Hostname,
    Uuid,
    Ipv4,
    Ipv6,
}

impl Format {
    const ALL: [(&'static str, Format); 6] = [
        ("email", Format::Email),
        ("uri", Format::Uri),
        ("hostname", Format::Hostname),
        ("uuid", Format::Uuid),
        ("ipv4", Format::Ipv4),
        ("ipv6", Format::Ipv6),
    ];
}

impl Choice {
    pub fn ends(&self) -> bool {
        match self {
            Choice::Var(_) => false,
            Choice::Rec(_, _, shown) => shown.ends(),
            Choice::AnyOf(all) => all.iter().any(Choice::ends),
            Choice::Array(item, min, ..) => *min == 0 || item.ends(),
            Choice::Tuple(all) => all.iter().all(Choice::ends),
            Choice::Object(fields) => fields.iter().all(|(_, c, required)| !required || c.ends()),
            _ => true,
        }
    }

    pub fn loops(&self) -> bool {
        match self {
            Choice::Var(_) => true,
            Choice::Rec(_, _, shown) => shown.loops(),
            Choice::AnyOf(all) | Choice::Tuple(all) => all.iter().any(Choice::loops),
            Choice::Array(item, ..) => item.loops(),
            Choice::Record(key, value) => key.loops() || value.loops(),
            Choice::Object(fields) => fields.iter().any(|(_, c, _)| c.loops()),
            _ => false,
        }
    }

    fn any() -> Choice {
        let var = || Box::new(Choice::Var(ANY.into()));
        let body = Box::new(Choice::AnyOf(vec![
            Choice::Enum(vec![Json::Null]),
            Choice::Bool,
            Choice::Int(-SAFE, SAFE),
            Choice::Text(0, None),
            Choice::Array(var(), 0, None, false),
            Choice::Record(Box::new(Choice::Text(0, None)), var()),
        ]));
        Choice::Rec(ANY.into(), body.clone(), body)
    }

    fn merged(self, other: Choice, name: &str) -> Result<Choice, String> {
        match (self, other) {
            (Choice::Rec(key, body, shown), other) | (other, Choice::Rec(key, body, shown)) => {
                Ok(Choice::Rec(key, body, Box::new(shown.merged(other, name)?)))
            }
            (Choice::AnyOf(all), other) | (other, Choice::AnyOf(all)) => all
                .into_iter()
                .map(|c| c.merged(other.clone(), name))
                .collect::<Result<_, _>>()
                .map(Choice::AnyOf),
            (Choice::Object(mut fields), Choice::Object(more)) => {
                for (key, choice, required) in more {
                    match fields.iter_mut().find(|(k, ..)| *k == key) {
                        Some((_, known, needed)) if *known == choice => *needed |= required,
                        Some(_) => {
                            return Err(format!(
                                "the field `{name}.{key}` has two schemas in one `allOf`"
                            ));
                        }
                        None => fields.push((key, choice, required)),
                    }
                }
                Ok(Choice::Object(fields))
            }
            _ => Err(format!(
                "the field `{name}` has an `allOf` that is not of objects"
            )),
        }
    }

    fn fit(&mut self, values: &[Json]) -> usize {
        let fits: Box<dyn Fn(&Json) -> bool> = match self {
            Choice::Enum(known) => {
                let known = known.clone();
                Box::new(move |v| known.contains(v))
            }
            Choice::Int(min, max) => {
                let (min, max) = (*min, *max);
                Box::new(move |v| v.as_i64().is_some_and(|n| (min..=max).contains(&n)))
            }
            Choice::Number(..) => Box::new(Json::is_i64),
            Choice::Text(..) | Choice::Pattern(_) | Choice::Format(_) => Box::new(Json::is_string),
            Choice::AnyOf(all) => return all.iter_mut().map(|c| c.fit(values)).sum(),
            Choice::Array(item, ..) => return item.fit(values),
            _ => return 0,
        };
        let kept: Vec<Json> = values.iter().filter(|v| fits(v)).cloned().collect();
        match kept.is_empty() {
            true => 0,
            false => {
                *self = Choice::Enum(kept);
                1
            }
        }
    }

    fn narrow(&mut self, at: &mut Vec<String>, draw: &Draw, values: &[Json]) -> usize {
        match self {
            Choice::Object(fields) => fields
                .iter_mut()
                .map(|(key, choice, _)| {
                    at.push(key.clone());
                    let found = match at.ends_with(&draw.path) {
                        true => choice.fit(values),
                        false => 0,
                    };
                    let found = match found {
                        0 => choice.narrow(at, draw, values),
                        found => found,
                    };
                    at.pop();
                    found
                })
                .sum(),
            Choice::AnyOf(all) | Choice::Tuple(all) => {
                all.iter_mut().map(|c| c.narrow(at, draw, values)).sum()
            }
            Choice::Array(item, ..) | Choice::Record(_, item) => item.narrow(at, draw, values),
            Choice::Rec(_, body, shown) => {
                body.narrow(at, draw, values) + shown.narrow(at, draw, values)
            }
            _ => 0,
        }
    }

    pub fn live(self) -> Option<Choice> {
        match self {
            Choice::Enum(values) => (!values.is_empty()).then_some(Choice::Enum(values)),
            Choice::AnyOf(all) => {
                let all: Vec<Choice> = all.into_iter().filter_map(Choice::live).collect();
                (!all.is_empty()).then_some(Choice::AnyOf(all))
            }
            Choice::Tuple(all) => all
                .into_iter()
                .map(Choice::live)
                .collect::<Option<_>>()
                .map(Choice::Tuple),
            Choice::Object(fields) => fields
                .into_iter()
                .filter_map(|(key, choice, required)| match (choice.live(), required) {
                    (Some(choice), _) => Some(Some((key, choice, required))),
                    (None, true) => Some(None),
                    (None, false) => None,
                })
                .collect::<Option<_>>()
                .map(Choice::Object),
            Choice::Array(item, min, max, unique) => match (item.live(), min) {
                (Some(item), _) => Some(Choice::Array(Box::new(item), min, max, unique)),
                (None, 0) => Some(Choice::Tuple(Vec::new())),
                (None, _) => None,
            },
            Choice::Record(key, value) => match (key.live(), value.live()) {
                (Some(key), Some(value)) => Some(Choice::Record(Box::new(key), Box::new(value))),
                _ => Some(Choice::Object(Vec::new())),
            },
            Choice::Rec(key, body, shown) => match (body.live(), shown.live()) {
                (Some(body), Some(shown)) => {
                    Some(Choice::Rec(key, Box::new(body), Box::new(shown)))
                }
                _ => None,
            },
            other => Some(other),
        }
    }

    pub fn narrowed(mut self, draw: &Draw) -> (Choice, usize) {
        let values: Vec<Json> = draw
            .values
            .iter()
            .map(|v| match v {
                Value::Text(s) => json!(s),
                Value::Int(n) => json!(*n as i64),
            })
            .collect();
        let found = self.narrow(&mut Vec::new(), draw, &values);
        (self, found)
    }
}

pub fn choice(name: &str, node: &Json) -> Result<Choice, String> {
    let mut read = Read {
        defs: node.get("$defs"),
        open: vec![(ROOT.to_string(), false)],
    };
    let body = read.choice(name, node)?;
    match (read.open.pop().is_some_and(|(_, used)| used), body.ends()) {
        (false, _) => Ok(body),
        (true, true) => Ok(Choice::Rec(
            ROOT.to_string(),
            Box::new(body.clone()),
            Box::new(body),
        )),
        (true, false) => Err(format!(
            "the schema `{name}` is in each value of `{name}`, and weft draws no value without an end"
        )),
    }
}

struct Read<'a> {
    defs: Option<&'a Json>,
    open: Vec<(String, bool)>,
}

impl Read<'_> {
    fn named(&mut self, name: &str, key: &str, wrong: &str) -> Result<Choice, String> {
        if let Some((_, used)) = self.open.iter_mut().find(|(o, _)| o == key) {
            *used = true;
            return Ok(Choice::Var(key.to_string()));
        }
        let def = self
            .defs
            .and_then(|all| all.get(key))
            .ok_or_else(|| wrong.to_string())?;
        self.open.push((key.to_string(), false));
        let body = self.choice(name, def);
        let used = self.open.pop().is_some_and(|(_, used)| used);
        match (body?, used) {
            (body, false) => Ok(body),
            (body, true) if body.ends() => Ok(Choice::Rec(
                key.to_string(),
                Box::new(body.clone()),
                Box::new(body),
            )),
            _ => Err(format!(
                "the field `{name}` has the schema `{key}` in each value of `{key}`, and weft draws no value without an end"
            )),
        }
    }

    fn each(&mut self, name: &str, all: &[Json]) -> Result<Vec<Choice>, String> {
        all.iter().map(|b| self.choice(name, b)).collect()
    }

    fn choice(&mut self, name: &str, node: &Json) -> Result<Choice, String> {
        let int = |k: &str| node.get(k).and_then(Json::as_i64);
        let number = |k: &str| node.get(k).and_then(Json::as_f64);
        let size = |k: &str| node.get(k).and_then(Json::as_u64);
        let wrong = format!("the field `{name}` is not a JSON Schema that weft draws: `{node}`");
        let listed = |k: &str| {
            node.get(k)
                .and_then(Json::as_array)
                .filter(|all| !all.is_empty())
        };
        let bound = |at: &str, past: &str| match (number(past), number(at)) {
            (Some(n), _) => Bound::Past(n),
            (_, Some(n)) => Bound::At(n),
            _ => Bound::Open,
        };
        if let Some(path) = node.get("$ref").and_then(Json::as_str) {
            let key = match path {
                ROOT => ROOT,
                other => other.strip_prefix("#/$defs/").ok_or(&wrong)?,
            };
            return self.named(name, key, &wrong);
        }
        if node.is_object() && !SHAPED.iter().any(|k| node.get(k).is_some()) {
            return Ok(Choice::any());
        }
        if node.get("not") == Some(&json!({})) {
            return Ok(Choice::Enum(Vec::new()));
        }
        if let Some(all) = listed("allOf") {
            let mut rest = node.clone();
            rest.as_object_mut().ok_or(&wrong)?.remove("allOf");
            let own = match rest.get("type").is_some() || rest.get("properties").is_some() {
                true => vec![self.choice(name, &rest)?],
                false => Vec::new(),
            };
            return [own, self.each(name, all)?]
                .concat()
                .into_iter()
                .map(Ok)
                .reduce(|a, b| a?.merged(b?, name))
                .ok_or(wrong)?;
        }
        if let Some(types) = node.get("type").and_then(Json::as_array) {
            return types
                .iter()
                .map(|t| {
                    let mut one = node.clone();
                    one["type"] = t.clone();
                    self.choice(name, &one)
                })
                .collect::<Result<_, _>>()
                .map(Choice::AnyOf);
        }
        match (
            listed("enum"),
            node.get("const"),
            listed("anyOf").or_else(|| listed("oneOf")),
            node.get("type").and_then(Json::as_str),
        ) {
            (Some(values), ..) => Ok(Choice::Enum(values.clone())),
            (_, Some(value), ..) => Ok(Choice::Enum(vec![value.clone()])),
            (_, _, Some(all), _) => self.each(name, all).map(Choice::AnyOf),
            (.., Some("integer")) => Ok(Choice::Int(
                int("exclusiveMinimum")
                    .map(|n| n.saturating_add(1))
                    .or(int("minimum"))
                    .unwrap_or(-SAFE),
                int("exclusiveMaximum")
                    .map(|n| n.saturating_sub(1))
                    .or(int("maximum"))
                    .unwrap_or(SAFE),
            )),
            (.., Some("string")) => match (
                node.get("pattern").and_then(Json::as_str),
                node.get("format").and_then(Json::as_str),
            ) {
                (Some(pattern), _) => Ok(Choice::Pattern(pattern.to_string())),
                (None, Some(format)) => Format::ALL
                    .iter()
                    .find(|(known, _)| *known == format)
                    .map(|(_, f)| Choice::Format(*f))
                    .ok_or(wrong),
                (None, None) => Ok(Choice::Text(
                    size("minLength").unwrap_or(0),
                    size("maxLength"),
                )),
            },
            (.., Some("number")) => Ok(Choice::Number(
                bound("minimum", "exclusiveMinimum"),
                bound("maximum", "exclusiveMaximum"),
            )),
            (.., Some("boolean")) => Ok(Choice::Bool),
            (.., Some("null")) => Ok(Choice::Enum(vec![Json::Null])),
            (.., Some("array")) => match (listed("prefixItems"), node.get("items")) {
                (Some(all), _) => all
                    .iter()
                    .enumerate()
                    .map(|(i, item)| self.choice(&format!("{name}[{i}]"), item))
                    .collect::<Result<_, _>>()
                    .map(Choice::Tuple),
                (None, Some(item)) => Ok(Choice::Array(
                    Box::new(self.choice(&format!("{name}[]"), item)?),
                    size("minItems").unwrap_or(0),
                    size("maxItems"),
                    node.get("uniqueItems").and_then(Json::as_bool) == Some(true),
                )),
                (None, None) => Err(wrong),
            },
            (.., Some("object"))
                if node.get("properties").is_none()
                    && node
                        .get("additionalProperties")
                        .is_some_and(Json::is_object) =>
            {
                let key = match node.get("propertyNames") {
                    Some(names) => self.choice(&format!("{name}{{}}"), names)?,
                    None => Choice::Text(0, None),
                };
                Ok(Choice::Record(
                    Box::new(key),
                    Box::new(self.choice(&format!("{name}{{}}"), &node["additionalProperties"])?),
                ))
            }
            (.., Some("object")) => {
                let required: Vec<&str> = node
                    .get("required")
                    .and_then(Json::as_array)
                    .map_or(Vec::new(), |all| {
                        all.iter().filter_map(Json::as_str).collect()
                    });
                match node.get("properties") {
                    Some(Json::Object(all)) => all
                        .iter()
                        .map(|(k, v)| {
                            self.choice(&format!("{name}.{k}"), v)
                                .map(|c| (k.clone(), c, required.contains(&k.as_str())))
                        })
                        .collect::<Result<_, _>>()
                        .map(Choice::Object),
                    _ => Ok(Choice::Object(Vec::new())),
                }
            }
            _ => Err(wrong),
        }
    }
}
