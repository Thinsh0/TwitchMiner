//! Settings forms generated from the JSON Schemas the engine publishes
//! (`modules_list[].settings_schema`, `settings_get("engine").schema`), so modules
//! added later get a form without interface changes.

use serde_json::{Map, Value, json};
use slint::{ModelRc, SharedString, VecModel};

use crate::{FieldItem, SegmentItem};

/// One editable setting found in a schema.
#[derive(Debug, Clone, PartialEq)]
pub struct Field {
    /// Dotted path inside the scope document (`alerts.at_risk`).
    pub path: String,
    pub kind: Kind,
    pub help: String,
    /// Enum values.
    pub options: Vec<String>,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub nullable: bool,
    pub depth: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Bool,
    Int,
    Number,
    Enum,
    Text,
    List,
    Json,
    /// Heading of a nested object; its fields follow.
    Group,
}

impl Kind {
    fn name(self) -> &'static str {
        match self {
            Kind::Bool => "bool",
            Kind::Int => "int",
            Kind::Number => "number",
            Kind::Enum => "enum",
            Kind::Text => "text",
            Kind::List => "list",
            Kind::Json => "json",
            Kind::Group => "group",
        }
    }
}

/// Resolves a local `$ref` (`#/$defs/Name`).
fn resolve<'a>(root: &'a Value, node: &'a Value) -> &'a Value {
    match node.get("$ref").and_then(|r| r.as_str()) {
        Some(r) => r
            .strip_prefix("#/$defs/")
            .and_then(|name| root["$defs"].get(name))
            .unwrap_or(node),
        None => node,
    }
}

fn types(node: &Value) -> Vec<&str> {
    match &node["type"] {
        Value::String(t) => vec![t.as_str()],
        Value::Array(a) => a.iter().filter_map(|t| t.as_str()).collect(),
        _ => Vec::new(),
    }
}

/// Values of an enum written as `enum: [...]` or `oneOf: [{const}, ...]`.
fn enum_values(node: &Value) -> Option<Vec<String>> {
    if let Some(e) = node["enum"].as_array() {
        return e.iter().map(|v| v.as_str().map(str::to_string)).collect();
    }
    node["oneOf"]
        .as_array()?
        .iter()
        .map(|o| o["const"].as_str().map(str::to_string))
        .collect()
}

/// Every setting of `schema`, depth first, skipping `hidden` top-level keys.
pub fn fields(schema: &Value, hidden: &[&str]) -> Vec<Field> {
    let mut out = Vec::new();
    walk(schema, schema, "", 0, hidden, &mut out);
    out
}

fn walk(
    root: &Value,
    node: &Value,
    prefix: &str,
    depth: usize,
    hidden: &[&str],
    out: &mut Vec<Field>,
) {
    let node = resolve(root, node);
    let Some(props) = node["properties"].as_object() else {
        return;
    };
    let mut keys: Vec<&String> = props.keys().collect();
    // Switches first, then the rest, alphabetically: a stable, readable order.
    keys.sort_by_key(|k| {
        let n = resolve(root, &props[k.as_str()]);
        (types(n) != ["boolean"], k.to_string())
    });
    for key in keys {
        if depth == 0 && hidden.contains(&key.as_str()) {
            continue;
        }
        let raw = &props[key.as_str()];
        let n = resolve(root, raw);
        let path = if prefix.is_empty() {
            key.clone()
        } else {
            format!("{prefix}.{key}")
        };
        let help = raw["description"]
            .as_str()
            .or(n["description"].as_str())
            .unwrap_or_default()
            .to_string();
        let t = types(n);
        let nullable = t.contains(&"null");
        let field = |kind| Field {
            path: path.clone(),
            kind,
            help: help.clone(),
            options: Vec::new(),
            min: n["minimum"].as_f64(),
            max: n["maximum"].as_f64(),
            nullable,
            depth,
        };
        if let Some(options) = enum_values(n) {
            out.push(Field {
                options,
                ..field(Kind::Enum)
            });
        } else if t.contains(&"boolean") {
            out.push(field(Kind::Bool));
        } else if t.contains(&"integer") {
            out.push(field(Kind::Int));
        } else if t.contains(&"number") {
            out.push(field(Kind::Number));
        } else if t.contains(&"string") {
            out.push(field(Kind::Text));
        } else if t.contains(&"array") && types(resolve(root, &n["items"])) == ["string"] {
            out.push(field(Kind::List));
        } else if t.contains(&"object")
            && n["properties"].is_object()
            && n.get("additionalProperties")
                .is_none_or(|a| a == &Value::Bool(false))
        {
            out.push(field(Kind::Group));
            walk(root, n, &path, depth + 1, hidden, out);
        } else {
            out.push(field(Kind::Json));
        }
    }
}

/// Value at a dotted path.
pub fn get<'a>(doc: &'a Value, path: &str) -> &'a Value {
    path.split('.')
        .fold(doc, |v, k| v.get(k).unwrap_or(&Value::Null))
}

/// `{"a": {"b": value}}` for the path `a.b`: a patch to merge into a scope.
pub fn patch(path: &str, value: Value) -> Value {
    path.rsplit('.').fold(value, |inner, k| {
        let mut m = Map::new();
        m.insert(k.to_string(), inner);
        Value::Object(m)
    })
}

