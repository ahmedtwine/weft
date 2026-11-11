use serde_json::Value as Json;

const KEY: &str = "weft";

#[derive(Clone, Copy, PartialEq)]
enum Type {
    Text,
    Flag,
    Count,
}

const KEYS: [(&str, Type, &str); 9] = [
    (
        "verb",
        Type::Text,
        "The branch of an input that is one action of this verb.",
    ),
    (
        "read",
        Type::Text,
        "The value of `<object>.<key>` in the state.",
    ),
    (
        "pool",
        Type::Text,
        "Each drawn value of this part goes to this Hegel pool.",
    ),
    (
        "from",
        Type::Text,
        "This part is a value of this Hegel pool.",
    ),
    (
        "consume",
        Type::Flag,
        "A `from` draw removes the value from the pool.",
    ),
    (
        "maxLeaves",
        Type::Count,
        "The most leaves of one value of a schema that names itself.",
    ),
    (
        "maxDepth",
        Type::Count,
        "The most levels of one value of a schema that names itself.",
    ),
    (
        "grammar",
        Type::Text,
        "A Lark grammar of the texts of this part.",
    ),
    ("start", Type::Text, "The start rule of the grammar."),
];

#[derive(Clone, Default, PartialEq)]
pub struct Annotation {
    pub verb: Option<String>,
    pub read: Option<String>,
    pub pool: Option<String>,
    pub from: Option<String>,
    pub consume: bool,
    pub max_leaves: Option<u64>,
    pub max_depth: Option<u64>,
    pub grammar: Option<String>,
    pub start: Option<String>,
}

impl Annotation {
    pub fn of(node: &Json) -> Result<Annotation, String> {
        let Some(given) = node.get(KEY) else {
            return Ok(Annotation::default());
        };
        let wrong = || format!("`{KEY}: {given}` is not an annotation of weft");
        let fields = given.as_object().ok_or_else(wrong)?;
        for (key, value) in fields {
            let (_, ty, _) = KEYS
                .iter()
                .find(|(known, ..)| known == key)
                .ok_or_else(wrong)?;
            let fits = match ty {
                Type::Text => value.is_string(),
                Type::Flag => value.is_boolean(),
                Type::Count => value.is_u64(),
            };
            if !fits {
                return Err(wrong());
            }
        }
        let text = |key: &str| fields.get(key).and_then(Json::as_str).map(String::from);
        let count = |key: &str| fields.get(key).and_then(Json::as_u64);
        Ok(Annotation {
            verb: text("verb"),
            read: text("read"),
            pool: text("pool"),
            from: text("from"),
            consume: fields.get("consume").and_then(Json::as_bool) == Some(true),
            max_leaves: count("maxLeaves"),
            max_depth: count("maxDepth"),
            grammar: text("grammar"),
            start: text("start"),
        })
    }
}

pub fn schema() -> String {
    let properties: String = KEYS
        .iter()
        .map(|(key, ty, description)| {
            let ty = match ty {
                Type::Text => "\"type\": \"string\"",
                Type::Flag => "\"type\": \"boolean\"",
                Type::Count => "\"type\": \"integer\",\n      \"minimum\": 0",
            };
            format!(
                "    \"{key}\": {{\n      {ty},\n      \"description\": {}\n    }}",
                Json::from(*description)
            )
        })
        .collect::<Vec<_>>()
        .join(",\n");
    format!(
        "{{\n  \"$schema\": \"https://json-schema.org/draft/2020-12/schema\",\n  \"title\": \"WeftMeta\",\n  \"description\": \"The `weft` annotation of a part of a schema.\",\n  \"type\": \"object\",\n  \"properties\": {{\n{properties}\n  }},\n  \"additionalProperties\": false\n}}\n"
    )
}

pub fn types() -> String {
    let fields: String = KEYS
        .iter()
        .map(|(key, ty, _)| {
            let ty = match ty {
                Type::Text => "string",
                Type::Flag => "boolean",
                Type::Count => "number",
            };
            format!("  {key}?: {ty};\n")
        })
        .collect();
    format!(
        "export type WeftMeta = {{\n{fields}}};\n\ndeclare module \"zod\" {{\n  interface GlobalMeta {{\n    weft?: WeftMeta;\n  }}\n}}\n"
    )
}
