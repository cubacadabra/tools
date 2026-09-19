//! Build-time validation and inlining for authored static collision.
//!
//! The runtime consumes only inline world-space triangles. `source` is a
//! creator convenience and is deliberately removed before a package is built.

use serde_json::{Map, Value};
use std::{
    fs,
    path::{Component, Path, PathBuf},
};

pub(crate) const FORMAT_VERSION: u64 = 1;
pub(crate) const MAX_TRIANGLES: usize = 200_000;
pub(crate) const MAX_COORDINATE: f64 = 4096.0;
pub(crate) const MAX_JSON_BYTES: u64 = 64 * 1024 * 1024;
const MIN_AREA_SQUARED: f64 = 1.0e-12;

pub(crate) fn resolve_manifest_sources(
    manifest: &mut Map<String, Value>,
    project_root: &Path,
) -> super::Result<()> {
    if let Some(collision) = manifest.get("collision").cloned() {
        let resolved = resolve_definition(&collision, project_root, "manifest.collision")?;
        manifest.insert("collision".to_owned(), resolved);
    }

    let Some(worlds) = manifest.get_mut("worlds") else {
        return Ok(());
    };
    let worlds = worlds
        .as_object_mut()
        .ok_or_else(|| super::BuildError("manifest.worlds must be an object".to_owned()))?;
    for (world_id, definition) in worlds {
        let world = definition.as_object_mut().ok_or_else(|| {
            super::BuildError(format!("manifest.worlds.{world_id} must be an object"))
        })?;
        let Some(collision) = world.get("collision").cloned() else {
            continue;
        };
        let resolved = resolve_definition(
            &collision,
            project_root,
            &format!("manifest.worlds.{world_id}.collision"),
        )?;
        world.insert("collision".to_owned(), resolved);
    }
    Ok(())
}

fn resolve_definition(value: &Value, project_root: &Path, field: &str) -> super::Result<Value> {
    let object = value.as_object().ok_or_else(|| {
        super::BuildError(format!(
            "{field} must be an inline collision object or {{source}}"
        ))
    })?;
    if object.contains_key("source") {
        if object.len() != 1 {
            return Err(super::BuildError(format!(
                "{field}.source cannot be combined with inline collision fields"
            )));
        }
        let source = object
            .get("source")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                super::BuildError(format!("{field}.source must be a relative JSON path"))
            })?;
        let source_path = resolve_source_path(project_root, source, field)?;
        let source_value = read_source_json(&source_path, field)?;
        validate_inline(&source_value, field)?;
        return Ok(source_value);
    }

    validate_inline(value, field)?;
    Ok(value.clone())
}

fn resolve_source_path(project_root: &Path, source: &str, field: &str) -> super::Result<PathBuf> {
    let relative = Path::new(source);
    if source.is_empty()
        || source.contains('\\')
        || relative.is_absolute()
        || !source.ends_with(".json")
        || relative.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(super::BuildError(format!(
            "{field}.source must be a relative JSON path inside the game project"
        )));
    }
    let project_root = fs::canonicalize(project_root)
        .map_err(super::io_error("could not resolve collision project root"))?;
    let candidate = project_root.join(relative);
    let canonical = fs::canonicalize(&candidate).map_err(|error| {
        super::BuildError(format!("{field}.source could not be resolved: {error}"))
    })?;
    if !canonical.starts_with(&project_root) || !canonical.is_file() {
        return Err(super::BuildError(format!(
            "{field}.source must resolve to a file inside the game project"
        )));
    }
    Ok(canonical)
}

fn read_source_json(path: &Path, field: &str) -> super::Result<Value> {
    let metadata =
        fs::metadata(path).map_err(super::io_error("could not inspect collision source"))?;
    if metadata.len() > MAX_JSON_BYTES {
        return Err(super::BuildError(format!(
            "{field}.source exceeds the 64 MiB JSON limit"
        )));
    }
    let bytes = fs::read(path).map_err(super::io_error("could not read collision source"))?;
    if bytes.len() as u64 > MAX_JSON_BYTES {
        return Err(super::BuildError(format!(
            "{field}.source exceeds the 64 MiB JSON limit"
        )));
    }
    serde_json::from_slice(&bytes).map_err(|error| {
        super::BuildError(format!(
            "invalid JSON in collision source {}: {error}",
            path.display()
        ))
    })
}