/// Parses what the user typed for a field. `Err` = not valid for this field.
pub fn parse(field: &Field, text: &str) -> Result<Value, ()> {
    let t = text.trim();
    match field.kind {
        Kind::Int => {
            let n: i64 = t.parse().map_err(|_| ())?;
            let n = n as f64;
            if field.min.is_some_and(|m| n < m) || field.max.is_some_and(|m| n > m) {
                return Err(());
            }
            Ok(json!(n as i64))
        }
        Kind::Number => {
            let n: f64 = t.replace(',', ".").parse().map_err(|_| ())?;
            if !n.is_finite()
                || field.min.is_some_and(|m| n < m)
                || field.max.is_some_and(|m| n > m)
            {
                return Err(());
            }
            Ok(json!(n))
        }
        Kind::Text if t.is_empty() && field.nullable => Ok(Value::Null),
        Kind::Text => Ok(json!(t)),
        Kind::Json => serde_json::from_str(t).map_err(|_| ()),
        _ => Err(()),
    }
}

/// How a value is shown in a text field.
pub fn text_of(field: &Field, v: &Value) -> String {
    match (field.kind, v) {
        (_, Value::Null) => String::new(),
        (Kind::Json, v) => serde_json::to_string_pretty(v).unwrap_or_default(),
        (_, Value::String(s)) => s.clone(),
        (_, v) => v.to_string(),
    }
}

/// `refresh_minutes` → `Refresh minutes` (labels of settings the views do not know).
pub fn humanize(key: &str) -> String {
    let last = key.rsplit('.').next().unwrap_or(key).replace('_', " ");
    let mut c = last.chars();
    match c.next() {
        Some(f) => f.to_uppercase().chain(c).collect(),
        None => String::new(),
    }
}

/// View rows for `fields` of `section` (a module id or `engine`).
pub fn items(
    section: &str,
    fields: &[Field],
    effective: &Value,
    defaults: &Value,
) -> Vec<FieldItem> {
    fields
        .iter()
        .map(|f| {
            let value = get(effective, &f.path);
            let default = get(defaults, &f.path);
            let list: Vec<SharedString> = value
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str())
                        .map(Into::into)
                        .collect()
                })
                .unwrap_or_default();
            let options: Vec<SegmentItem> = f
                .options
                .iter()
                .map(|o| SegmentItem {
                    label: o.as_str().into(),
                    icon: Default::default(),
                })
                .collect();
            FieldItem {
                section: section.into(),
                path: f.path.as_str().into(),
                key: format!("{section}.{}", f.path).into(),
                fallback_label: humanize(&f.path).into(),
                help: f.help.as_str().into(),
                kind: f.kind.name().into(),
                depth: f.depth as i32,
                on: value.as_bool().unwrap_or_default(),
                text: text_of(f, value).into(),
                selected: value
                    .as_str()
                    .and_then(|v| f.options.iter().position(|o| o == v))
                    .map(|i| i as i32)
                    .unwrap_or(-1),
                options: ModelRc::new(VecModel::from(options)),
                values: ModelRc::new(VecModel::from(list)),
                modified: f.kind != Kind::Group && value != default && !default.is_null(),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn schema() -> Value {
        json!({
            "$defs": {
                "Alerts": {"type": "object", "properties": {
                    "at_risk": {"type": "boolean", "default": true}}},
                "Strategy": {"oneOf": [{"const": "smart", "type": "string"},
                                       {"const": "most_voted", "type": "string"}]}
            },
            "properties": {
                "enabled": {"type": "boolean"},
                "games": {"type": "array", "items": {"type": "string"}},
                "refresh_minutes": {"type": "integer", "minimum": 1, "maximum": 1440},
                "percent": {"type": "number"},
                "proxy": {"type": ["string", "null"]},
                "alerts": {"$ref": "#/$defs/Alerts", "description": "Alerts."},
                "strategy": {"$ref": "#/$defs/Strategy"},
                "targets": {"type": "array", "items": {"type": "object"}}
            }
        })
    }

    #[test]
    fn kinds_are_read_from_the_schema() {
        let f = fields(&schema(), &["enabled"]);
        let kind = |p: &str| f.iter().find(|x| x.path == p).map(|x| x.kind);
        assert_eq!(kind("enabled"), None);
        assert_eq!(kind("games"), Some(Kind::List));
        assert_eq!(kind("refresh_minutes"), Some(Kind::Int));
        assert_eq!(kind("percent"), Some(Kind::Number));
        assert_eq!(kind("proxy"), Some(Kind::Text));
        assert_eq!(kind("alerts"), Some(Kind::Group));
        assert_eq!(kind("alerts.at_risk"), Some(Kind::Bool));
        assert_eq!(kind("strategy"), Some(Kind::Enum));
        assert_eq!(kind("targets"), Some(Kind::Json));
        let s = f.iter().find(|x| x.path == "strategy").unwrap();
        assert_eq!(s.options, vec!["smart", "most_voted"]);
    }

    #[test]
    fn typed_values_are_validated() {
        let f = fields(&schema(), &[]);
        let field = |p: &str| f.iter().find(|x| x.path == p).unwrap().clone();
        assert_eq!(parse(&field("refresh_minutes"), " 30 "), Ok(json!(30)));
        assert_eq!(parse(&field("refresh_minutes"), "0"), Err(()));
        assert_eq!(parse(&field("refresh_minutes"), "abc"), Err(()));
        assert_eq!(parse(&field("percent"), "2,5"), Ok(json!(2.5)));
        assert_eq!(parse(&field("proxy"), ""), Ok(Value::Null));
        assert_eq!(
            parse(&field("targets"), "[{\"kind\": \"x\"}]"),
            Ok(json!([{"kind": "x"}]))
        );
        assert_eq!(parse(&field("targets"), "[oops"), Err(()));
    }

    #[test]
    fn patches_nest_the_path() {
        assert_eq!(
            patch("alerts.at_risk", json!(false)),
            json!({"alerts": {"at_risk": false}})
        );
        assert_eq!(get(&json!({"a": {"b": 3}}), "a.b"), &json!(3));
        assert_eq!(humanize("alerts.new_campaign"), "New campaign");
    }
}
