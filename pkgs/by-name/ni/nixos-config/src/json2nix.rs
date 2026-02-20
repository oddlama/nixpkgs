use serde_json::{Map, Value};

/// Convert a JSON value to Nix syntax with proper indentation.
///
/// When `flat` is true, all bindings are emitted with fully-qualified dot paths
/// (no `= { ... };` braces), which is ideal for diffs where every line should
/// show the complete path.
pub fn convert(value: &Value, flat: bool) -> String {
    let mut buf = String::new();
    if flat {
        match value {
            Value::Object(map) if map.is_empty() => buf.push_str("{ }"),
            Value::Object(map) => {
                buf.push_str("{\n");
                emit_bindings_flat(&mut buf, map, 1, "");
                buf.push('}');
            }
            other => format_value(&mut buf, other, 0),
        }
    } else {
        match value {
            Value::Object(map) if map.is_empty() => buf.push_str("{ }"),
            Value::Object(map) => {
                buf.push_str("{\n");
                emit_bindings(&mut buf, map, 1);
                buf.push('}');
            }
            other => format_value(&mut buf, other, 0),
        }
    }
    buf.push('\n');
    buf
}

/// Emit bindings for all entries in a JSON object at the given indentation level.
fn emit_bindings(buf: &mut String, map: &Map<String, Value>, indent: usize) {
    for (k, v) in map {
        emit_binding(buf, &quote_key(k), v, indent);
    }
}

/// Emit a single binding, collapsing single-key objects into dot notation
/// until hitting a multi-key object or leaf value.
fn emit_binding(buf: &mut String, prefix: &str, value: &Value, indent: usize) {
    match value {
        Value::Object(map) if map.len() == 1 => {
            // Single-key collapse: chain into dot notation
            let (k, v) = map.iter().next().unwrap();
            emit_binding(buf, &format!("{}.{}", prefix, quote_key(k)), v, indent);
        }
        Value::Object(map) if map.len() > 1 => {
            // Multi-key: open braces
            write_indent(buf, indent);
            buf.push_str(prefix);
            buf.push_str(" = {\n");
            emit_bindings(buf, map, indent + 1);
            write_indent(buf, indent);
            buf.push_str("};\n");
        }
        Value::Object(_) => {
            // Empty object
            write_indent(buf, indent);
            buf.push_str(prefix);
            buf.push_str(" = { };\n");
        }
        other => {
            write_indent(buf, indent);
            buf.push_str(prefix);
            buf.push_str(" = ");
            format_value(buf, other, indent);
            buf.push_str(";\n");
        }
    }
}

/// Emit bindings in flat mode — all keys use fully-qualified dot paths, no braces.
fn emit_bindings_flat(buf: &mut String, map: &Map<String, Value>, indent: usize, prefix: &str) {
    for (k, v) in map {
        let key = if prefix.is_empty() {
            quote_key(k)
        } else {
            format!("{}.{}", prefix, quote_key(k))
        };
        emit_binding_flat(buf, &key, v, indent);
    }
}

/// Emit a single flat binding — recurse into objects accumulating dot-prefix,
/// only emit `key = value;` at leaf values.
fn emit_binding_flat(buf: &mut String, prefix: &str, value: &Value, indent: usize) {
    match value {
        Value::Object(map) if map.is_empty() => {
            write_indent(buf, indent);
            buf.push_str(prefix);
            buf.push_str(" = { };\n");
        }
        Value::Object(map) => {
            emit_bindings_flat(buf, map, indent, prefix);
        }
        other => {
            write_indent(buf, indent);
            buf.push_str(prefix);
            buf.push_str(" = ");
            format_value(buf, other, indent);
            buf.push_str(";\n");
        }
    }
}