fn validate_inline(value: &Value, field: &str) -> super::Result<()> {
    let object = value
        .as_object()
        .ok_or_else(|| super::BuildError(format!("{field} must be an inline collision object")))?;
    if object.len() != 2
        || !object.contains_key("formatVersion")
        || !object.contains_key("triangles")
    {
        return Err(super::BuildError(format!(
            "{field} must contain only formatVersion and triangles"
        )));
    }
    let version = object
        .get("formatVersion")
        .and_then(Value::as_u64)
        .ok_or_else(|| super::BuildError(format!("{field}.formatVersion must be an integer")))?;
    if version != FORMAT_VERSION {
        return Err(super::BuildError(format!(
            "unsupported {field}.formatVersion {version}; builder supports {FORMAT_VERSION}"
        )));
    }
    let triangles = object
        .get("triangles")
        .and_then(Value::as_array)
        .ok_or_else(|| super::BuildError(format!("{field}.triangles must be an array")))?;
    if triangles.len() > MAX_TRIANGLES {
        return Err(super::BuildError(format!(
            "{field}.triangles cannot contain more than {MAX_TRIANGLES} triangles"
        )));
    }
    for (index, triangle) in triangles.iter().enumerate() {
        validate_triangle(triangle, field, index)?;
    }
    let encoded = serde_json::to_vec(value)
        .map_err(|error| super::BuildError(format!("could not serialize {field}: {error}")))?;
    if encoded.len() as u64 > MAX_JSON_BYTES {
        return Err(super::BuildError(format!(
            "{field} exceeds the 64 MiB JSON limit"
        )));
    }
    Ok(())
}

fn validate_triangle(value: &Value, field: &str, index: usize) -> super::Result<()> {
    let triangle = value.as_array().ok_or_else(|| {
        super::BuildError(format!(
            "{field}.triangles[{index}] must contain three points"
        ))
    })?;
    if triangle.len() != 3 {
        return Err(super::BuildError(format!(
            "{field}.triangles[{index}] must contain three points"
        )));
    }
    let mut points = [[0.0; 3]; 3];
    for (point_index, point) in triangle.iter().enumerate() {
        let point = point.as_array().ok_or_else(|| {
            super::BuildError(format!(
                "{field}.triangles[{index}][{point_index}] must contain three coordinates"
            ))
        })?;
        if point.len() != 3 {
            return Err(super::BuildError(format!(
                "{field}.triangles[{index}][{point_index}] must contain three coordinates"
            )));
        }
        for (axis, coordinate) in point.iter().enumerate() {
            let coordinate = coordinate.as_f64().ok_or_else(|| {
                super::BuildError(format!(
                    "{field}.triangles[{index}] coordinates must be finite numbers"
                ))
            })?;
            if !coordinate.is_finite() || coordinate.abs() > MAX_COORDINATE {
                return Err(super::BuildError(format!(
                    "{field}.triangles[{index}] coordinates must be finite and inside supported world bounds"
                )));
            }
            points[point_index][axis] = coordinate;
        }
    }
    let ab = sub(points[1], points[0]);
    let ac = sub(points[2], points[0]);
    let cross = [
        ab[1] * ac[2] - ab[2] * ac[1],
        ab[2] * ac[0] - ab[0] * ac[2],
        ab[0] * ac[1] - ab[1] * ac[0],
    ];
    let area_squared = cross[0] * cross[0] + cross[1] * cross[1] + cross[2] * cross[2];
    if area_squared <= MIN_AREA_SQUARED {
        return Err(super::BuildError(format!(
            "{field}.triangles[{index}] must have non-zero area"
        )));
    }
    Ok(())
}

