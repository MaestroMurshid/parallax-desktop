//! Making llama.cpp's schemas acceptable to a cloud API, and its replies look
//! like llama.cpp's again.
//!
//! The enrichment schemas were written for a grammar: `maxLength` and
//! `maxItems` are how a runaway string is stopped there, and a root `anyOf` is
//! how the judge says "none" in one token. Hosted structured-output modes
//! accept a narrower dialect -- no length or count constraints, an object at
//! the root, and (OpenAI's strict mode) every property listed as required.
//!
//! So the schema is narrowed on the way out and the reply is put back on the
//! way in: the dropped bounds are applied to what comes back, a wrapped root is
//! unwrapped, and the nulls standing in for absent optional fields are removed.
//! Everything above `LlmProvider` sees the same shape it always did.

use crate::error::{Error, Result};
use serde_json::{json, Map, Value};

/// Keywords no hosted structured-output mode reliably accepts. Each is
/// re-applied to the reply by `enforce`, where it has a meaning there.
const DROPPED: &[&str] = &[
    "maxLength",
    "minLength",
    "maxItems",
    "minItems",
    "minimum",
    "maximum",
    "exclusiveMinimum",
    "exclusiveMaximum",
    "multipleOf",
    "pattern",
];

/// The property a non-object root is wrapped in.
const WRAP: &str = "reply";

pub struct Portable {
    pub schema: Value,
    wrapped: bool,
}

/// The schema to send. `all_required` is OpenAI's strict mode: every property
/// must be required, so the optional ones are made nullable instead.
pub fn portable(schema: &Value, all_required: bool) -> Portable {
    let mut narrowed = schema.clone();
    narrow(&mut narrowed, all_required);
    if narrowed.get("type").and_then(Value::as_str) == Some("object") {
        return Portable {
            schema: narrowed,
            wrapped: false,
        };
    }
    Portable {
        schema: json!({
            "type": "object",
            "properties": { WRAP: narrowed },
            "required": [WRAP],
            "additionalProperties": false,
        }),
        wrapped: true,
    }
}

/// Walks schema positions only -- property *names* are never touched, so a
/// field that happened to be called `pattern` would survive.
fn narrow(node: &mut Value, all_required: bool) {
    let Some(obj) = node.as_object_mut() else {
        return;
    };
    for key in DROPPED {
        obj.remove(*key);
    }
    // `const` is the newest of these keywords and the least consistently
    // supported; a one-member enum says the same thing everywhere.
    if let Some(value) = obj.remove("const") {
        if let Some(s) = value.as_str() {
            obj.insert("type".into(), json!("string"));
            obj.insert("enum".into(), json!([s]));
        } else {
            obj.insert("enum".into(), json!([value]));
        }
    }
    if let Some(Value::Object(props)) = obj.get_mut("properties") {
        for prop in props.values_mut() {
            narrow(prop, all_required);
        }
    }
    if all_required {
        make_all_required(obj);
    }
    if let Some(items) = obj.get_mut("items") {
        narrow(items, all_required);
    }
    for key in ["anyOf", "allOf", "oneOf"] {
        if let Some(Value::Array(branches)) = obj.get_mut(key) {
            for branch in branches {
                narrow(branch, all_required);
            }
        }
    }
    for key in ["$defs", "definitions"] {
        if let Some(Value::Object(defs)) = obj.get_mut(key) {
            for def in defs.values_mut() {
                narrow(def, all_required);
            }
        }
    }
}

