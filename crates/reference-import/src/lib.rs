//! Narrow Roblox XML importer for visual-reference reconstruction.
//!
//! This crate intentionally emits a tool-owned intermediate scene instead of a
//! Cubacadabra package manifest. It preserves source facts first; package and
//! renderer adaptation can then be measured against that stable artifact.

use glam::{EulerRot, Mat3, Quat, Vec3};
use rbx_dom_weak::types::{CFrame, Color3, ContentType, Matrix3, Variant, Vector3};
use rbx_dom_weak::{Instance, WeakDom, types::Ref, ustr};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, HashMap},
    fs::{self, File},
    io::{BufReader, Write},
    path::{Path, PathBuf},
};

mod authoring;
mod collision;
mod importer;
mod mesh_export;
mod mesh_overrides;
mod model;

pub use authoring::{
    RobloxAuthoringImport, RobloxExportReport, import_roblox_authoring_scene, roblox_source_file,
    roblox_source_files, write_roblox_place, write_roblox_place_with_manifest,
};
pub use importer::{import_reference, load_reference, read_reference_scene};
#[cfg(test)]
pub(crate) use mesh_export::{
    StaticMeshGroup, StaticMeshVertex, append_static_geometry, write_static_glb,
};
pub use mesh_export::{export_reference_mesh, roblox_material_runtime_name};
pub use model::*;

/// Validate the Roblox Part subset that can receive a native static visual
/// representation. Source physics remains in the preserved Roblox place.
pub fn validate_roblox_native_part(geometry: &GeometryInstance) -> Result<(), &'static str> {
    if geometry.class != "Part" {
        return Err("unsupported-class");
    }
    if geometry.shape.is_some_and(|shape| !matches!(shape, 0 | 1)) {
        return Err("unsupported-shape");
    }
    if geometry.shape == Some(0)
        && geometry
            .size
            .iter()
            .any(|size| (*size - geometry.size[0]).abs() > 0.0001)
    {
        return Err("unsupported-nonuniform-sphere");
    }
    if geometry.mesh.is_some() {
        return Err("unsupported-mesh");
    }
    if !roblox_source_rotation_is_axis_aligned(geometry.transform.rotation) {
        return Err("unsupported-transform");
    }
    if geometry.transparency > 0.0001 {
        return Err("unsupported-transparency");
    }
    if geometry.reflectance > 0.0001 {
        return Err("unsupported-reflectance");
    }
    if geometry.material.name.as_deref().is_some_and(|name| {
        !matches!(
            name.to_ascii_lowercase().as_str(),
            "plastic" | "smoothplastic"
        ) && roblox_material_runtime_name(name).is_none()
    }) {
        return Err("unsupported-material");
    }
    if geometry
        .size
        .iter()
        .any(|value| !value.is_finite() || *value < 0.05)
    {
        return Err("unsupported-size");
    }
    Ok(())
}

pub fn is_roblox_workspace_path(scene: &ReferenceScene, path: &str) -> bool {
    scene
        .instances
        .iter()
        .filter(|instance| instance.class == "Workspace")
        .any(|workspace| {
            path == workspace.path || path.starts_with(&format!("{}/", workspace.path))
        })
}

pub fn roblox_source_rotation_is_axis_aligned(rotation: [[f32; 3]; 3]) -> bool {
    let tolerance = 0.0001;
    let rows_are_axes = rotation.iter().all(|row| {
        row.iter().filter(|value| value.abs() > tolerance).count() == 1
            && row
                .iter()
                .all(|value| value.abs() <= tolerance || (value.abs() - 1.0).abs() <= tolerance)
    });
    let columns_are_axes = (0..3).all(|column| {
        rotation
            .iter()
            .filter(|row| row[column].abs() > tolerance)
            .count()
            == 1
    });
    let determinant = rotation[0][0]
        * (rotation[1][1] * rotation[2][2] - rotation[1][2] * rotation[2][1])
        - rotation[0][1] * (rotation[1][0] * rotation[2][2] - rotation[1][2] * rotation[2][0])
        + rotation[0][2] * (rotation[1][0] * rotation[2][1] - rotation[1][1] * rotation[2][0]);
    rows_are_axes && columns_are_axes && (determinant - 1.0).abs() <= tolerance
}