fn sub(left: [f64; 3], right: [f64; 3]) -> [f64; 3] {
    [left[0] - right[0], left[1] - right[1], left[2] - right[2]]
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[cfg(unix)]
    use std::os::unix::fs::symlink;

    fn triangle() -> Value {
        json!([[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]])
    }

    #[test]
    fn validates_versions_and_triangle_shape() {
        let root = tempfile::tempdir().unwrap();
        let mut manifest = Map::new();
        manifest.insert(
            "collision".to_owned(),
            json!({"formatVersion": 2, "triangles": [triangle()]}),
        );
        let error = resolve_manifest_sources(&mut manifest, root.path()).unwrap_err();
        assert!(
            error
                .0
                .contains("unsupported manifest.collision.formatVersion")
        );
        manifest.insert(
            "collision".to_owned(),
            json!({"formatVersion": 1, "triangles": [[[0,0,0],[1,0,0],[2,0,0]]]}),
        );
        let error = resolve_manifest_sources(&mut manifest, root.path()).unwrap_err();
        assert!(error.0.contains("non-zero area"));
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlink_escape() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let source = outside.path().join("collision.json");
        fs::write(
            &source,
            json!({"formatVersion": 1, "triangles": [triangle()]}).to_string(),
        )
        .unwrap();
        let link = root.path().join("collision.json");
        symlink(&source, &link).unwrap();
        let mut manifest = Map::new();
        manifest.insert("collision".to_owned(), json!({"source": "collision.json"}));
        let error = resolve_manifest_sources(&mut manifest, root.path()).unwrap_err();
        assert!(error.0.contains("inside the game project"));
    }

    #[test]
    fn inlines_root_and_world_sources_in_a_built_output() {
        let project = tempfile::tempdir().unwrap();
        fs::create_dir_all(project.path().join("src")).unwrap();
        fs::create_dir_all(project.path().join("assets/collision")).unwrap();
        fs::write(project.path().join("src/main.luau"), "return {}\n").unwrap();
        let source = json!({"formatVersion": 1, "triangles": [triangle()]});
        fs::write(
            project.path().join("assets/collision/hub.json"),
            source.to_string(),
        )
        .unwrap();
        fs::write(
            project.path().join("manifest.json"),
            json!({
                "id": "collision-game",
                "version": 1,
                "sdkVersion": "0.5.0",
                "collision": {"source": "assets/collision/hub.json"},
                "worlds": {"hub": {"collision": {"source": "assets/collision/hub.json"}}}
            })
            .to_string(),
        )
        .unwrap();
        let output = project.path().join("build/package");
        super::super::build_game(&super::super::BuildOptions {
            source_root: project.path().join("src"),
            manifest_path: project.path().join("manifest.json"),
            output: output.clone(),
            zip_path: None,
        })
        .unwrap();
        let built: Value =
            serde_json::from_str(&fs::read_to_string(output.join("manifest.json")).unwrap())
                .unwrap();
        assert_eq!(built["collision"], source);
        assert_eq!(built["worlds"]["hub"]["collision"], source);
        assert!(built["collision"]["source"].is_null());
    }

    #[test]
    fn sdk_05_fields_require_current_manifest_version() {
        let mut manifest = Map::new();
        manifest.insert(
            "collision".to_owned(),
            json!({"formatVersion": 1, "triangles": [triangle()]}),
        );
        let error = super::super::validate_sdk_features(&manifest, Some("0.4.0"))
            .expect_err("collision must not be silently accepted by SDK 0.4");
        assert!(error.0.contains("require manifest.sdkVersion 0.5.0"));
        super::super::validate_sdk_features(&manifest, Some("0.5.0"))
            .expect("SDK 0.5 should accept collision");

        let mut camera = Map::new();
        camera.insert(
            "world".to_owned(),
            json!({"camera": {"yaw": 0, "pitch": 0, "distance": 8}}),
        );
        let error = super::super::validate_sdk_features(&camera, Some("0.4.0"))
            .expect_err("camera must not be silently accepted by SDK 0.4");
        assert!(error.0.contains("require manifest.sdkVersion 0.5.0"));

        let mut terrain = Map::new();
        terrain.insert(
            "terrain".to_owned(),
            json!({"operations": [{"shape": "block"}]}),
        );
        super::super::validate_sdk_features(&terrain, Some("0.4.0"))
            .expect("SDK 0.4 should accept terrain");
        super::super::validate_sdk_features(&terrain, Some("0.5.0"))
            .expect("SDK 0.5 should accept terrain");
        let error = super::super::validate_sdk_features(&terrain, Some("0.3.0"))
            .expect_err("terrain must not be silently accepted by SDK 0.3");
        assert!(
            error
                .0
                .contains("require manifest.sdkVersion 0.4.0, 0.5.0, or 0.6.0")
        );
    }
}
