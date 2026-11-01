use serde_json::Value;

/// Extracts a value from a JSON document using dot-separated keys. Numeric
/// segments are treated as array indexes. Returns the textual
/// representation of the value, or `None` if the path is invalid or the
/// input is not parseable JSON.
pub fn extract(json_text: &str, path: &str) -> Option<String> {
    if path.is_empty() {
        return Some(json_text.to_string());
    }
    let root: Value = serde_json::from_str(json_text).ok()?;
    let mut cursor = root;
    for segment in path.split('.') {
        cursor = step(&cursor, segment)?;
    }
    match cursor {
        Value::String(s) => Some(s),
        other => Some(other.to_string()),
    }
}

fn step(node: &Value, segment: &str) -> Option<Value> {
    match node {
        Value::Object(map) => map.get(segment).cloned(),
        Value::Array(arr) => segment.parse::<usize>().ok().and_then(|i| arr.get(i).cloned()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_top_level_string() {
        assert_eq!(extract(r#"{"msg":"hi"}"#, "msg"), Some("hi".to_string()));
    }

    #[test]
    fn walks_nested_objects() {
        let s = r#"{"a":{"b":{"c":"deep"}}}"#;
        assert_eq!(extract(s, "a.b.c"), Some("deep".to_string()));
    }

    #[test]
    fn numeric_segment_indexes_arrays() {
        let s = r#"{"choices":[{"text":"first"},{"text":"second"}]}"#;
        assert_eq!(extract(s, "choices.1.text"), Some("second".to_string()));
    }

    #[test]
    fn chatgpt_style_path_works() {
        let s = r#"{"choices":[{"message":{"content":"hello"}}]}"#;
        assert_eq!(extract(s, "choices.0.message.content"), Some("hello".to_string()));
    }

    #[test]
    fn unknown_path_returns_none() {
        assert_eq!(extract(r#"{"a":1}"#, "b"), None);
    }

    #[test]
    fn invalid_json_returns_none() {
        assert_eq!(extract("not json", "a"), None);
    }

    #[test]
    fn empty_path_returns_raw_body() {
        assert_eq!(extract(r#"{"a":1}"#, ""), Some(r#"{"a":1}"#.to_string()));
    }
}
