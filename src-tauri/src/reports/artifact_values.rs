//! Human-readable artifact values. Source strings remain text, never report markup.
use crate::artifact_view::Column;
use serde_json::Value;

pub fn label(key: &str) -> String {
    let chars: Vec<_> = key.chars().collect();
    let mut result = String::new();
    for (index, &ch) in chars.iter().enumerate() {
        if ch == '_' || ch == '-' {
            result.push(' ');
        } else {
            let boundary = index > 0
                && ch.is_uppercase()
                && (chars[index - 1].is_lowercase()
                    || (chars[index - 1].is_uppercase()
                        && chars.get(index + 1).is_some_and(|next| next.is_lowercase())));
            if boundary {
                result.push(' ');
            }
            if boundary && chars.get(index + 1).is_some_and(|next| next.is_lowercase()) {
                result.extend(ch.to_lowercase());
            } else {
                result.push(ch);
            }
        }
    }
    if let Some(first) = result.chars().next() {
        format!("{}{}", first.to_uppercase(), &result[first.len_utf8()..])
    } else {
        "Value".into()
    }
}

pub fn text(value: &Value) -> String {
    match value {
        Value::Null => "—".into(),
        Value::Bool(value) => if *value { "Yes" } else { "No" }.into(),
        Value::Number(value) => value.to_string(),
        Value::String(value) => value.clone(),
        Value::Array(values) => values.iter().map(text).collect::<Vec<_>>().join("\n"),
        Value::Object(values) => values
            .iter()
            .map(|(key, value)| {
                let value = text(value).replace('\n', "\n  ");
                format!("{}: {value}", label(key))
            })
            .collect::<Vec<_>>()
            .join("\n"),
    }
}

pub fn cell(value: &Value, column: &Column) -> String {
    if matches!(column.format.as_deref(), Some("date" | "datetime")) {
        if let Some(unit) = column.timestamp_unit.as_deref() {
            let numeric = value.as_f64().or_else(|| value.as_str()?.parse().ok());
            if let Some(numeric) = numeric {
                let millis = numeric * if unit == "seconds" { 1000.0 } else { 1.0 };
                if millis.is_finite() && millis >= i64::MIN as f64 && millis < i64::MAX as f64 {
                    if let Some(time) = chrono::DateTime::from_timestamp_millis(millis as i64) {
                        return format!(
                            "{}\nRecorded: {}",
                            time.format("%Y-%m-%d %H:%M:%S%.3f UTC"),
                            text(value)
                        );
                    }
                }
            }
        }
    }
    text(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn renders_nested_fields_without_json_and_preserves_empty_false_and_zero_values() {
        assert_eq!(label("sourceURL"), "Source URL");
        assert_eq!(label("SHA256_hash"), "SHA256 hash");
        let value = text(
            &json!({"senderName":"Alice", "details":{"read":false,"count":0},"phones":["123","456"],"missing":null,"empty":""}),
        );
        assert!(value.contains("Sender name: Alice"));
        assert!(value.contains("Read: No"));
        assert!(value.contains("Count: 0"));
        assert!(value.contains("Phones: 123\n  456"));
        assert!(value.contains("Missing: —"));
        assert!(!value.contains('{'));
        assert!(!value.contains("\"senderName\""));
    }

    #[test]
    fn formats_typed_epoch_columns_with_the_original_value() {
        let mut column = Column {
            key: "time".into(),
            label: "Time".into(),
            format: Some("datetime".into()),
            timestamp_unit: Some("seconds".into()),
        };
        assert_eq!(
            cell(&json!(0), &column),
            "1970-01-01 00:00:00.000 UTC\nRecorded: 0"
        );
        column.timestamp_unit = Some("milliseconds".into());
        assert!(cell(&json!(1000), &column).contains("00:00:01.000 UTC"));
        assert_eq!(cell(&json!("not a timestamp"), &column), "not a timestamp");
    }
}
