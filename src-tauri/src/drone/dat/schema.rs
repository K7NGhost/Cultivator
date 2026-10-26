use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Clone, Debug, Serialize)]
pub struct Field {
    pub name: String,
    pub kind: String,
    pub offset: usize,
    pub size: usize,
}

#[derive(Clone, Debug, Serialize)]
pub struct Definition {
    pub name: String,
    pub id: u16,
    pub length: usize,
    pub fields: Vec<Field>,
}

pub fn parse_definitions(text: &str) -> BTreeMap<u16, Definition> {
    let mut definitions = BTreeMap::new();
    let mut current: Option<Definition> = None;
    let mut binary = false;
    let mut name = String::new();
    for line in text.lines() {
        let tokens: Vec<_> = line.split_whitespace().collect();
        let Some(first) = tokens.first() else {
            continue;
        };
        let token = first.to_ascii_lowercase();
        if token == "op.[config]" || token == "op.[string]" {
            if let Some(def) = current.take() {
                definitions.insert(def.id, def);
            }
            binary = token == "op.[config]";
            name.clear();
        } else if token == "name" {
            name = tokens[1..].join("_");
        } else if token == "type" && binary {
            if let Some(id) = tokens.get(1).and_then(|v| v.parse().ok()) {
                current = Some(Definition {
                    name: name.clone(),
                    id,
                    length: 0,
                    fields: vec![],
                });
            }
        } else if let Some(kind) = token.strip_prefix("op.") {
            let size = match kind {
                "uint8_t" | "int8_t" => 1,
                "uint16_t" | "int16_t" => 2,
                "uint32_t" | "int32_t" | "fp32" | "float" => 4,
                "double" | "fp64" => 8,
                "expr" => continue,
                // An unknown storage type makes subsequent offsets unsafe.
                _ => {
                    current = None;
                    continue;
                }
            };
            if let (Some(def), Some(field_name)) = (current.as_mut(), tokens.get(1)) {
                def.fields.push(Field {
                    name: (*field_name).to_string(),
                    kind: kind.to_string(),
                    offset: def.length,
                    size,
                });
                def.length += size;
            }
        }
    }
    if let Some(def) = current {
        definitions.insert(def.id, def);
    }
    definitions
}

pub fn read_number(payload: &[u8], field: &Field) -> Option<f64> {
    let bytes = payload.get(field.offset..field.offset.checked_add(field.size)?)?;
    let value = match field.kind.as_str() {
        "uint8_t" => bytes[0] as f64,
        "int8_t" => (bytes[0] as i8) as f64,
        "uint16_t" => u16::from_le_bytes(bytes.try_into().ok()?) as f64,
        "int16_t" => i16::from_le_bytes(bytes.try_into().ok()?) as f64,
        "uint32_t" => u32::from_le_bytes(bytes.try_into().ok()?) as f64,
        "int32_t" => i32::from_le_bytes(bytes.try_into().ok()?) as f64,
        "fp32" | "float" => f32::from_le_bytes(bytes.try_into().ok()?) as f64,
        "fp64" | "double" => f64::from_le_bytes(bytes.try_into().ok()?),
        _ => return None,
    };
    value.is_finite().then_some(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_packed_fields_and_checks_bounds() {
        let defs = parse_definitions(
            "Op.[config]\nname test\ntype 42\nOp.uint8_t flag 0\nOp.fp32 value 0\nOp.expr computed value\nOp.uint16_t count 0\n",
        );
        let def = &defs[&42];
        assert_eq!(def.length, 7);
        assert_eq!(def.fields[1].offset, 1);
        let mut bytes = vec![1];
        bytes.extend(1.25f32.to_le_bytes());
        bytes.extend(600u16.to_le_bytes());
        assert_eq!(read_number(&bytes, &def.fields[1]), Some(1.25));
        assert_eq!(read_number(&bytes[..6], &def.fields[2]), None);
    }
}
