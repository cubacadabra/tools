//! Narrow Roblox XML importer for visual-reference reconstruction.
//!
//! This crate intentionally emits a tool-owned intermediate scene instead of a
//! Cubacadabra package manifest. It preserves source facts first; package and
//! renderer adaptation can then be measured against that stable artifact.

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

mod collision;
mod importer;
mod mesh_export;
mod mesh_overrides;
mod model;

pub use importer::{import_reference, read_reference_scene};
#[cfg(test)]
pub(crate) use mesh_export::{
    StaticMeshGroup, StaticMeshVertex, append_static_geometry, write_static_glb,
};
pub use mesh_export::{export_reference_mesh, roblox_material_runtime_name};
pub use model::*;

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