pub fn roblox_source_rotation_to_euler(rotation: [[f32; 3]; 3]) -> [f32; 3] {
    let rotation = canonical_axis_rotation(rotation).unwrap_or(rotation);
    let matrix = Mat3::from_cols(
        Vec3::new(rotation[0][0], rotation[1][0], rotation[2][0]),
        Vec3::new(rotation[0][1], rotation[1][1], rotation[2][1]),
        Vec3::new(rotation[0][2], rotation[1][2], rotation[2][2]),
    );
    let (x, y, z) = Quat::from_mat3(&matrix).to_euler(EulerRot::XYZ);
    [x, y, z]
}

fn canonical_axis_rotation(rotation: [[f32; 3]; 3]) -> Option<[[f32; 3]; 3]> {
    if !roblox_source_rotation_is_axis_aligned(rotation) {
        return None;
    }
    let mut canonical = [[0.0; 3]; 3];
    for (row_index, row) in rotation.iter().enumerate() {
        let column_index = row
            .iter()
            .enumerate()
            .max_by(|left, right| left.1.abs().total_cmp(&right.1.abs()))
            .map(|(index, _)| index)?;
        canonical[row_index][column_index] = row[column_index].signum();
    }
    Some(canonical)
}

/// Infer the stable local frame for a source model from its first descendant
/// geometry. Roblox models in the normalized reference often have no CFrame of
/// their own; using a descendant frame keeps repeated model instances
/// reusable while preserving their world placement and facing direction.
pub fn reference_instance_frame(
    scene: &ReferenceScene,
    instance_root: &str,
) -> Result<ReferenceInstanceFrame, String> {
    let geometry = scene
        .geometry
        .iter()
        .filter(|geometry| geometry.path.starts_with(&format!("{instance_root}/")))
        .min_by(|left, right| left.path.cmp(&right.path))
        .ok_or_else(|| {
            format!(
                "source instance {instance_root:?} has no descendant geometry to define a local frame"
            )
        })?;
    Ok(ReferenceInstanceFrame {
        position: geometry.transform.position,
        rotation: geometry.transform.rotation,
    })
}

/// Return a stable, quantized fingerprint for the normalized geometry of one
/// source instance. Small floating-point differences from Roblox exports are
/// ignored so repeated models can share one GLB while material/color changes
/// remain separate assets.
pub fn reference_instance_fingerprint(
    scene: &ReferenceScene,
    instance_root: &str,
) -> Result<String, String> {
    let mut records = scene
        .geometry
        .iter()
        .filter(|geometry| geometry.path.starts_with(&format!("{instance_root}/")))
        .map(|geometry| {
            json!({
                "path": geometry.path.strip_prefix(instance_root).unwrap_or(&geometry.path),
                "class": geometry.class,
                "name": geometry.name,
                "size": geometry.size.map(quantize),
                "color": geometry.color.map(quantize),
                "material": geometry.material,
                "transparency": quantize(geometry.transparency),
                "reflectance": quantize(geometry.reflectance),
                "canCollide": geometry.can_collide,
                "castShadow": geometry.cast_shadow,
                "shape": geometry.shape,
                "mesh": geometry.mesh,
            })
        })
        .collect::<Vec<_>>();
    if records.is_empty() {
        return Err(format!("source instance {instance_root:?} has no geometry"));
    }
    records.sort_by_key(|record| record["path"].as_str().unwrap_or_default().to_owned());
    let bytes = serde_json::to_vec(&records)
        .map_err(|error| format!("could not fingerprint source instance: {error}"))?;
    Ok(format!("{:x}", Sha256::digest(bytes))[..16].to_owned())
}

fn quantize(value: f32) -> f32 {
    (value * 1000.0).round() / 1000.0
}

#[cfg(test)]
mod tests;
