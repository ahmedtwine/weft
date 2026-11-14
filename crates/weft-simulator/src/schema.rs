use serde_json::{Value as Json, json};
use weft_hegel::{Annotation, Bound, Generator};
use weft_parse::Draw;

const SHAPED: [&str; 7] = ["type", "enum", "const", "anyOf", "oneOf", "allOf", "not"];
const ANY: &str = "";
const ROOT: &str = "#";
const DEFS: &str = "#/$defs/";
const SAFE: i64 = (1 << 53) - 1;
const FORMATS: [(&str, Generator); 9] = [
    ("email", Generator::Email),
    ("uri", Generator::Url),
    ("hostname", Generator::Domain),
    ("uuid", Generator::Uuid),
    ("ipv4", Generator::Ipv4),
    ("ipv6", Generator::Ipv6),
    ("date", Generator::Date),
    ("time", Generator::Time),
    ("date-time", Generator::Datetime),
];

fn any() -> Generator {
    let subtree = || Box::new(Generator::Subtree(ANY.into()));
    Generator::Recursive(
        ANY.into(),
        Box::new(Generator::OneOf(vec![
            Generator::Constant(Json::Null),
            Generator::Boolean,
            Generator::Integer(-SAFE, SAFE),
            Generator::Text(0, None),
            Generator::List(subtree(), 0, None, false),
            Generator::Dict(Box::new(Generator::Text(0, None)), subtree()),
        ])),
        None,
        None,
    )
}

fn required(field: Generator) -> (Generator, bool) {
    match field {
        Generator::Optional(body) => (*body, false),
        field => (field, true),
    }
}

fn merged(a: Generator, b: Generator, name: &str) -> Result<Generator, String> {
    match (a, b) {
        (Generator::Recursive(key, body, depth, leaves), other)
        | (other, Generator::Recursive(key, body, depth, leaves)) => Ok(Generator::Recursive(
            key,
            Box::new(merged(*body, other, name)?),
            depth,
            leaves,
        )),
        (Generator::OneOf(all), other) | (other, Generator::OneOf(all)) => all
            .into_iter()
            .map(|g| merged(g, other.clone(), name))
            .collect::<Result<_, _>>()
            .map(Generator::OneOf),
        (Generator::Record(mut fields), Generator::Record(more)) => {
            for (key, field) in more {
                let (field, needed) = required(field);
                match fields.iter().position(|(k, _)| *k == key) {
                    Some(at) => {
                        let (known, was) = required(fields[at].1.clone());
                        if known != field {
                            return Err(format!(
                                "the field `{name}.{key}` has two schemas in one `allOf`"
                            ));
                        }
                        fields[at].1 = match was || needed {
                            true => known,
                            false => Generator::Optional(Box::new(known)),
                        };
                    }
                    None => fields.push((
                        key,
                        match needed {
                            true => field,
                            false => Generator::Optional(Box::new(field)),
                        },
                    )),
                }
            }
            Ok(Generator::Record(fields))
        }
        _ => Err(format!(
            "the field `{name}` has an `allOf` that is not of objects"
        )),
    }
}

fn keyed(g: &mut Generator, values: &[Json]) -> usize {
    match g {
        Generator::Dict(_, value) => {
            *g = Generator::Record(
                values
                    .iter()
                    .filter_map(Json::as_str)
                    .map(|key| (key.to_string(), Generator::Optional(value.clone())))
                    .collect(),
            );
            1
        }
        Generator::OneOf(all) => all.iter_mut().map(|g| keyed(g, values)).sum(),
        Generator::List(body, ..)
        | Generator::Optional(body)
        | Generator::Add(_, body)
        | Generator::Event(_, body) => keyed(body, values),
        _ => 0,
    }
}

