//! Creator-supplied dimensions, independent of inferred point/camera coordinates.
use crate::reconstruction::{finite, read_json, read_reconstruction, write_json};
use serde::{Deserialize, Serialize};
use std::path::Path;

pub const MEASUREMENTS_FORMAT_VERSION: u32 = 1;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MeasuredObject {
    pub id: String,
    pub label: String,
    pub length_meters: f64,
    pub depth_meters: f64,
    pub height_meters: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CaptureMeasurements {
    pub format_version: u32,
    pub capture_sha256: String,
    pub objects: Vec<MeasuredObject>,
}

fn validate(object: &MeasuredObject) -> Result<(), String> {
    if object.id.is_empty()
        || object.id.len() > 128
        || !object
            .id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        || object.label.trim().is_empty()
        || object.label.len() > 200
        || object.label.chars().any(char::is_control)
    {
        return Err("Measurement needs a short object ID and readable label.".into());
    }
    finite(&[
        object.length_meters,
        object.depth_meters,
        object.height_meters,
    ])?;
    if [
        object.length_meters,
        object.depth_meters,
        object.height_meters,
    ]
    .iter()
    .any(|v| *v <= 0.0 || *v > 10000.0)
    {
        return Err("Measured dimensions must be positive meters, at most 10,000 each.".into());
    }
    Ok(())
}

pub fn read_measurements(manifest: &Path) -> Result<Option<CaptureMeasurements>, String> {
    let reconstruction = read_reconstruction(manifest)?;
    let root = manifest.parent().ok_or("Missing reconstruction folder.")?;
    let path = root.join("measurements.json");
    if !path.exists() {
        return Ok(None);
    }
    let measurements: CaptureMeasurements = read_json(&path, Some(MEASUREMENTS_FORMAT_VERSION))?;
    if measurements.capture_sha256 != reconstruction.capture_sha256
        || measurements.objects.len() > 300
    {
        return Err("Measurements do not match this capture or exceed the object limit.".into());
    }
    let mut ids = std::collections::BTreeSet::new();
    for object in &measurements.objects {
        validate(object)?;
        if !ids.insert(&object.id) {
            return Err("Duplicate measured object ID.".into());
        }
    }
    Ok(Some(measurements))
}

/// Atomically update one measured object, preserving unrelated creator measurements.
/// This records dimensions; it does not approve inferred anchors or establish metric scale.
pub fn record_measurement(
    manifest: &Path,
    object: MeasuredObject,
) -> Result<CaptureMeasurements, String> {
    validate(&object)?;
    let reconstruction = read_reconstruction(manifest)?;
    let root = manifest
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut measurements = read_measurements(manifest)?.unwrap_or(CaptureMeasurements {
        format_version: MEASUREMENTS_FORMAT_VERSION,
        capture_sha256: reconstruction.capture_sha256,
        objects: Vec::new(),
    });
    measurements.objects.retain(|o| o.id != object.id);
    measurements.objects.push(object);
    if measurements.objects.len() > 300 {
        return Err("Measurements exceed the object limit.".into());
    }
    measurements.objects.sort_by(|a, b| a.id.cmp(&b.id));
    let temp = tempfile::NamedTempFile::new_in(root).map_err(|e| e.to_string())?;
    write_json(temp.path(), &measurements)?;
    temp.persist(root.join("measurements.json"))
        .map_err(|e| e.to_string())?;
    Ok(measurements)
}
