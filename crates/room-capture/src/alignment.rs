//! Reviewed similarity alignment. Never mutate recovered cameras or sparse evidence.
use crate::reconstruction::{SparsePoint, digest, finite, read_json, write_json};
use serde::{Deserialize, Serialize};
use std::{fs, path::Path};

pub const ALIGNMENT_FORMAT_VERSION: u32 = 1;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Alignment {
    pub format_version: u32,
    pub reconstruction_sha256: String,
    pub component_id: String,
    pub distance_point_ids: [u64; 2],
    pub distance_meters: f64,
    /// First point is origin; first-to-second defines +X. Order defines the +Y normal.
    pub floor_point_ids: [u64; 3],
    pub meters_per_unit: f64,
    /// world = metersPerUnit * rotation * reconstruction + translationMeters.
    /// Right-handed world axes, +Y up, meters. Row-major rotation.
    pub rotation: [[f64; 3]; 3],
    pub translation_meters: [f64; 3],
}

pub fn subtract(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    std::array::from_fn(|i| a[i] - b[i])
}
fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    (0..3).map(|i| a[i] * b[i]).sum()
}
fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn normalized(a: [f64; 3]) -> Result<[f64; 3], String> {
    let length = dot(a, a).sqrt();
    if length < 1e-9 || !length.is_finite() {
        return Err("Choose distinct, non-collinear alignment points.".into());
    }
    Ok(a.map(|v| v / length))
}

impl Alignment {
    pub fn transform_point(&self, p: [f64; 3]) -> [f64; 3] {
        std::array::from_fn(|i| {
            self.meters_per_unit * dot(self.rotation[i], p) + self.translation_meters[i]
        })
    }
    pub fn transform_direction(&self, v: [f64; 3]) -> [f64; 3] {
        self.rotation.map(|row| dot(row, v))
    }
}

pub fn create_alignment(
    manifest: &Path,
    component_id: &str,
    points: &[SparsePoint],
    distance_point_ids: [u64; 2],
    distance_meters: f64,
    floor_point_ids: [u64; 3],
) -> Result<Alignment, String> {
    finite(&[distance_meters])?;
    if distance_meters <= 0.0 {
        return Err("Enter a positive measured distance in meters.".into());
    }
    let find = |id| {
        points
            .iter()
            .find(|p| p.id == id)
            .map(|p| p.position)
            .ok_or("Alignment point is missing from this component.")
    };
    let a = find(distance_point_ids[0])?;
    let b = find(distance_point_ids[1])?;
    let delta = subtract(b, a);
    let length = dot(delta, delta).sqrt();
    if length < 1e-9 {
        return Err("Choose two different measured-distance endpoints.".into());
    }
    let meters_per_unit = distance_meters / length;
    let origin = find(floor_point_ids[0])?;
    let x = normalized(subtract(find(floor_point_ids[1])?, origin))?;
    let toward = normalized(subtract(find(floor_point_ids[2])?, origin))?;
    let normal = cross(x, toward);
    if dot(normal, normal) < 1e-6 {
        return Err("Floor points are nearly collinear; choose a wider triangle.".into());
    }
    let y = normalized(normal)?;
    let z = cross(x, y);
    let rotation = [x, y, z];
    let translation_meters = rotation.map(|row| -meters_per_unit * dot(row, origin));
    let reconstruction = crate::reconstruction::read_reconstruction(manifest)?;
    let component = reconstruction
        .components
        .iter()
        .find(|c| c.id == component_id)
        .ok_or("Unknown reconstruction component.")?;
    // Bind reviewed anchors to the authoritative shard bytes, not caller-provided geometry.
    let actual =
        crate::reconstruction::read_component_points(manifest.parent().unwrap(), component)?;
    for id in distance_point_ids.into_iter().chain(floor_point_ids) {
        if actual.iter().find(|p| p.id == id).map(|p| p.position) != Some(find(id)?) {
            return Err("Alignment evidence differs from the reconstruction.".into());
        }
    }
    let alignment = Alignment {
        format_version: ALIGNMENT_FORMAT_VERSION,
        reconstruction_sha256: digest(&fs::read(manifest).map_err(|e| e.to_string())?),
        component_id: component_id.into(),
        distance_point_ids,
        distance_meters,
        floor_point_ids,
        meters_per_unit,
        rotation,
        translation_meters,
    };
    validate(&alignment)?;
    Ok(alignment)
}