/// Format a JSON value as a Nix expression, appending to buf.
fn format_value(buf: &mut String, value: &Value, indent: usize) {
    match value {
        Value::Null => buf.push_str("null"),
        Value::Bool(b) => buf.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => buf.push_str(&n.to_string()),
        Value::String(s) => {
            if s.contains('\n') {
                buf.push_str("''\n");
                for line in s.lines() {
                    write_indent(buf, indent + 1);
                    buf.push_str(&nix_escape_indented(line));
                    buf.push('\n');
                }
                write_indent(buf, indent);
                buf.push_str("''");
            } else {
                buf.push('"');
                buf.push_str(&nix_escape(s));
                buf.push('"');
            }
        }
        Value::Array(arr) if arr.is_empty() => buf.push_str("[ ]"),
        Value::Array(arr) => {
            buf.push_str("[\n");
            for item in arr {
                write_indent(buf, indent + 1);
                format_value(buf, item, indent + 1);
                buf.push('\n');
            }
            write_indent(buf, indent);
            buf.push(']');
        }
        Value::Object(map) if map.is_empty() => buf.push_str("{ }"),
        Value::Object(map) => {
            buf.push_str("{\n");
            emit_bindings(buf, map, indent + 1);
            write_indent(buf, indent);
            buf.push('}');
        }
    }
}

fn write_indent(buf: &mut String, indent: usize) {
    for _ in 0..indent {
        buf.push_str("  ");
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

/// Escape a string for use inside Nix indented ('' ... '') strings.
fn nix_escape_indented(s: &str) -> String {
    if s.contains("''") {
        format!("${{\"{}\" }}", nix_escape(s))
    } else {
        s.replace('$', "''$")
    }
}

/// Quote an attribute key for Nix. Bare identifiers matching
/// [a-zA-Z_][0-9a-zA-Z_-]* are left unquoted; everything else is quoted.
fn quote_key(s: &str) -> String {
    let mut chars = s.chars();
    let valid_start = chars
        .next()
        .map_or(false, |c| c.is_ascii_alphabetic() || c == '_');
    let valid_rest = chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');

    if valid_start && valid_rest && !s.is_empty() {
        s.to_string()
    } else {
        format!("\"{}\"", nix_escape(s))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn simple_flat() {
        let v = json!({"a": true, "b": 42});
        let out = convert(&v, false);
        assert!(out.contains("a = true;"));
        assert!(out.contains("b = 42;"));
    }

    #[test]
    fn single_key_collapse() {
        let v = json!({"services": {"openssh": {"enable": true}}});
        let out = convert(&v, false);
        assert!(out.contains("services.openssh.enable = true;"));
    }

    #[test]
    fn multi_key_braces() {
        let v = json!({"boot": {"a": 1, "b": 2}});
        let out = convert(&v, false);
        assert!(out.contains("boot = {"));
        assert!(out.contains("a = 1;"));
        assert!(out.contains("b = 2;"));
    }

    #[test]
    fn empty_object() {
        let v = json!({});
        assert_eq!(convert(&v, false), "{ }\n");
    }

    #[test]
    fn string_escaping() {
        let v = json!({"key": "hello \"world\""});
        let out = convert(&v, false);
        assert!(out.contains("\"hello \\\"world\\\"\""));
    }

    #[test]
    fn multiline_string() {
        let v = json!({"key": "line1\nline2"});
        let out = convert(&v, false);
        assert!(out.contains("''"));
    }

    #[test]
    fn array_values() {
        let v = json!({"list": [1, 2, 3]});
        let out = convert(&v, false);
        assert!(out.contains("["));
        assert!(out.contains("1"));
        assert!(out.contains("]"));
    }

    #[test]
    fn flat_mode_no_braces() {
        let v = json!({"boot": {"a": 1, "b": 2}});
        let out = convert(&v, true);
        // Flat mode: no braces, full dot paths
        assert!(out.contains("boot.a = 1;"));
        assert!(out.contains("boot.b = 2;"));
        assert!(!out.contains("boot = {"));
    }

    #[test]
    fn flat_mode_deep_nesting() {
        let v = json!({"services": {"openssh": {"enable": true, "port": 22}}});
        let out = convert(&v, true);
        assert!(out.contains("services.openssh.enable = true;"));
        assert!(out.contains("services.openssh.port = 22;"));
        assert!(!out.contains("= {"));
    }
}
