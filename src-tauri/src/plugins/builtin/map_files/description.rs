use regex::Regex;
use serde_json::{json, Map, Value};
use std::sync::OnceLock;

struct Patterns {
    rows: Regex,
    cells: Regex,
    tags: Regex,
    entities: Regex,
    speed: Regex,
    speed_limit: Regex,
}

fn patterns() -> &'static Patterns {
    static PATTERNS: OnceLock<Patterns> = OnceLock::new();
    PATTERNS.get_or_init(|| Patterns {
        rows: Regex::new(r"(?is)<tr\b[^>]*>(.*?)</tr\s*>").unwrap(),
        cells: Regex::new(r"(?is)<t[hd]\b[^>]*>(.*?)</t[hd]\s*>").unwrap(),
        tags: Regex::new(r"(?s)<[^>]*>").unwrap(),
        entities: Regex::new(r"(?i)&(?:#x[0-9a-f]+|#\d+|[a-z][a-z0-9]*);").unwrap(),
        speed: Regex::new(r"(?i)\bspeed\s*[:=]\s*([+-]?(?:\d+(?:\.\d+)?|\.\d+)\s*(?:MPH|KPH|km/h|m/s)\b(?:\s*\(\s*[+-]?(?:\d+(?:\.\d+)?|\.\d+)\s*(?:MPH|KPH|km/h|m/s)\s*\))?)").unwrap(),
        speed_limit: Regex::new(r"(?i)\bspeed\s+limit\s*[:=]\s*([+-]?(?:\d+(?:\.\d+)?|\.\d+)(?:\s*(?:MPH|KPH|km/h|m/s)\b)?)").unwrap(),
    })
}

fn cell_text(html: &str, patterns: &Patterns) -> String {
    // Extract text only. The original description remains available as evidence;
    // HTML from a datasource is never executed to display its table fields.
    let text = patterns.tags.replace_all(html, " ");
    let text = patterns
        .entities
        .replace_all(&text, |capture: &regex::Captures<'_>| match &capture[0] {
            "&nbsp;" => " ".to_owned(),
            entity => quick_xml::escape::unescape(entity)
                .map(|text| text.into_owned())
                .unwrap_or_else(|_| entity.to_owned()),
        });
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub(super) fn enrich(properties: &mut Map<String, Value>) {
    let Some(description) = properties.get("description").and_then(Value::as_str) else {
        return;
    };
    let patterns = patterns();
    let mut fields = Map::new();
    let mut speed = None;
    let mut speed_limit = None;
    for row in patterns.rows.captures_iter(description) {
        let mut cells = patterns.cells.captures_iter(&row[1]);
        let (Some(label), Some(value)) = (cells.next(), cells.next()) else {
            continue;
        };
        let label = cell_text(&label[1], patterns);
        let value = cell_text(&value[1], patterns);
        if label.is_empty() {
            continue;
        }
        if label.eq_ignore_ascii_case("speed") && !value.is_empty() {
            speed = Some(value.clone());
        } else if label.eq_ignore_ascii_case("speed limit") && !value.is_empty() {
            speed_limit = Some(value.clone());
        } else if label.eq_ignore_ascii_case("identifier") {
            // This exporter records event speed inside Identifier, while regular
            // track samples may have only time/bearing/distance. Do not derive a
            // speed for those samples or confuse a speed limit with vehicle speed.
            if let Some(capture) = patterns.speed.captures(&value) {
                speed = Some(capture[1].to_owned());
            }
            if let Some(capture) = patterns.speed_limit.captures(&value) {
                speed_limit = Some(capture[1].to_owned());
            }
        }
        match fields.get_mut(&label) {
            Some(Value::Array(values)) => values.push(json!(value)),
            Some(previous) => *previous = json!([previous.clone(), value]),
            None => {
                fields.insert(label, json!(value));
            }
        }
    }
    if !fields.is_empty() {
        properties.insert("descriptionFields".into(), json!(fields));
    }
    if let Some(speed) = speed {
        properties.insert("speed".into(), json!(speed));
    }
    if let Some(limit) = speed_limit {
        properties.insert("speedLimit".into(), json!(limit));
    }
}
