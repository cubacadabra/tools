use crate::{BuildError, Result};
use serde_json::{Map, Value, json};

const TERRAIN_MATERIALS: &[&str] = &[
    "grass",
    "ground",
    "dirt",
    "rock",
    "sand",
    "mud",
    "snow",
    "leafygrass",
];
pub(super) fn integer(
    config: &Map<String, Value>,
    key: &str,
    default: usize,
    minimum: usize,
    maximum: usize,
) -> Result<usize> {
    let Some(raw) = config.get(key) else {
        return Ok(default);
    };
    let Some(raw) = raw.as_u64() else {
        return Err(BuildError(format!(
            "manifest.maze.{key} must be an integer between {minimum} and {maximum}"
        )));
    };
    let value = usize::try_from(raw).unwrap_or(usize::MAX);
    if value < minimum || value > maximum {
        return Err(BuildError(format!(
            "manifest.maze.{key} must be an integer between {minimum} and {maximum}"
        )));
    }
    Ok(value)
}

pub(super) fn number(
    config: &Map<String, Value>,
    key: &str,
    default: f64,
    minimum: f64,
    maximum: f64,
) -> Result<f64> {
    let Some(value) = config.get(key) else {
        return Ok(default);
    };
    let value = value.as_f64().ok_or_else(|| {
        BuildError(format!(
            "manifest.maze.{key} must be between {minimum} and {maximum}"
        ))
    })?;
    if !value.is_finite() || value < minimum || value > maximum {
        return Err(BuildError(format!(
            "manifest.maze.{key} must be between {minimum} and {maximum}"
        )));
    }
    Ok(value)
}

pub(super) fn cell(
    config: &Map<String, Value>,
    key: &str,
    default: (usize, usize),
    width: usize,
    height: usize,
) -> Result<(usize, usize)> {
    let Some(value) = config.get(key) else {
        return Ok(default);
    };
    let values = value.as_array().ok_or_else(|| {
        BuildError(format!(
            "manifest.maze.{key} must be a two-item cell coordinate inside the maze"
        ))
    })?;
    if values.len() != 2 || values.iter().any(|value| value.as_u64().is_none()) {
        return Err(BuildError(format!(
            "manifest.maze.{key} must be a two-item cell coordinate inside the maze"
        )));
    }
    let result = (
        values[0].as_u64().unwrap() as usize,
        values[1].as_u64().unwrap() as usize,
    );
    if result.0 >= width || result.1 >= height {
        return Err(BuildError(format!(
            "manifest.maze.{key} must be a two-item cell coordinate inside the maze"
        )));
    }
    Ok(result)
}

pub(super) fn origin(
    config: &Map<String, Value>,
    width: usize,
    height: usize,
    cell_size: f64,
) -> Result<[f64; 3]> {
    let default = [
        -(width as f64 * cell_size) / 2.0,
        0.0,
        -(height as f64 * cell_size) / 2.0,
    ];
    let Some(value) = config.get("origin") else {
        return Ok(default);
    };
    let values = value.as_array().ok_or_else(|| {
        BuildError("manifest.maze.origin must contain three finite numbers".to_owned())
    })?;
    if values.len() != 3 {
        return Err(BuildError(
            "manifest.maze.origin must contain three finite numbers".to_owned(),
        ));
    }
    let mut result = [0.0; 3];
    for (index, value) in values.iter().enumerate() {
        result[index] = value.as_f64().ok_or_else(|| {
            BuildError("manifest.maze.origin must contain three finite numbers".to_owned())
        })?;
        if !result[index].is_finite() {
            return Err(BuildError(
                "manifest.maze.origin must contain three finite numbers".to_owned(),
            ));
        }
    }
    Ok(result)
}

pub(super) fn material(config: &Map<String, Value>, key: &str, default: &str) -> Result<String> {
    let value = config.get(key).and_then(Value::as_str).unwrap_or(default);
    let value = value
        .strip_prefix("builtin:")
        .unwrap_or(value)
        .to_ascii_lowercase();
    if !TERRAIN_MATERIALS.contains(&value.as_str()) {
        return Err(BuildError(format!(
            "manifest.maze.terrain.{key} must be a built-in terrain material"
        )));
    }
    Ok(value)
}

pub(super) fn array_clone(world: &Map<String, Value>, key: &str) -> Result<Vec<Value>> {
    match world.get(key) {
        None => Ok(Vec::new()),
        Some(Value::Array(values)) => Ok(values.clone()),
        Some(_) => Err(BuildError(format!("world.{key} must be an array"))),
    }
}

pub(super) fn object_clone(world: &Map<String, Value>, key: &str) -> Result<Map<String, Value>> {
    match world.get(key) {
        None => Ok(Map::new()),
        Some(Value::Object(values)) => Ok(values.clone()),
        Some(_) => Err(BuildError(format!("world.{key} must be an object"))),
    }
}

pub(super) fn position(origin: [f64; 3], cell_size: f64, cell: (usize, usize), y: f64) -> Value {
    json!([
        origin[0] + (cell.0 as f64 + 0.5) * cell_size,
        origin[1] + y,
        origin[2] + (cell.1 as f64 + 0.5) * cell_size
    ])
}
