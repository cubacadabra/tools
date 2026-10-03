//! Validation and serialization for package-authored static collision.
//!
//! Keep these limits in sync with the Rust runtime's static collision loader.

use serde::{Deserialize, Serialize};
use std::{fs, path::Path};

pub(crate) const FORMAT_VERSION: u32 = 1;
pub(crate) const MAX_TRIANGLES: usize = 200_000;
pub(crate) const MAX_COORDINATE: f32 = 4096.0;
pub(crate) const MAX_JSON_BYTES: usize = 64 * 1024 * 1024;
const MAX_SOURCE_JSON_BYTES: usize = 4_000_000;
const MIN_AREA_SQUARED: f32 = 1.0e-12;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CollisionFile<'a, T> {
    format_version: u32,
    triangles: &'a [T],
}

pub(crate) fn write_file(path: &Path, triangles: &[[[f32; 3]; 3]]) -> Result<(), String> {
    validate(triangles)?;
    write_with_limit(path, triangles, MAX_SOURCE_JSON_BYTES)
}

fn write_with_limit<T: Serialize>(
    path: &Path,
    triangles: &[T],
    shard_bytes: usize,
) -> Result<(), String> {
    let bytes = serde_json::to_vec(&CollisionFile {
        format_version: FORMAT_VERSION,
        triangles,
    })
    .map_err(|error| format!("could not encode collision {}: {error}", path.display()))?;
    if bytes.len() > MAX_JSON_BYTES {
        return Err(format!(
            "collision JSON exceeds the {} MiB limit",
            MAX_JSON_BYTES / (1024 * 1024)
        ));
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            format!(
                "could not create collision output directory {}: {error}",
                parent.display()
            )
        })?;
    }
    if bytes.len() > shard_bytes {
        let stem = path
            .file_stem()
            .and_then(|name| name.to_str())
            .ok_or_else(|| "collision output needs a UTF-8 filename".to_owned())?;
        let shard_directory = path.with_file_name(format!("{stem}.parts"));
        fs::create_dir_all(&shard_directory)
            .map_err(|error| format!("could not create collision shards: {error}"))?;
        let prefix = b"{\"formatVersion\":1,\"triangles\":[";
        let mut shards = Vec::new();
        let mut current = prefix.to_vec();
        let mut count = 0;
        for triangle in triangles {
            let encoded = serde_json::to_vec(triangle)
                .map_err(|error| format!("could not encode collision triangle: {error}"))?;
            if prefix.len() + encoded.len() + 2 > shard_bytes {
                return Err("collision shard limit cannot fit one triangle".to_owned());
            }
            if current.len() + usize::from(count > 0) + encoded.len() + 2 > shard_bytes {
                current.extend_from_slice(b"]}");
                shards.push(current);
                current = prefix.to_vec();
                count = 0;
            }
            if count > 0 {
                current.push(b',');
            }
            current.extend_from_slice(&encoded);
            count += 1;
        }
        current.extend_from_slice(b"]}");
        shards.push(current);
        let mut sources = Vec::new();
        for (index, bytes) in shards.into_iter().enumerate() {
            let name = format!("part-{index:04}.json");
            fs::write(shard_directory.join(&name), bytes)
                .map_err(|error| format!("could not write collision shard: {error}"))?;
            sources.push(format!("{stem}.parts/{name}"));
        }
        let index = serde_json::to_vec_pretty(&serde_json::json!({
            "formatVersion": FORMAT_VERSION, "sources": sources,
        }))
        .map_err(|error| format!("could not encode collision source index: {error}"))?;
        return fs::write(path, index).map_err(|error| {
            format!(
                "could not write collision index {}: {error}",
                path.display()
            )
        });
    }
    fs::write(path, bytes)
        .map_err(|error| format!("could not write collision {}: {error}", path.display()))
}

