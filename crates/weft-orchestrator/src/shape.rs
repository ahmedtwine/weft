use serde_json::{Map, Value as Json, json};
use weft_parse::Diagram;

use crate::stateful::Members;

const DEPTH: usize = 16;

fn named(reference: &str) -> Option<&str> {
    reference
        .strip_prefix("#/$defs/")
        .or_else(|| reference.strip_prefix("#/definitions/"))
}

fn inlined(node: &Json, root: &Json, depth: usize) -> Json {
    match node {
        Json::Object(all) => {
            let target = all
                .get("$ref")
                .and_then(Json::as_str)
                .and_then(|r| Some((named(r)?, r)))
                .filter(|_| depth < DEPTH)
                .and_then(|(name, r)| {
                    let path = r.trim_start_matches('#');
                    Some((name, root.pointer(path)?))
                });
            match target {
                Some((name, found)) => {
                    let mut out = inlined(found, root, depth + 1);
                    if let Some(out) = out.as_object_mut() {
                        out.entry("title").or_insert_with(|| json!(name));
                    }
                    out
                }
                None => Json::Object(
                    all.iter()
                        .filter(|(k, _)| *k != "$defs" && *k != "definitions")
                        .map(|(k, v)| (k.clone(), inlined(v, root, depth)))
                        .collect::<Map<_, _>>(),
                ),
            }
        }
        Json::Array(all) => Json::Array(all.iter().map(|v| inlined(v, root, depth)).collect()),
        other => other.clone(),
    }
}

pub(crate) fn resolved(schema: &Json) -> Json {
    inlined(schema, schema, 0)
}

fn plain(schema: &Json) -> &Json {
    ["anyOf", "oneOf"]
        .iter()
        .find_map(|k| schema.get(*k)?.as_array())
        .and_then(|all| all.iter().find(|s| s.get("type") != Some(&json!("null"))))
        .map_or(schema, plain)
}

pub(crate) enum Field<'a> {
    Found(&'a Json),
    Open,
    Missing(Vec<String>),
}

pub(crate) fn field<'a>(schema: &'a Json, name: &str) -> Field<'a> {
    match plain(schema).get("properties").and_then(Json::as_object) {
        Some(all) => all.get(name).map_or_else(
            || Field::Missing(all.keys().cloned().collect()),
            Field::Found,
        ),
        None => Field::Open,
    }
}

pub(crate) fn item(schema: &Json) -> Option<&Json> {
    let s = plain(schema);
    s.get("items")
        .or_else(|| s.get("additionalProperties").filter(|a| a.is_object()))
}

fn registered<'a>(members: &'a Members, o: &str, k: &str) -> Result<Option<&'a Json>, String> {
    let tag = format!("{o}.{k}");
    let all = || members.iter().flat_map(|m| &m.hello().schemas);
    if let Some((_, s)) = all().find(|(key, _)| *key == tag) {
        return Ok(Some(s));
    }
    match all().find(|(key, _)| key == o) {
        Some((_, s)) => match field(s, k) {
            Field::Found(s) => Ok(Some(s)),
            Field::Open => Ok(None),
            Field::Missing(fields) => Err(format!(
                "the diagram names `{tag}`, and `weft.state(\"{o}\", …)` has the fields {}",
                fields.join(", ")
            )),
        },
        None => Err(format!(
            "the diagram names `{tag}`, and no runner calls `weft.state(\"{tag}\", …)`"
        )),
    }
}

pub(crate) fn shaped(d: &Diagram, members: &Members) -> Result<(), String> {
    for (o, k) in d.keys() {
        registered(members, o, k)?;
    }
    for (o, k, path) in d.fields() {
        let Some(mut at) = registered(members, o, k)?.and_then(item) else {
            continue;
        };
        for (n, part) in path.iter().enumerate() {
            match field(at, part) {
                Field::Found(s) => at = s,
                Field::Open => break,
                Field::Missing(fields) => {
                    return Err(format!(
                        "the invariant reads `{}` of each item of `{o}.{k}`, and an item has the fields {}",
                        path[..=n].join("."),
                        fields.join(", ")
                    ));
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_ref_inlines_with_its_name() {
        let s = resolved(&json!({
            "type": "array",
            "items": {"$ref": "#/$defs/Parcel"},
            "$defs": {"Parcel": {"type": "object", "properties": {"customer": {"type": "string"}}}}
        }));
        let parcel = item(&s).expect("items");
        assert!(matches!(field(parcel, "customer"), Field::Found(_)));
        assert!(matches!(field(parcel, "custmer"), Field::Missing(_)));
    }

    #[test]
    fn a_map_names_its_values() {
        let s = json!({"type": "object", "additionalProperties": {"type": "object", "title": "Order", "properties": {}}});
        assert!(matches!(field(&s, "o-ana"), Field::Open));
    }
}
