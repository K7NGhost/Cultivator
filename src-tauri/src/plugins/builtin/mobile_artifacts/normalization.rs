use super::*;
use std::collections::HashMap;

fn models() -> &'static HashMap<String, Value> {
    static MODELS: OnceLock<HashMap<String, Value>> = OnceLock::new();
    MODELS.get_or_init(|| {
        let catalog: Value = serde_json::from_str(include_str!(
            "../../../scripting/worker/artifact_model_schemas.json"
        ))
        .expect("valid bundled artifact schemas");
        catalog["models"]
            .as_array()
            .expect("artifact schemas")
            .iter()
            .map(|model| (model["kind"].as_str().unwrap().to_owned(), model.clone()))
            .collect()
    })
}

pub(super) fn prepare(output: &Path) -> Result<Value, String> {
    let module = output.join("leapp_normalization.py");
    let rules = output.join("leapp_normalization_rules.json");
    let schemas = output.join("artifact_model_schemas.json");
    for (path, contents) in [
        (
            &module,
            include_str!("../../../scripting/worker/leapp_normalization.py"),
        ),
        (
            &rules,
            include_str!("../../../scripting/worker/leapp_normalization_rules.json"),
        ),
        (
            &schemas,
            include_str!("../../../scripting/worker/artifact_model_schemas.json"),
        ),
    ] {
        fs::write(path, contents).map_err(|error| error.to_string())?;
    }
    Ok(json!({"modulePath": module, "rulesPath": rules, "schemasPath": schemas}))
}

pub(super) fn artifact(response: &Value) -> Result<BuiltinArtifact, String> {
    let payload = response
        .get("payload")
        .filter(|payload| payload.is_object())
        .ok_or("Invalid mobile artifact payload")?;
    let kind = payload["kind"]
        .as_str()
        .ok_or("Missing mobile artifact kind")?;
    if kind == "custom_table" {
        if !payload["table"]["rows"].is_array() {
            return Err("Invalid mobile artifact table".into());
        }
    } else {
        let model = models()
            .get(kind)
            .ok_or("Unknown standardized mobile artifact kind")?;
        if payload["category"] != model["category"]
            || payload["normalization"]["version"] != 1
            || !payload["raw"]["row"].is_object()
        {
            return Err("Invalid standardized mobile artifact metadata".into());
        }
        for field in model["fields"].as_array().ok_or("Invalid model fields")? {
            let key = field["key"].as_str().ok_or("Invalid model field")?;
            if field["required"] == true && payload.get(key).is_none_or(Value::is_null) {
                return Err(format!("Missing standardized artifact field: {key}"));
            }
        }
    }
    Ok(BuiltinArtifact {
        file_path: response["filePath"]
            .as_str()
            .ok_or("Missing artifact source")?
            .into(),
        kind: kind.into(),
        label: response["label"]
            .as_str()
            .ok_or("Missing artifact label")?
            .into(),
        payload: payload.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn normalized_events_and_fallback_tables_are_accepted_but_unknown_models_are_rejected() {
        let mut event = json!({"filePath":"source.db", "label":"History", "payload":{
            "kind":"browser_history", "category":"browser", "label":"History", "url":"https://example.test",
            "normalization":{"version":1}, "raw":{"row":{"url":"https://example.test"}}
        }});
        assert_eq!(artifact(&event).unwrap().kind, "browser_history");
        event["payload"]["kind"] = json!("unknown_model");
        assert!(artifact(&event).is_err());
        event["payload"] = json!({"kind":"custom_table", "table":{"rows":[]}});
        assert_eq!(artifact(&event).unwrap().kind, "custom_table");
        event["payload"] = json!({"kind":"browser_history", "category":"browser", "normalization":{"version":1}, "raw":{"row":{}}});
        assert!(artifact(&event).is_err());
    }

    #[test]
    fn engine_assets_are_written_outside_the_worker_command_line() {
        let directory = tempfile::tempdir().unwrap();
        let config = prepare(directory.path()).unwrap();
        for key in ["modulePath", "rulesPath", "schemasPath"] {
            assert!(Path::new(config[key].as_str().unwrap()).is_file());
        }
    }
}
