use crate::{AUTHORING_FLOAT_PRECISION, AuthoringScene};
use serde_json::Value;

pub fn parse_authoring_scene(source: &str) -> Result<AuthoringScene, String> {
    let scene: AuthoringScene = serde_json::from_str(source)
        .map_err(|error| format!("could not parse scene.json: {error}"))?;
    scene.validate()?;
    Ok(scene)
}

pub fn serialize_authoring_scene(scene: &AuthoringScene) -> Result<String, String> {
    scene.validate()?;
    let mut value = serde_json::to_value(scene)
        .map_err(|error| format!("could not prepare scene.json for serialization: {error}"))?;
    normalize_authoring_numbers(&mut value);
    serde_json::to_string_pretty(&value)
        .map(|source| format!("{source}\n"))
        .map_err(|error| format!("could not serialize scene.json: {error}"))
}

fn normalize_authoring_numbers(value: &mut Value) {
    match value {
        Value::Array(values) => {
            for value in values {
                normalize_authoring_numbers(value);
            }
        }
        Value::Object(values) => {
            for value in values.values_mut() {
                normalize_authoring_numbers(value);
            }
        }
        Value::Number(number) if !number.is_i64() && !number.is_u64() => {
            if let Some(number_value) = number.as_f64() {
                let rounded =
                    (number_value * AUTHORING_FLOAT_PRECISION).round() / AUTHORING_FLOAT_PRECISION;
                let rounded = if rounded == -0.0 { 0.0 } else { rounded };
                if let Some(normalized) = serde_json::Number::from_f64(rounded) {
                    *number = normalized;
                }
            }
        }
        _ => {}
    }
}