fn fit(g: &mut Generator, draw: &Draw, values: &[Json]) -> usize {
    match (draw.keys, values.is_empty()) {
        (true, _) => return keyed(g, values),
        (false, true) => return 1,
        (false, false) => {}
    }
    let fits: Box<dyn Fn(&Json) -> bool> = match g {
        Generator::SampledFrom(known) => {
            let known = known.clone();
            Box::new(move |v| known.contains(v))
        }
        Generator::Constant(known) => {
            let known = known.clone();
            Box::new(move |v| *v == known)
        }
        Generator::Integer(min, max) => {
            let (min, max) = (*min, *max);
            Box::new(move |v| v.as_i64().is_some_and(|n| (min..=max).contains(&n)))
        }
        Generator::Float(..) => Box::new(Json::is_i64),
        Generator::Text(..)
        | Generator::Regex(_)
        | Generator::Grammar(..)
        | Generator::Email
        | Generator::Url
        | Generator::Domain
        | Generator::Uuid
        | Generator::Ipv4
        | Generator::Ipv6
        | Generator::Date
        | Generator::Time
        | Generator::Datetime => Box::new(Json::is_string),
        Generator::OneOf(all) => return all.iter_mut().map(|g| fit(g, draw, values)).sum(),
        Generator::List(body, ..)
        | Generator::Optional(body)
        | Generator::Add(_, body)
        | Generator::Event(_, body) => return fit(body, draw, values),
        _ => return 0,
    };
    let fitting: Vec<Json> = values.iter().filter(|v| fits(v)).cloned().collect();
    match fitting.is_empty() {
        true => 0,
        false => {
            *g = Generator::Event(
                format!("draw {}", draw.path.join(".")),
                Box::new(Generator::SampledFrom(fitting)),
            );
            1
        }
    }
}

fn narrow(g: &mut Generator, at: &mut Vec<String>, draw: &Draw, values: &[Json]) -> usize {
    match g {
        Generator::Record(fields) => fields
            .iter_mut()
            .map(|(key, field)| {
                at.push(key.clone());
                let found = match at.ends_with(&draw.path) {
                    true => fit(field, draw, values),
                    false => 0,
                };
                let found = match found {
                    0 => narrow(field, at, draw, values),
                    found => found,
                };
                at.pop();
                found
            })
            .sum(),
        Generator::OneOf(all) | Generator::Tuple(all) => {
            all.iter_mut().map(|g| narrow(g, at, draw, values)).sum()
        }
        Generator::List(body, ..)
        | Generator::Dict(_, body)
        | Generator::Optional(body)
        | Generator::Recursive(_, body, ..)
        | Generator::Add(_, body)
        | Generator::Event(_, body) => narrow(body, at, draw, values),
        _ => 0,
    }
}

pub fn narrowed(mut g: Generator, draw: &Draw) -> (Generator, usize) {
    let found = narrow(&mut g, &mut Vec::new(), draw, &draw.values);
    (g, found)
}

pub fn live(g: Generator) -> Option<Generator> {
    let boxed = |g: Generator| live(g).map(Box::new);
    match g {
        Generator::SampledFrom(values) => {
            (!values.is_empty()).then_some(Generator::SampledFrom(values))
        }
        Generator::OneOf(all) => {
            let all: Vec<Generator> = all.into_iter().filter_map(live).collect();
            (!all.is_empty()).then_some(Generator::OneOf(all))
        }
        Generator::Tuple(all) => all
            .into_iter()
            .map(live)
            .collect::<Option<_>>()
            .map(Generator::Tuple),
        Generator::Record(fields) => fields
            .into_iter()
            .filter_map(|(key, field)| match required(field) {
                (field, true) => Some(live(field).map(|field| (key, field))),
                (field, false) => {
                    live(field).map(|field| Some((key, Generator::Optional(Box::new(field)))))
                }
            })
            .collect::<Option<_>>()
            .map(Generator::Record),
        Generator::Optional(body) => {
            Some(boxed(*body).map_or(Generator::Constant(Json::Null), Generator::Optional))
        }
        Generator::List(item, min, max, unique) => match (boxed(*item), min) {
            (Some(item), _) => Some(Generator::List(item, min, max, unique)),
            (None, 0) => Some(Generator::Tuple(Vec::new())),
            (None, _) => None,
        },
        Generator::Dict(key, value) => match (boxed(*key), boxed(*value)) {
            (Some(key), Some(value)) => Some(Generator::Dict(key, value)),
            _ => Some(Generator::Record(Vec::new())),
        },
        Generator::Recursive(key, body, depth, leaves) => {
            boxed(*body).map(|body| Generator::Recursive(key, body, depth, leaves))
        }
        Generator::Add(pool, body) => boxed(*body).map(|body| Generator::Add(pool, body)),
        Generator::Event(label, body) => boxed(*body).map(|body| Generator::Event(label, body)),
        other => Some(other),
    }
}

