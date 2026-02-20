use std::io::{self, Read};

use anyhow::{Context, Result};
use serde_json::{Map, Value};

fn main() -> Result<()> {
    let input = match std::env::args().nth(1) {
        Some(path) => {
            std::fs::read_to_string(&path).with_context(|| format!("reading {}", path))?
        }
        None => {
            let mut buf = String::new();
            io::stdin().read_to_string(&mut buf)?;
            buf
        }
    };

    let json: Value = serde_json::from_str(&input).context("parsing JSON")?;
    println!("{}", convert(&json));
    Ok(())
}

/// Convert a JSON value to Nix syntax.
fn convert(value: &Value) -> String {
    match value {
        Value::Object(map) => format!("{{\n{}\n}}", emit_bindings(map)),
        other => format_value(other),
    }
}

/// Emit bindings for all entries in a JSON object.
fn emit_bindings(map: &Map<String, Value>) -> String {
    map.iter()
        .map(|(k, v)| emit_binding(&quote_key(k), v))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Emit a single binding, collapsing single-key objects into dot notation.
fn emit_binding(prefix: &str, value: &Value) -> String {
    match value {
        Value::Object(map) => {
            let mut out = String::new();
            for (k, v) in map {
                out.push_str(&emit_binding(&format!("{}.{}", prefix, quote_key(k)), v));
            }
            out
        }
        // Value::Object(map) => {
        //     format!("{} = {{ {} }};", prefix, emit_bindings(map))
        // }
        other => {
            format!("{} = {};", prefix, format_value(other))
        }
    }
}

/// Format a JSON value as a Nix expression.
fn format_value(value: &Value) -> String {
    match value {
        Value::Null => "null".into(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => {
            if s.contains('\n') {
                let mut out = String::new();
                out.push_str("''\n");
                for i in s.lines() {
                    out.push_str(&nix_escape_indented(i));
                    out.push_str("\n")
                }
                out.push_str("''");
                out
            } else {
                format!("\"{}\"", nix_escape(s))
            }
        },
        Value::Array(arr) if arr.is_empty() => "[ ]".into(),
        Value::Array(arr) => {
            let items: Vec<String> = arr.iter().map(format_value).collect();
            format!("[\n{}\n]", items.join("\n"))
        }
        Value::Object(map) => format!("{{ {} }}", emit_bindings(map)),
    }
}

/// Escape a string for use inside Nix double-quoted strings.
fn nix_escape(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\t', "\\t")
        .replace('\r', "\\r")
        .replace("${", "\\${")
}

fn nix_escape_indented(s: &str) -> String {
    if s.contains("''") {
        format!("${{ \"{}\" }}", nix_escape(s))
    } else {
        s.replace('$', "''$")
    }
}

/// Quote an attribute key for Nix.  Bare identifiers matching
/// [a-zA-Z_][0-9a-zA-Z_-]* are left unquoted; everything else is quoted.
fn quote_key(s: &str) -> String {
    let mut chars = s.chars();
    let valid_start = chars
        .next()
        .map_or(false, |c| c.is_ascii_alphabetic() || c == '_');
    let valid_rest = chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');

    if valid_start && valid_rest {
        s.to_string()
    } else {
        format!("\"{}\"", nix_escape(s))
    }
}
