use super::*;

#[derive(Clone, Debug)]
pub(super) struct EditableInstanceSpec {
    pub(super) source_name: String,
    pub(super) mesh: String,
}

#[derive(Clone, Debug)]
pub(super) struct PromotedInstance {
    pub(super) source_path: String,
    pub(super) mesh: String,
    pub(super) collision_asset: Option<String>,
    pub(super) position: [f32; 3],
    pub(super) rotation: [f32; 3],
    pub(super) scale: [f32; 3],
}

#[derive(Clone, Debug)]
pub(super) struct PromotedPrimitive {
    pub(super) source_path: String,
    pub(super) position: [f32; 3],
    pub(super) rotation: [f32; 3],
    pub(super) size: [f32; 3],
    pub(super) material: String,
    pub(super) runtime_material: Option<&'static str>,
    pub(super) can_collide: bool,
    pub(super) cast_shadow: bool,
}

#[derive(Clone, Debug, Default)]
pub(super) struct PromotionStats {
    pub(super) source_parts: usize,
    pub(super) workspace_parts: usize,
    pub(super) promoted_parts: usize,
    pub(super) fallback_parts: usize,
    pub(super) fallback_reasons: BTreeMap<String, usize>,
}

#[derive(Clone, Debug, Default)]
pub(super) struct PromotionSelection {
    pub(super) promoted: Vec<PromotedPrimitive>,
    pub(super) statuses: BTreeMap<String, PromotionStatus>,
    pub(super) stats: PromotionStats,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum PromotionStatus {
    Promoted { node_id: String },
    Fallback { reason: &'static str },
}

pub(super) fn select_promoted_primitives(
    reference: &ReferenceScene,
    names: &[String],
    path_prefix: Option<&str>,
) -> PromotionSelection {
    let mut selection = PromotionSelection::default();
    for geometry in &reference.geometry {
        if geometry.class != "Part" {
            continue;
        }
        selection.stats.source_parts += 1;
        if !cubacadabra_reference_import::is_roblox_workspace_path(reference, &geometry.path) {
            continue;
        }
        selection.stats.workspace_parts += 1;
        if !names.is_empty() && !names.iter().any(|name| name == &geometry.name) {
            record_fallback(&mut selection, geometry, "name-filtered");
            continue;
        }
        if path_prefix.is_some_and(|prefix| !geometry.path.starts_with(prefix)) {
            record_fallback(&mut selection, geometry, "path-filtered");
            continue;
        }
        let Some(reason) = can_promote_part(geometry).err() else {
            let node_id = primitive_node_id(&geometry.path);
            selection.promoted.push(PromotedPrimitive {
                source_path: geometry.path.clone(),
                position: geometry.transform.position,
                rotation: source_rotation_to_euler(geometry.transform.rotation),
                size: geometry.size,
                material: source_color(geometry.color),
                runtime_material: geometry
                    .material
                    .name
                    .as_deref()
                    .and_then(cubacadabra_reference_import::roblox_material_runtime_name),
                can_collide: geometry.can_collide,
                cast_shadow: geometry.cast_shadow,
            });
            selection
                .statuses
                .insert(geometry.path.clone(), PromotionStatus::Promoted { node_id });
            selection.stats.promoted_parts += 1;
            continue;
        };
        record_fallback(&mut selection, geometry, reason);
    }
    selection
}

pub(super) fn record_fallback(
    selection: &mut PromotionSelection,
    geometry: &GeometryInstance,
    reason: &'static str,
) {
    selection.stats.fallback_parts += 1;
    *selection
        .stats
        .fallback_reasons
        .entry(reason.to_owned())
        .or_default() += 1;
    selection
        .statuses
        .insert(geometry.path.clone(), PromotionStatus::Fallback { reason });
}

pub(super) fn can_promote_part(geometry: &GeometryInstance) -> Result<(), &'static str> {
    cubacadabra_reference_import::validate_roblox_native_part(geometry)
}

pub(super) fn primitive_node_id(source_path: &str) -> String {
    format!("imported-part-{}", short_hash(source_path))
}

pub(super) fn write_promotion_report(path: &Path, stats: &PromotionStats) -> Result<(), String> {
    let report = json!({
        "formatVersion": 1,
        "kind": "roblox-native-promotion-report",
        "parts": {
            "total": stats.workspace_parts,
            "sourceTotal": stats.source_parts,
            "workspace": {
                "total": stats.workspace_parts,
                "promoted": stats.promoted_parts,
                "fallback": stats.fallback_parts,
                "fallbackReasons": stats.fallback_reasons,
            },
            "promoted": stats.promoted_parts,
            "fallback": stats.fallback_parts,
            "fallbackReasons": stats.fallback_reasons,
        }
    });
    write_text(
        path,
        &format!(
            "{}\n",
            serde_json::to_string_pretty(&report)
                .map_err(|error| format!("could not encode promotion report: {error}"))?
        ),
    )
}

pub(super) fn read_editable_instance_map(path: &Path) -> Result<Vec<PromotedInstance>, String> {
    let source = fs::read_to_string(path).map_err(|error| {
        format!(
            "could not read editable instance map {}: {error}",
            path.display()
        )
    })?;
    let value: Value = serde_json::from_str(&source)
        .map_err(|error| format!("editable instance map is not valid JSON: {error}"))?;
    let collision_assets = value
        .get("assets")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_object)
        .filter(|asset| asset.get("collision").and_then(Value::as_str).is_some())
        .filter_map(|asset| asset.get("id").and_then(Value::as_str))
        .collect::<BTreeSet<_>>();
    let entries = value
        .get("instances")
        .and_then(Value::as_array)
        .ok_or_else(|| "editable instance map requires an instances array".to_owned())?;
    entries
        .iter()
        .map(|entry| {
            let object = entry
                .as_object()
                .ok_or_else(|| "editable instance map entries must be objects".to_owned())?;
            let mesh = object
                .get("asset")
                .and_then(Value::as_str)
                .ok_or_else(|| "editable instance map entry requires asset".to_owned())?
                .to_owned();
            Ok(PromotedInstance {
                source_path: object
                    .get("sourcePath")
                    .and_then(Value::as_str)
                    .ok_or_else(|| "editable instance map entry requires sourcePath".to_owned())?
                    .to_owned(),
                collision_asset: collision_assets
                    .contains(mesh.as_str())
                    .then(|| mesh.clone()),
                mesh,
                position: vector3(object.get("position"), "position")?,
                rotation: vector3(object.get("rotation"), "rotation")?,
                scale: vector3(object.get("scale"), "scale")?,
            })
        })
        .collect()
}

pub(super) fn vector3(value: Option<&Value>, label: &str) -> Result<[f32; 3], String> {
    let values = value
        .and_then(Value::as_array)
        .ok_or_else(|| format!("editable instance map entry requires {label}"))?;
    let values = values
        .iter()
        .map(|value| {
            value
                .as_f64()
                .filter(|value| value.is_finite())
                .map(|value| value as f32)
                .ok_or_else(|| format!("editable instance map {label} must contain finite numbers"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    values
        .try_into()
        .map_err(|_| format!("editable instance map {label} must contain three numbers"))
}