struct Read<'a> {
    defs: Option<&'a Json>,
    open: Vec<(String, bool)>,
}

impl<'a> Read<'a> {
    fn new(node: &'a Json) -> Read<'a> {
        Read {
            defs: node.get("$defs"),
            open: vec![(ROOT.to_string(), false)],
        }
    }

    fn recursive(
        &mut self,
        name: &str,
        key: &str,
        node: &Json,
        body: Generator,
    ) -> Result<Generator, String> {
        match (
            self.open.pop().is_some_and(|(_, used)| used),
            body.ends(&[key]),
        ) {
            (false, _) => Ok(body),
            (true, true) => {
                let limits = Annotation::of(node)?;
                Ok(Generator::Recursive(
                    key.to_string(),
                    Box::new(body),
                    limits.max_depth,
                    limits.max_leaves,
                ))
            }
            (true, false) => Err(format!(
                "the field `{name}` has the schema `{key}` in each value of `{key}`, and weft draws no value without an end"
            )),
        }
    }

    fn named(&mut self, name: &str, key: &str, wrong: &str) -> Result<Generator, String> {
        if let Some((_, used)) = self.open.iter_mut().find(|(o, _)| o == key) {
            *used = true;
            return Ok(Generator::Subtree(key.to_string()));
        }
        let def = self
            .defs
            .and_then(|all| all.get(key))
            .ok_or_else(|| wrong.to_string())?;
        self.open.push((key.to_string(), false));
        match self.choice(name, def) {
            Ok(body) => self.recursive(name, key, def, body),
            Err(e) => {
                self.open.pop();
                Err(e)
            }
        }
    }

    fn each(&mut self, name: &str, all: &[Json]) -> Result<Vec<Generator>, String> {
        all.iter().map(|b| self.choice(name, b)).collect()
    }

    fn choice(&mut self, name: &str, node: &Json) -> Result<Generator, String> {
        let meta = Annotation::of(node)?;
        match &meta.grammar {
            Some(grammar) => Ok(Generator::Grammar(grammar.clone(), meta.start.clone())),
            None => self.shaped(name, node),
        }
    }

    fn shaped(&mut self, name: &str, node: &Json) -> Result<Generator, String> {
        let int = |k: &str| node.get(k).and_then(Json::as_i64);
        let number = |k: &str| node.get(k).and_then(Json::as_f64);
        let size = |k: &str| node.get(k).and_then(Json::as_u64);
        let text = |k: &str| node.get(k).and_then(Json::as_str);
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
        if let Some(path) = text("$ref") {
            let key = match path {
                ROOT => ROOT,
                other => other.strip_prefix(DEFS).ok_or(&wrong)?,
            };
            return self.named(name, key, &wrong);
        }
        if node.is_object() && !SHAPED.iter().any(|k| node.get(k).is_some()) {
            return Ok(any());
        }
        if node.get("not") == Some(&json!({})) {
            return Ok(Generator::SampledFrom(Vec::new()));
        }
        if let Some(all) = listed("allOf") {
            let mut rest = node.clone();
            rest.as_object_mut().ok_or(&wrong)?.remove("allOf");
            let own = match rest.get("type").is_some() || rest.get("properties").is_some() {
                true => vec![self.shaped(name, &rest)?],
                false => Vec::new(),
            };
            return [own, self.each(name, all)?]
                .concat()
                .into_iter()
                .map(Ok)
                .reduce(|a, b| merged(a?, b?, name))
                .ok_or(wrong)?;
        }
        if let Some(types) = node.get("type").and_then(Json::as_array) {
            return types
                .iter()
                .map(|t| {
                    let mut one = node.clone();
                    one["type"] = t.clone();
                    self.shaped(name, &one)
                })
                .collect::<Result<_, _>>()
                .map(Generator::OneOf);
        }
        match (
            listed("enum"),
            node.get("const"),
            listed("anyOf").or_else(|| listed("oneOf")),
            text("type"),
        ) {
            (Some(values), ..) => Ok(Generator::SampledFrom(values.clone())),
            (_, Some(value), ..) => Ok(Generator::Constant(value.clone())),
            (_, _, Some(all), _) => self.each(name, all).map(Generator::OneOf),
            (.., Some("integer")) => Ok(Generator::Integer(
                int("exclusiveMinimum")
                    .map(|n| n.saturating_add(1))
                    .or(int("minimum"))
                    .unwrap_or(-SAFE),
                int("exclusiveMaximum")
                    .map(|n| n.saturating_sub(1))
                    .or(int("maximum"))
                    .unwrap_or(SAFE),
            )),
            (.., Some("string")) => {
                match (text("pattern"), text("format"), text("contentEncoding")) {
                    (Some(pattern), ..) => Ok(Generator::Regex(pattern.to_string())),
                    (None, _, Some("base64")) => Ok(Generator::Bytes(
                        size("minLength").unwrap_or(0),
                        size("maxLength"),
                    )),
                    (None, Some(format), _) => FORMATS
                        .iter()
                        .find(|(known, _)| *known == format)
                        .map(|(_, g)| g.clone())
                        .ok_or(wrong),
                    (None, None, _) => Ok(Generator::Text(
                        size("minLength").unwrap_or(0),
                        size("maxLength"),
                    )),
                }
            }
            (.., Some("number")) => Ok(Generator::Float(
                bound("minimum", "exclusiveMinimum"),
                bound("maximum", "exclusiveMaximum"),
            )),
            (.., Some("boolean")) => Ok(Generator::Boolean),
            (.., Some("null")) => Ok(Generator::Constant(Json::Null)),
            (.., Some("array")) => match (listed("prefixItems"), node.get("items")) {
                (Some(all), _) => all
                    .iter()
                    .enumerate()
                    .map(|(i, item)| self.choice(&format!("{name}[{i}]"), item))
                    .collect::<Result<_, _>>()
                    .map(Generator::Tuple),
                (None, Some(item)) => Ok(Generator::List(
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
                    None => Generator::Text(0, None),
                };
                Ok(Generator::Dict(
                    Box::new(key),
                    Box::new(self.choice(&format!("{name}{{}}"), &node["additionalProperties"])?),
                ))
            }
            (.., Some("object")) => {
                let needed: Vec<&str> = node
                    .get("required")
                    .and_then(Json::as_array)
                    .map_or(Vec::new(), |all| {
                        all.iter().filter_map(Json::as_str).collect()
                    });
                match node.get("properties") {
                    Some(Json::Object(all)) => all
                        .iter()
                        .map(|(k, v)| {
                            let field = self.choice(&format!("{name}.{k}"), v)?;
                            Ok((
                                k.clone(),
                                match needed.contains(&k.as_str()) {
                                    true => field,
                                    false => Generator::Optional(Box::new(field)),
                                },
                            ))
                        })
                        .collect::<Result<_, String>>()
                        .map(Generator::Record),
                    _ => Ok(Generator::Record(Vec::new())),
                }
            }
            _ => Err(wrong),
        }
    }
}

pub fn whole(name: &str, node: &Json) -> Result<Generator, String> {
    let mut read = Read::new(node);
    let body = read.choice(name, node)?;
    read.recursive(name, ROOT, node, body)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(schema: Json) -> Result<Generator, String> {
        whole("input", &schema)
    }

    fn draw(path: &str, values: &[&str]) -> Draw {
        Draw {
            path: path.split('.').map(String::from).collect(),
            values: values.iter().map(|v| json!(v)).collect(),
            keys: false,
        }
    }

    #[test]
    fn each_annotation_gives_the_generator_of_its_hegel_part() {
        assert_eq!(
            read(
                json!({ "type": "string", "weft": { "grammar": "start: \"a\"", "start": "start" } })
            ),
            Ok(Generator::Grammar(
                "start: \"a\"".into(),
                Some("start".into())
            ))
        );
        assert_eq!(
            read(json!({
                "type": "object",
                "properties": { "children": { "type": "array", "items": { "$ref": "#" } } },
                "required": ["children"],
                "weft": { "maxLeaves": 40, "maxDepth": 6 }
            })),
            Ok(Generator::Recursive(
                ROOT.into(),
                Box::new(Generator::Record(vec![(
                    "children".into(),
                    Generator::List(Box::new(Generator::Subtree(ROOT.into())), 0, None, false)
                )])),
                Some(6),
                Some(40)
            ))
        );
    }

    #[test]
    fn weft_refuses_an_annotation_that_it_does_not_know() {
        let wrong = |weft: Json| read(json!({ "type": "integer", "weft": weft })).unwrap_err();
        assert!(wrong(json!({ "pool": "names" })).contains("is not an annotation of weft"));
        assert!(wrong(json!({ "maxDepth": "deep" })).contains("is not an annotation of weft"));
        assert!(wrong(json!("send")).contains("is not an annotation of weft"));
    }

    #[test]
    fn each_format_and_encoding_gives_the_native_generator() {
        let text = |key: &str, value: &str| read(json!({ "type": "string", key: value }));
        assert_eq!(text("format", "date"), Ok(Generator::Date));
        assert_eq!(text("format", "time"), Ok(Generator::Time));
        assert_eq!(text("format", "date-time"), Ok(Generator::Datetime));
        assert_eq!(text("format", "hostname"), Ok(Generator::Domain));
        assert_eq!(
            text("contentEncoding", "base64"),
            Ok(Generator::Bytes(0, None))
        );
        assert_eq!(text("pattern", "^a$"), Ok(Generator::Regex("^a$".into())));
    }

    #[test]
    fn a_draw_line_gives_the_keys_of_a_record_and_the_values_of_each_key() {
        let element = read(json!({
            "type": "object",
            "properties": {
                "attributes": { "type": "object", "additionalProperties": { "type": "string" } }
            },
            "required": ["attributes"]
        }))
        .unwrap();
        let (keyed, found) = narrowed(
            element,
            &Draw {
                keys: true,
                ..draw("attributes", &["style", "title"])
            },
        );
        assert_eq!(found, 1);
        let optional = |g: Generator| Generator::Optional(Box::new(g));
        assert_eq!(
            keyed,
            Generator::Record(vec![(
                "attributes".into(),
                Generator::Record(vec![
                    ("style".into(), optional(Generator::Text(0, None))),
                    ("title".into(), optional(Generator::Text(0, None))),
                ])
            )])
        );
        let (valued, found) = narrowed(keyed, &draw("attributes.title", &["Sent Jun 11"]));
        assert_eq!(found, 1);
        assert_eq!(
            valued,
            Generator::Record(vec![(
                "attributes".into(),
                Generator::Record(vec![
                    ("style".into(), optional(Generator::Text(0, None))),
                    (
                        "title".into(),
                        optional(Generator::Event(
                            "draw attributes.title".into(),
                            Box::new(Generator::SampledFrom(vec![json!("Sent Jun 11")]))
                        ))
                    ),
                ])
            )])
        );
    }
}