fn make_all_required(obj: &mut Map<String, Value>) {
    let Some(Value::Object(props)) = obj.get("properties") else {
        return;
    };
    let required: Vec<String> = obj
        .get("required")
        .and_then(Value::as_array)
        .map(|r| {
            r.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    let optional: Vec<String> = props
        .keys()
        .filter(|k| !required.contains(k))
        .cloned()
        .collect();
    if optional.is_empty() {
        return;
    }
    let Some(Value::Object(props)) = obj.get_mut("properties") else {
        return;
    };
    for name in &optional {
        if let Some(prop) = props.get_mut(name) {
            let original = prop.take();
            *prop = json!({ "anyOf": [original, { "type": "null" }] });
        }
    }
    let mut all = required;
    all.extend(optional);
    obj.insert("required".into(), json!(all));
}

/// The reply, as llama.cpp would have produced it against `original`.
pub fn conform(original: &Value, portable: &Portable, reply: &str) -> Result<String> {
    let mut value: Value = serde_json::from_str(reply.trim()).map_err(|e| {
        Error::Other(format!(
            "the model's reply was not the JSON it was asked for: {e}"
        ))
    })?;
    if portable.wrapped {
        value = value
            .get_mut(WRAP)
            .map(Value::take)
            .ok_or_else(|| Error::Other("the model's reply was missing its contents".into()))?;
    }
    strip_nulls(&mut value);
    enforce(original, &mut value);
    Ok(value.to_string())
}

/// Optional fields come back as `null` under strict mode; absent is what the
/// parsers above expect, and `#[serde(default)]` does not cover null.
fn strip_nulls(value: &mut Value) {
    match value {
        Value::Object(obj) => {
            obj.retain(|_, v| !v.is_null());
            for v in obj.values_mut() {
                strip_nulls(v);
            }
        }
        Value::Array(items) => {
            for v in items {
                strip_nulls(v);
            }
        }
        _ => {}
    }
}

/// Re-applies the bounds `narrow` dropped. Cuts land where the grammar's would
/// have -- on a character, mid-word if need be -- because the readable trims
/// downstream (`trim_title`, `trim_phrase`) were written for exactly that.
fn enforce(schema: &Value, value: &mut Value) {
    let Some(rules) = schema.as_object() else {
        return;
    };
    match value {
        Value::String(s) => {
            if let Some(max) = rules.get("maxLength").and_then(Value::as_u64) {
                if s.chars().count() > max as usize {
                    *s = s.chars().take(max as usize).collect();
                }
            }
        }
        Value::Array(items) => {
            if let Some(max) = rules.get("maxItems").and_then(Value::as_u64) {
                items.truncate(max as usize);
            }
            if let Some(item_schema) = rules.get("items") {
                for item in items.iter_mut() {
                    enforce(item_schema, item);
                }
            }
        }
        Value::Object(fields) => {
            if let Some(Value::Object(props)) = rules.get("properties") {
                for (name, field) in fields.iter_mut() {
                    if let Some(prop) = props.get(name) {
                        enforce(prop, field);
                    }
                }
            }
        }
        _ => {}
    }
    // Every branch's bounds apply where its fields appear; a branch that does
    // not describe this value has no properties in it to touch.
    for key in ["anyOf", "allOf", "oneOf"] {
        if let Some(Value::Array(branches)) = rules.get(key) {
            for branch in branches {
                enforce(branch, value);
            }
        }
    }
}

/// A key sent in the clear is a key given away. Plain http is allowed only to
/// this machine, where there is no wire to read it off.
pub fn check_endpoint(base_url: &str) -> Result<()> {
    let url = base_url.trim();
    if url.starts_with("https://") {
        return Ok(());
    }
    let local = ["http://localhost", "http://127.0.0.1", "http://[::1]"];
    if local.iter().any(|p| {
        url.strip_prefix(p)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with(':') || rest.starts_with('/'))
    }) {
        return Ok(());
    }
    Err(Error::Other(
        "the endpoint must start with https:// (plain http only to this computer)".into(),
    ))
}

/// What OpenAI's strict mode and Anthropic's structured outputs both demand
/// of a schema, checked over every object in it. For the enrichment schemas'
/// own tests: each is written for llama.cpp, and this is the proof it still
/// survives the trip to a cloud provider.
#[cfg(test)]
pub(crate) fn assert_portable(schema: &Value) {
    fn walk(node: &Value, path: &str) {
        let Some(obj) = node.as_object() else {
            return;
        };
        for key in DROPPED {
            assert!(obj.get(*key).is_none(), "{path} still has {key}");
        }
        assert!(obj.get("const").is_none(), "{path} still has const");
        if let Some(Value::Object(props)) = obj.get("properties") {
            assert_eq!(
                obj.get("additionalProperties"),
                Some(&json!(false)),
                "{path} allows extra properties"
            );
            let required: Vec<&str> = obj["required"]
                .as_array()
                .unwrap_or_else(|| panic!("{path} has no required list"))
                .iter()
                .filter_map(Value::as_str)
                .collect();
            for (name, prop) in props {
                assert!(
                    required.contains(&name.as_str()),
                    "{path}.{name} is not required"
                );
                walk(prop, &format!("{path}.{name}"));
            }
        }
        if let Some(items) = obj.get("items") {
            walk(items, &format!("{path}[]"));
        }
        for key in ["anyOf", "allOf", "oneOf"] {
            if let Some(Value::Array(branches)) = obj.get(key) {
                for (i, b) in branches.iter().enumerate() {
                    walk(b, &format!("{path}.{key}[{i}]"));
                }
            }
        }
    }
    let strict = portable(schema, true).schema;
    assert_eq!(strict["type"], "object", "the root must be an object");
    walk(&strict, "$");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn classify_like() -> Value {
        json!({
            "type": "object",
            "properties": {
                "title": { "type": "string", "maxLength": 5 },
                "tags": { "type": "array", "items": { "type": "string", "maxLength": 3 }, "maxItems": 2 },
                "pattern": { "type": "string" },
            },
            "required": ["title"],
            "additionalProperties": false,
        })
    }

    #[test]
    fn bounds_are_dropped_but_property_names_survive() {
        let p = portable(&classify_like(), false);
        assert!(p.schema["properties"]["title"].get("maxLength").is_none());
        assert!(p.schema["properties"]["tags"].get("maxItems").is_none());
        assert!(p.schema["properties"]["pattern"].is_object());
        assert_eq!(p.schema["required"], json!(["title"]));
    }

    #[test]
    fn strict_mode_makes_optional_fields_required_and_nullable() {
        let p = portable(&classify_like(), true);
        assert_eq!(p.schema["required"], json!(["title", "tags", "pattern"]));
        assert_eq!(
            p.schema["properties"]["tags"]["anyOf"][1],
            json!({ "type": "null" })
        );
        // Required ones are left as they were.
        assert_eq!(p.schema["properties"]["title"]["type"], "string");
    }

    #[test]
    fn dropped_bounds_are_applied_to_the_reply() {
        let schema = classify_like();
        let p = portable(&schema, true);
        let out = conform(
            &schema,
            &p,
            r#"{"title":"indexing dilemma","tags":["alpha","beta","gamma"],"pattern":null}"#,
        )
        .unwrap();
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["title"], "index");
        assert_eq!(v["tags"], json!(["alp", "bet"]));
        assert!(v.get("pattern").is_none(), "null stands for absent");
    }

    /// The judge's shape: `anyOf` at the root, one branch a bare "none".
    #[test]
    fn a_root_any_of_is_wrapped_and_unwrapped() {
        let schema = json!({
            "anyOf": [
                { "type": "object", "properties": { "relation": { "const": "none" } },
                  "required": ["relation"], "additionalProperties": false },
                { "type": "object", "properties": {
                    "relation": { "type": "string", "enum": ["supports"] },
                    "quoteA": { "type": "string", "maxLength": 4 } },
                  "required": ["relation", "quoteA"], "additionalProperties": false },
            ],
        });
        let p = portable(&schema, false);
        assert_eq!(p.schema["type"], "object");
        assert_eq!(
            p.schema["properties"]["reply"]["anyOf"][0]["properties"]["relation"],
            json!({ "type": "string", "enum": ["none"] })
        );

        let out = conform(
            &schema,
            &p,
            r#"{"reply":{"relation":"supports","quoteA":"word for word"}}"#,
        )
        .unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&out).unwrap(),
            json!({ "relation": "supports", "quoteA": "word" })
        );
    }

    #[test]
    fn a_reply_that_is_not_json_is_an_error_not_a_guess() {
        let schema = classify_like();
        let p = portable(&schema, false);
        assert!(conform(&schema, &p, "Sure! Here is the JSON").is_err());
    }

    #[test]
    fn keys_only_travel_encrypted_or_stay_on_this_machine() {
        assert!(check_endpoint("https://api.openai.com/v1").is_ok());
        assert!(check_endpoint("http://localhost:1234/v1").is_ok());
        assert!(check_endpoint("http://127.0.0.1/v1").is_ok());
        assert!(check_endpoint("http://192.168.1.4:8080/v1").is_err());
        assert!(check_endpoint("http://localhost.evil.com/v1").is_err());
        assert!(check_endpoint("ftp://example.com").is_err());
    }
}
