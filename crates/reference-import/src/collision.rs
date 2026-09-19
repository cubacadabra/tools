//! Validation and serialization for package-authored static collision.
//!
//! Keep these limits in sync with the Rust runtime's static collision loader.

use serde::Serialize;
use std::{fs, path::Path};

pub(crate) const FORMAT_VERSION: u32 = 1;
pub(crate) const MAX_TRIANGLES: usize = 200_000;
pub(crate) const MAX_COORDINATE: f32 = 4096.0;
pub(crate) const MAX_JSON_BYTES: usize = 64 * 1024 * 1024;
const MIN_AREA_SQUARED: f32 = 1.0e-12;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CollisionFile<'a> {
    format_version: u32,
    triangles: &'a [[[f32; 3]; 3]],
}

pub(crate) fn write_file(path: &Path, triangles: &[[[f32; 3]; 3]]) -> Result<(), String> {
    validate(triangles)?;
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
    fs::write(path, bytes)
        .map_err(|error| format!("could not write collision {}: {error}", path.display()))
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
}