fn validate(a: &Alignment) -> Result<(), String> {
    finite(&[a.meters_per_unit, a.distance_meters])?;
    finite(&a.translation_meters)?;
    if a.meters_per_unit <= 0.0 || a.distance_meters <= 0.0 {
        return Err("Invalid metric alignment scale.".into());
    }
    for row in &a.rotation {
        finite(row)?;
    }
    for i in 0..3 {
        for j in 0..3 {
            if (dot(a.rotation[i], a.rotation[j]) - if i == j { 1.0 } else { 0.0 }).abs() > 1e-6 {
                return Err("Alignment rotation must be orthonormal.".into());
            }
        }
    }
    if (dot(cross(a.rotation[0], a.rotation[1]), a.rotation[2]) - 1.0).abs() > 1e-6 {
        return Err("Alignment must preserve right-handed axes.".into());
    }
    Ok(())
}

pub fn save_alignment(manifest: &Path, a: &Alignment) -> Result<(), String> {
    validate(a)?;
    if digest(&fs::read(manifest).map_err(|e| e.to_string())?) != a.reconstruction_sha256 {
        return Err("Alignment belongs to another reconstruction.".into());
    }
    let root = manifest
        .parent()
        .ok_or("Missing reconstruction directory.")?;
    // Component identity is supplied by the reconstruction, not used directly as a path.
    let result = crate::reconstruction::read_reconstruction(manifest)?;
    let index = result
        .components
        .iter()
        .position(|c| c.id == a.component_id)
        .ok_or("Unknown alignment component.")?;
    let points = crate::reconstruction::read_component_points(root, &result.components[index])?;
    let expected = create_alignment(
        manifest,
        &a.component_id,
        &points,
        a.distance_point_ids,
        a.distance_meters,
        a.floor_point_ids,
    )?;
    matching_transform(a, &expected)?;
    let temp = tempfile::NamedTempFile::new_in(root).map_err(|e| e.to_string())?;
    write_json(temp.path(), a)?;
    temp.persist(root.join(format!("alignment-{index:03}.json")))
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn read_alignment(manifest: &Path, index: usize) -> Result<Option<Alignment>, String> {
    let result = crate::reconstruction::read_reconstruction(manifest)?;
    let component = result
        .components
        .get(index)
        .ok_or("Unknown alignment component.")?;
    let path = manifest
        .parent()
        .unwrap()
        .join(format!("alignment-{index:03}.json"));
    if !path.exists() {
        return Ok(None);
    }
    let a: Alignment = read_json(&path, Some(ALIGNMENT_FORMAT_VERSION))?;
    validate(&a)?;
    if a.component_id != component.id
        || a.reconstruction_sha256 != digest(&fs::read(manifest).map_err(|e| e.to_string())?)
    {
        return Err("Alignment does not match this reconstruction/component.".into());
    }
    let points =
        crate::reconstruction::read_component_points(manifest.parent().unwrap(), component)?;
    let expected = create_alignment(
        manifest,
        &a.component_id,
        &points,
        a.distance_point_ids,
        a.distance_meters,
        a.floor_point_ids,
    )?;
    matching_transform(&a, &expected)?;
    Ok(Some(a))
}

fn matching_transform(a: &Alignment, expected: &Alignment) -> Result<(), String> {
    for (actual, expected) in a
        .rotation
        .iter()
        .flatten()
        .chain(a.translation_meters.iter())
        .chain(std::iter::once(&a.meters_per_unit))
        .zip(
            expected
                .rotation
                .iter()
                .flatten()
                .chain(expected.translation_meters.iter())
                .chain(std::iter::once(&expected.meters_per_unit)),
        )
    {
        if (actual - expected).abs() > 1e-8 * (1.0 + expected.abs()) {
            return Err("Alignment transform does not match its reviewed anchors.".into());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_reflection_and_non_orthonormal_transforms() {
        let mut a = Alignment {
            format_version: 1,
            reconstruction_sha256: String::new(),
            component_id: String::new(),
            distance_point_ids: [1, 2],
            distance_meters: 1.0,
            floor_point_ids: [1, 2, 3],
            meters_per_unit: 1.0,
            rotation: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, -1.0]],
            translation_meters: [0.0; 3],
        };
        assert!(validate(&a).is_err());
        a.rotation[2][2] = 1.0;
        assert!(validate(&a).is_ok());
        assert_eq!(a.transform_point([2.0, 3.0, 4.0]), [2.0, 3.0, 4.0]);
        a.rotation[0][1] = 0.1;
        assert!(validate(&a).is_err());
    }
}