/// Migrate an existing inline source export without changing numeric values.
/// This is source tooling; runtime packages continue to contain inline triangles.
pub fn shard_collision_source(input: &Path, output: &Path) -> Result<usize, String> {
    if fs::metadata(input)
        .map_err(|error| error.to_string())?
        .len()
        > MAX_JSON_BYTES as u64
    {
        return Err("collision input exceeds the 64 MiB JSON limit".to_owned());
    }
    let bytes =
        fs::read(input).map_err(|error| format!("could not read collision source: {error}"))?;
    if bytes.len() > MAX_JSON_BYTES {
        return Err("collision input exceeds the 64 MiB JSON limit".to_owned());
    }
    let value: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
    write_collision_source(output, &value)
}

/// Write a validated inline definition as bounded authoring JSON.
pub fn write_collision_source(output: &Path, value: &serde_json::Value) -> Result<usize, String> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Input {
        format_version: u32,
        triangles: Vec<[[f32; 3]; 3]>,
    }
    let parsed: Input = serde_json::from_value(value.clone())
        .map_err(|error| format!("expected inline collision source: {error}"))?;
    if parsed.format_version != FORMAT_VERSION {
        return Err(format!(
            "unsupported collision format {}",
            parsed.format_version
        ));
    }
    validate(&parsed.triangles)?;
    let triangles = value["triangles"]
        .as_array()
        .expect("validated triangle array");
    write_with_limit(output, triangles, MAX_SOURCE_JSON_BYTES)?;
    Ok(triangles.len())
}

pub(crate) fn validate(triangles: &[[[f32; 3]; 3]]) -> Result<(), String> {
    if triangles.len() > MAX_TRIANGLES {
        return Err(format!(
            "collision cannot contain more than {MAX_TRIANGLES} triangles"
        ));
    }
    for triangle in triangles {
        if triangle
            .iter()
            .flatten()
            .any(|value| !value.is_finite() || value.abs() > MAX_COORDINATE)
        {
            return Err(
                "collision triangle coordinates must be finite and inside supported world bounds"
                    .to_owned(),
            );
        }
        let [a, b, c] = triangle;
        let ab = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let ac = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
        let cross = [
            ab[1] * ac[2] - ab[2] * ac[1],
            ab[2] * ac[0] - ab[0] * ac[2],
            ab[0] * ac[1] - ab[1] * ac[0],
        ];
        let area_squared = cross[0] * cross[0] + cross[1] * cross[1] + cross[2] * cross[2];
        if area_squared <= MIN_AREA_SQUARED {
            return Err("collision triangles must have non-zero area".to_owned());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn triangle() -> [[f32; 3]; 3] {
        [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]]
    }

    #[test]
    fn rejects_invalid_runtime_geometry() {
        assert!(validate(&[[[0.0; 3]; 3]]).is_err());
        assert!(validate(&[[[4097.0, 0.0, 0.0], [0.0, 0.0, 0.0], [0.0, 0.0, 1.0]]]).is_err());
        assert!(validate(&[[[f32::NAN, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]]]).is_err());
    }

    #[test]
    fn writes_versioned_collision_json() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("assets/collision/hub.json");
        write_file(&path, &[triangle()]).unwrap();
        let value: serde_json::Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
        assert_eq!(value["formatVersion"], FORMAT_VERSION);
        assert_eq!(value["triangles"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn shards_large_source_without_losing_or_reordering_triangles() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("hub.json");
        let triangles = [triangle(); 17];
        write_with_limit(&path, &triangles, 160).unwrap();
        let index: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        let mut merged = Vec::new();
        for source in index["sources"].as_array().unwrap() {
            let bytes = fs::read(temp.path().join(source.as_str().unwrap())).unwrap();
            assert!(bytes.len() <= 160);
            let shard: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            merged.extend(shard["triangles"].as_array().unwrap().iter().cloned());
        }
        assert_eq!(serde_json::json!(merged), serde_json::json!(triangles));
        let first = fs::read(&path).unwrap();
        write_with_limit(&path, &triangles, 160).unwrap();
        assert_eq!(first, fs::read(path).unwrap());
    }
}
