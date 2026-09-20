use super::*;
use glam::{EulerRot, Mat3, Quat, Vec3};

const WORKSPACE_ROOT: &str = "Workspace:Workspace[1]";

pub(crate) fn import_roblox_scene_command(args: &[String]) -> Result<(), String> {
    let mut reference = None;
    let mut base_scene = None;
    let mut output = None;
    let mut source_index = None;
    let mut parent_id = None;
    let mut tree_depth = 4usize;
    let mut focus_paths = Vec::new();
    let mut reset_generated_source_tree = false;
    let mut editable_instances = Vec::new();
    let mut editable_part_names = Vec::new();
    let mut editable_instance_map = None;
    let mut editable_path_prefix = None;
    let mut editable_part_path_prefix = None;
    let mut editable_parent_id = None;
    let mut editable_id_prefix = "imported-object".to_owned();
    let mut editable_display_prefix = None;
    let mut promotion_report_output = None;
    let mut compact_output = false;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--reference" => {
                index += 1;
                reference = Some(PathBuf::from(required_arg(args, index, "--reference")?));
            }
            "--base-scene" => {
                index += 1;
                base_scene = Some(PathBuf::from(required_arg(args, index, "--base-scene")?));
            }
            "--output" => {
                index += 1;
                output = Some(PathBuf::from(required_arg(args, index, "--output")?));
            }
            "--source-index" => {
                index += 1;
                source_index = Some(PathBuf::from(required_arg(args, index, "--source-index")?));
            }
            "--parent-id" => {
                index += 1;
                parent_id = Some(required_arg(args, index, "--parent-id")?.to_owned());
            }
            "--tree-depth" => {
                index += 1;
                tree_depth = required_arg(args, index, "--tree-depth")?
                    .parse::<usize>()
                    .map_err(|_| "--tree-depth must be a non-negative integer".to_owned())?;
            }
            "--focus-path" => {
                index += 1;
                focus_paths.push(required_arg(args, index, "--focus-path")?.to_owned());
            }
            "--reset-generated-source-tree" => {
                reset_generated_source_tree = true;
            }
            "--editable-instance" => {
                index += 1;
                let value = required_arg(args, index, "--editable-instance")?;
                let (name, mesh) = value.split_once('=').ok_or_else(|| {
                    "--editable-instance must be formatted as source-name=mesh-asset".to_owned()
                })?;
                if name.is_empty() || mesh.is_empty() {
                    return Err(
                        "--editable-instance must include a source name and mesh asset".to_owned(),
                    );
                }
                editable_instances.push(EditableInstanceSpec {
                    source_name: name.to_owned(),
                    mesh: mesh.to_owned(),
                });
            }
            "--editable-part-name" => {
                index += 1;
                let name = required_arg(args, index, "--editable-part-name")?;
                if name.is_empty() {
                    return Err("--editable-part-name requires a non-empty name".to_owned());
                }
                editable_part_names.push(name.to_owned());
            }
            "--editable-instance-map" => {
                index += 1;
                editable_instance_map = Some(PathBuf::from(required_arg(
                    args,
                    index,
                    "--editable-instance-map",
                )?));
            }
            "--editable-path-prefix" => {
                index += 1;
                editable_path_prefix =
                    Some(required_arg(args, index, "--editable-path-prefix")?.to_owned());
            }
            "--editable-part-path-prefix" => {
                index += 1;
                editable_part_path_prefix =
                    Some(required_arg(args, index, "--editable-part-path-prefix")?.to_owned());
            }
            "--editable-parent-id" => {
                index += 1;
                editable_parent_id =
                    Some(required_arg(args, index, "--editable-parent-id")?.to_owned());
            }
            "--editable-id-prefix" => {
                index += 1;
                editable_id_prefix = required_arg(args, index, "--editable-id-prefix")?.to_owned();
            }
            "--editable-display-prefix" => {
                index += 1;
                editable_display_prefix =
                    Some(required_arg(args, index, "--editable-display-prefix")?.to_owned());
            }
            "--promotion-report-output" => {
                index += 1;
                promotion_report_output = Some(PathBuf::from(required_arg(
                    args,
                    index,
                    "--promotion-report-output",
                )?));
            }
            "--compact-output" => {
                compact_output = true;
            }
            value => return Err(format!("unknown import-roblox-scene option {value}")),
        }
        index += 1;
    }
    let reference_path =
        reference.ok_or_else(|| "import-roblox-scene requires --reference".to_owned())?;
    let reference_scene = read_reference_scene(&reference_path)?;
    if reference_scene.instances.is_empty() {
        return Err(
            "reference scene has no normalized source hierarchy; rerun import-roblox-reference from the original Roblox XML"
                .to_owned(),
        );
    }
    let output = output.unwrap_or_else(|| {
        base_scene
            .clone()
            .unwrap_or_else(|| PathBuf::from("scene.json"))
    });
    let source_index = source_index.unwrap_or_else(|| {
        output
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join("imports")
            .join("roblox")
            .join(source_dataset_name(&reference_scene.source.place.name))
            .join("index.json")
    });
    let source_index_reference = source_index_reference(&output, &source_index);
    let mut scene = if let Some(path) = base_scene {
        let source = fs::read_to_string(&path)
            .map_err(|error| format!("could not read base scene {}: {error}", path.display()))?;
        parse_authoring_scene(&source)?
    } else {
        AuthoringScene {
            format_version: 1,
            world_id: None,
            nodes: Vec::new(),
        }
    };
    let parent_id = parent_id.or_else(|| {
        scene
            .nodes
            .iter()
            .find(|node| node.id == "imported-environment")
            .map(|node| node.id.clone())
    });
    let editable_parent_id = editable_parent_id.or_else(|| parent_id.clone());
    let mut promoted_instances = if let Some(path) = editable_instance_map {
        read_editable_instance_map(&path)?
    } else {
        Vec::new()
    };
    let promotion = select_promoted_primitives(
        &reference_scene,
        &editable_part_names,
        editable_part_path_prefix.as_deref(),
    );
    let promoted_primitives = promotion.promoted;
    for spec in &editable_instances {
        for instance in reference_scene.instances.iter().filter(|instance| {
            instance.class == "Model"
                && instance.name == spec.source_name
                && editable_path_prefix
                    .as_deref()
                    .is_none_or(|prefix| instance.path.starts_with(prefix))
        }) {
            let Ok(frame) = reference_instance_frame(&reference_scene, &instance.path) else {
                continue;
            };
            promoted_instances.push(PromotedInstance {
                source_path: instance.path.clone(),
                mesh: spec.mesh.clone(),
                collision_asset: None,
                position: frame.position,
                rotation: [0.0, source_yaw(frame.rotation), 0.0],
                scale: [1.0; 3],
            });
        }
    }
    promoted_instances.sort_by(|left, right| left.source_path.cmp(&right.source_path));
    promoted_instances.dedup_by(|left, right| left.source_path == right.source_path);
    if (!promoted_instances.is_empty() || !promoted_primitives.is_empty())
        && editable_parent_id
            .as_deref()
            .is_some_and(|id| scene.node(id).is_none())
    {
        return Err(format!(
            "editable imported representation parent {:?} was not found",
            editable_parent_id.as_deref().unwrap_or_default()
        ));
    }
    if let Some(parent_id) = &parent_id
        && scene.node(parent_id).is_none()
    {
        return Err(format!(
            "import-roblox-scene parent node {parent_id:?} was not found"
        ));
    }

    let source_hash = short_hash(&reference_scene.source.place.sha256);
    let root_id = format!("source-hierarchy-{source_hash}");
    scene.nodes.retain(|node| {
        let generated_source = node
            .source
            .as_ref()
            .and_then(|source| source.properties.get("generatedBy"))
            .and_then(Value::as_str)
            == Some("import-roblox-scene");
        let editable_generated = !node.editor.locked
            && node
                .source
                .as_ref()
                .and_then(|source| source.properties.get("representation"))
                .and_then(Value::as_str)
                .is_some_and(|representation| {
                    matches!(
                        representation,
                        "editable-imported-instance" | "primitive" | "native-group"
                    )
                });
        let legacy_editable = !node.editor.locked
            && node
                .source
                .as_ref()
                .and_then(|source| source.path.as_deref())
                .is_some_and(|path| {
                    promoted_instances
                        .iter()
                        .any(|instance| instance.source_path == path)
                });
        !(reset_generated_source_tree && generated_source || editable_generated || legacy_editable)
    });
    let generated_ids = generated_scene_nodes(
        &reference_scene,
        &root_id,
        parent_id.as_deref(),
        tree_depth,
        &focus_paths,
        &source_index_reference,
        &promoted_instances,
        &promoted_primitives,
        editable_parent_id.as_deref(),
        editable_display_prefix.as_deref(),
        &editable_id_prefix,
        &promotion.statuses,
    );
    let existing_ids = scene
        .nodes
        .iter()
        .map(|node| node.id.clone())
        .collect::<BTreeSet<_>>();
    scene.nodes.extend(
        generated_ids
            .into_iter()
            .filter(|node| !existing_ids.contains(&node.id)),
    );
    scene.validate()?;
    let scene_source = if compact_output {
        scene.validate()?;
        format!(
            "{}\n",
            serde_json::to_string(&scene)
                .map_err(|error| format!("could not encode compact authoring scene: {error}"))?
        )
    } else {
        serialize_authoring_scene(&scene)?
    };
    write_text(&output, &scene_source)?;
    let shard_count = write_source_index(&source_index, &reference_scene)?;
    if let Some(path) = promotion_report_output {
        write_promotion_report(&path, &promotion.stats)?;
    }
    println!(
        "Imported Roblox source hierarchy: {} source nodes ({} in scene tree, depth {}, {} focus paths) -> {}",
        reference_scene.instances.len(),
        scene
            .nodes
            .iter()
            .filter(|node| {
                node.source
                    .as_ref()
                    .and_then(|source| source.properties.get("generatedBy"))
                    .and_then(Value::as_str)
                    == Some("import-roblox-scene")
            })
            .count(),
        tree_depth,
        focus_paths.len(),
        output.display()
    );
    println!(
        "  sharded source index ({} files) -> {}",
        shard_count,
        source_index.display()
    );
    println!(
        "  Parts: Workspace {} total, {} promoted, {} fallback (all source: {})",
        promotion.stats.workspace_parts,
        promotion.stats.promoted_parts,
        promotion.stats.fallback_parts,
        promotion.stats.source_parts
    );
    for (reason, count) in &promotion.stats.fallback_reasons {
        println!("    {reason}: {count}");
    }
    Ok(())
}

#[derive(Clone, Debug)]
struct EditableInstanceSpec {
    source_name: String,
    mesh: String,
}

#[derive(Clone, Debug)]
struct PromotedInstance {
    source_path: String,
    mesh: String,
    collision_asset: Option<String>,
    position: [f32; 3],
    rotation: [f32; 3],
    scale: [f32; 3],
}

#[derive(Clone, Debug)]
struct PromotedPrimitive {
    source_path: String,
    position: [f32; 3],
    rotation: [f32; 3],
    size: [f32; 3],
    material: String,
    runtime_material: Option<&'static str>,
    can_collide: bool,
    cast_shadow: bool,
}

#[derive(Clone, Debug, Default)]
struct PromotionStats {
    source_parts: usize,
    workspace_parts: usize,
    promoted_parts: usize,
    fallback_parts: usize,
    fallback_reasons: BTreeMap<String, usize>,
}

#[derive(Clone, Debug, Default)]
struct PromotionSelection {
    promoted: Vec<PromotedPrimitive>,
    statuses: BTreeMap<String, PromotionStatus>,
    stats: PromotionStats,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum PromotionStatus {
    Promoted { node_id: String },
    Fallback { reason: &'static str },
}

fn select_promoted_primitives(
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
        if !is_workspace_path(&geometry.path) {
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

fn record_fallback(
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

fn can_promote_part(geometry: &GeometryInstance) -> Result<(), &'static str> {
    if geometry.class != "Part" {
        return Err("unsupported-class");
    }
    // Roblox Enum.PartType uses Block = 1; missing Shape is treated as the
    // ordinary Part default for older normalized fixtures.
    if geometry.shape.is_some_and(|shape| shape != 1) {
        return Err("unsupported-shape");
    }
    if geometry.mesh.is_some() {
        return Err("unsupported-mesh");
    }
    if !geometry.anchored {
        return Err("unsupported-dynamic");
    }
    if !source_rotation_is_axis_aligned(geometry.transform.rotation) {
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
        ) && cubacadabra_reference_import::roblox_material_runtime_name(name).is_none()
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

fn primitive_node_id(source_path: &str) -> String {
    format!("imported-part-{}", short_hash(source_path))
}

fn is_workspace_path(path: &str) -> bool {
    path == WORKSPACE_ROOT || path.starts_with(&format!("{WORKSPACE_ROOT}/"))
}

fn write_promotion_report(path: &Path, stats: &PromotionStats) -> Result<(), String> {
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

fn read_editable_instance_map(path: &Path) -> Result<Vec<PromotedInstance>, String> {
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

fn vector3(value: Option<&Value>, label: &str) -> Result<[f32; 3], String> {
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

fn generated_scene_nodes(
    reference: &ReferenceScene,
    root_id: &str,
    parent_id: Option<&str>,
    tree_depth: usize,
    focus_paths: &[String],
    source_index_reference: &str,
    promoted_instances: &[PromotedInstance],
    promoted_primitives: &[PromotedPrimitive],
    editable_parent_id: Option<&str>,
    editable_display_prefix: Option<&str>,
    editable_id_prefix: &str,
    promotion_statuses: &BTreeMap<String, PromotionStatus>,
) -> Vec<AuthoringNode> {
    let mut nodes = vec![AuthoringNode {
        id: root_id.to_owned(),
        parent_id: parent_id.map(str::to_owned),
        name: "Roblox Source".to_owned(),
        transform: Transform::default(),
        components: BTreeMap::new(),
        editor: EditorMetadata {
            visible: true,
            locked: true,
            lock_reason: Some(
                "Source hierarchy is read-only; extracted render assets are authored separately"
                    .to_owned(),
            ),
        },
        source: Some(SourceMetadata {
            format: "roblox".to_owned(),
            class: Some("SourceHierarchy".to_owned()),
            path: None,
            properties: BTreeMap::from([
                ("generatedBy".to_owned(), json!("import-roblox-scene")),
                ("sourceIndex".to_owned(), json!(source_index_reference)),
                ("instanceCount".to_owned(), json!(reference.instances.len())),
                ("geometryCount".to_owned(), json!(reference.geometry.len())),
            ]),
        }),
    }];
    let included = reference
        .instances
        .iter()
        .filter(|instance| {
            instance.path.split('/').count() <= tree_depth
                || focus_paths.iter().any(|focus| {
                    instance.path == *focus
                        || instance.path.starts_with(&format!("{focus}/"))
                        || focus.starts_with(&format!("{}/", instance.path))
                })
        })
        .map(|instance| instance.path.as_str())
        .collect::<BTreeSet<_>>();
    let promoted_paths = promoted_instances
        .iter()
        .map(|instance| instance.source_path.as_str())
        .chain(
            promoted_primitives
                .iter()
                .map(|primitive| primitive.source_path.as_str()),
        )
        .collect::<Vec<_>>();
    let (native_groups, native_group_ids) =
        native_group_hierarchy(reference, &promoted_paths, editable_parent_id);
    for instance in &reference.instances {
        if !included.contains(instance.path.as_str()) {
            continue;
        }
        let id = source_node_id(&instance.path);
        let parent = if included.contains(instance.parent_path.as_str()) {
            source_node_id(&instance.parent_path)
        } else {
            root_id.to_owned()
        };
        let mut properties =
            BTreeMap::from([("generatedBy".to_owned(), json!("import-roblox-scene"))]);
        let geometry_count = reference
            .geometry
            .iter()
            .filter(|geometry| geometry.path == instance.path)
            .count();
        if geometry_count > 0 {
            properties.insert("geometryCount".to_owned(), json!(geometry_count));
        }
        if let Some(status) = promotion_statuses.get(&instance.path) {
            match status {
                PromotionStatus::Promoted { node_id } => {
                    properties.insert("promotionStatus".to_owned(), json!("promoted"));
                    properties.insert("nativeRepresentationId".to_owned(), json!(node_id));
                }
                PromotionStatus::Fallback { reason } => {
                    properties.insert("promotionStatus".to_owned(), json!("fallback"));
                    properties.insert("promotionReason".to_owned(), json!(reason));
                }
            }
        }
        if let Some(native_id) = native_group_ids.get(&instance.path) {
            properties.insert("representation".to_owned(), json!("native-group"));
            properties.insert("nativeRepresentationId".to_owned(), json!(native_id));
        }
        nodes.push(AuthoringNode {
            id,
            parent_id: Some(parent),
            name: instance.name.clone(),
            transform: Transform {
                position: instance
                    .transform
                    .as_ref()
                    .map(|transform| transform.position)
                    .unwrap_or([0.0; 3]),
                ..Transform::default()
            },
            components: BTreeMap::new(),
            editor: EditorMetadata {
                visible: true,
                locked: true,
                lock_reason: Some(
                    "Imported source node has no editable native representation yet".to_owned(),
                ),
            },
            source: Some(SourceMetadata {
                format: "roblox".to_owned(),
                class: Some(instance.class.clone()),
                path: Some(instance.path.clone()),
                properties,
            }),
        });
    }
    nodes.extend(native_groups);
    let native_parent = |source_path: &str| {
        reference
            .instances
            .iter()
            .find(|instance| instance.path == source_path)
            .and_then(|instance| native_group_ids.get(&instance.parent_path))
            .cloned()
            .or_else(|| editable_parent_id.map(str::to_owned))
    };
    let mut editable_number = 0;
    for promoted in promoted_instances {
        let Some(instance) = reference
            .instances
            .iter()
            .find(|instance| instance.path == promoted.source_path)
        else {
            continue;
        };
        editable_number += 1;
        let id = format!("{}-{}", editable_id_prefix, short_hash(&instance.path));
        let display_name = editable_display_prefix
            .map(|prefix| format!("{prefix} {editable_number}"))
            .unwrap_or_else(|| format!("{} — {}", instance.name, editable_number));
        let mut properties = BTreeMap::from([
            ("generatedBy".to_owned(), json!("import-roblox-scene")),
            (
                "representation".to_owned(),
                json!("editable-imported-instance"),
            ),
            (
                "sourceFrame".to_owned(),
                json!("inferred-from-first-descendant-geometry"),
            ),
            ("regenerationPolicy".to_owned(), json!("source-generated")),
        ]);
        properties.insert("asset".to_owned(), json!(promoted.mesh));
        nodes.push(AuthoringNode {
            id,
            parent_id: native_parent(&instance.path),
            name: display_name,
            transform: Transform {
                position: promoted.position,
                rotation: promoted.rotation,
                scale: promoted.scale,
            },
            components: {
                let mut components =
                    BTreeMap::from([("render".to_owned(), json!({"mesh": promoted.mesh}))]);
                if let Some(asset) = &promoted.collision_asset {
                    components.insert(
                        "collision".to_owned(),
                        json!({"kind": "mesh", "asset": asset}),
                    );
                }
                components
            },
            editor: EditorMetadata::default(),
            source: Some(SourceMetadata {
                format: "roblox".to_owned(),
                class: Some(instance.class.clone()),
                path: Some(instance.path.clone()),
                properties,
            }),
        });
    }
    let mut primitive_number = 0;
    for promoted in promoted_primitives {
        let Some(instance) = reference
            .instances
            .iter()
            .find(|instance| instance.path == promoted.source_path)
        else {
            continue;
        };
        primitive_number += 1;
        let id = primitive_node_id(&instance.path);
        let mut properties = BTreeMap::from([
            ("generatedBy".to_owned(), json!("import-roblox-scene")),
            ("representation".to_owned(), json!("primitive")),
            ("sourceColor".to_owned(), json!(promoted.material.clone())),
            ("sourceCanCollide".to_owned(), json!(promoted.can_collide)),
            ("sourceCastShadow".to_owned(), json!(promoted.cast_shadow)),
            (
                "sourceFrame".to_owned(),
                json!("geometry-transform-and-bounds"),
            ),
            ("regenerationPolicy".to_owned(), json!("source-generated")),
        ]);
        if let Some(material) = reference
            .geometry
            .iter()
            .find(|geometry| geometry.path == promoted.source_path)
            .and_then(|geometry| geometry.material.name.as_deref())
        {
            properties.insert("sourceMaterial".to_owned(), json!(material));
        }
        let mut components = BTreeMap::from([("primitive".to_owned(), {
            let mut primitive = json!({
                "shape": "box",
                "size": promoted.size,
                "material": promoted.material.clone(),
                "collidable": promoted.can_collide,
                "castShadow": promoted.cast_shadow,
            });
            if let Some(runtime_material) = promoted.runtime_material {
                primitive["runtimeMaterial"] = json!(runtime_material);
                properties.insert("runtimeMaterial".to_owned(), json!(runtime_material));
            }
            primitive
        })]);
        if promoted.can_collide {
            components.insert("collision".to_owned(), json!({"kind": "box"}));
        }
        nodes.push(AuthoringNode {
            id,
            parent_id: native_parent(&instance.path),
            name: format!("{} — {}", instance.name, primitive_number),
            transform: Transform {
                position: promoted.position,
                rotation: promoted.rotation,
                scale: [1.0; 3],
            },
            components,
            editor: EditorMetadata::default(),
            source: Some(SourceMetadata {
                format: "roblox".to_owned(),
                class: Some(instance.class.clone()),
                path: Some(instance.path.clone()),
                properties,
            }),
        });
    }
    nodes
}

fn native_group_hierarchy(
    reference: &ReferenceScene,
    promoted_paths: &[&str],
    fallback_parent_id: Option<&str>,
) -> (Vec<AuthoringNode>, BTreeMap<String, String>) {
    let instances = reference
        .instances
        .iter()
        .map(|instance| (instance.path.as_str(), instance))
        .collect::<BTreeMap<_, _>>();
    let mut paths = BTreeSet::new();
    for promoted_path in promoted_paths {
        let Some(instance) = instances.get(promoted_path) else {
            continue;
        };
        let mut ancestor_path = instance.parent_path.as_str();
        while !ancestor_path.is_empty() {
            let Some(ancestor) = instances.get(ancestor_path) else {
                break;
            };
            if matches!(ancestor.class.as_str(), "Model" | "Folder") {
                paths.insert(ancestor.path.clone());
            }
            ancestor_path = ancestor.parent_path.as_str();
        }
    }
    let mut ordered_paths = paths.into_iter().collect::<Vec<_>>();
    ordered_paths.sort_by_key(|path| (path.split('/').count(), path.clone()));
    let mut ids = BTreeMap::new();
    let mut nodes = Vec::new();
    for path in ordered_paths {
        let Some(instance) = instances.get(path.as_str()) else {
            continue;
        };
        let id = format!("imported-group-{}", short_hash(&path));
        let parent_id = instances
            .get(instance.parent_path.as_str())
            .and_then(|parent| ids.get(parent.path.as_str()))
            .cloned()
            .or_else(|| fallback_parent_id.map(str::to_owned));
        ids.insert(path.clone(), id.clone());
        nodes.push(AuthoringNode {
            id,
            parent_id,
            name: instance.name.clone(),
            transform: Transform::default(),
            components: BTreeMap::new(),
            editor: EditorMetadata::default(),
            source: Some(SourceMetadata {
                format: "roblox".to_owned(),
                class: Some(instance.class.clone()),
                path: Some(path),
                properties: BTreeMap::from([
                    ("generatedBy".to_owned(), json!("import-roblox-scene")),
                    ("representation".to_owned(), json!("native-group")),
                    ("regenerationPolicy".to_owned(), json!("source-generated")),
                ]),
            }),
        });
    }
    (nodes, ids)
}

fn source_rotation_to_euler(rotation: [[f32; 3]; 3]) -> [f32; 3] {
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
    if !source_rotation_is_axis_aligned(rotation) {
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

fn source_yaw(rotation: [[f32; 3]; 3]) -> f32 {
    rotation[0][2].atan2(rotation[0][0])
}

fn source_color(color: [f32; 3]) -> String {
    let channel = |value: f32| (value.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!(
        "#{:02X}{:02X}{:02X}",
        channel(color[0]),
        channel(color[1]),
        channel(color[2])
    )
}

fn source_rotation_is_axis_aligned(rotation: [[f32; 3]; 3]) -> bool {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::{FRAC_PI_2, PI};

    fn source_rows(matrix: Mat3) -> [[f32; 3]; 3] {
        [
            [matrix.x_axis.x, matrix.y_axis.x, matrix.z_axis.x],
            [matrix.x_axis.y, matrix.y_axis.y, matrix.z_axis.y],
            [matrix.x_axis.z, matrix.y_axis.z, matrix.z_axis.z],
        ]
    }

    fn close(left: f32, right: f32) {
        assert!((left - right).abs() < 0.0001, "{left} != {right}");
    }

    #[test]
    fn converts_axis_aligned_xyz_rotations_without_dropping_axes() {
        for (matrix, expected) in [
            (Mat3::from_rotation_y(FRAC_PI_2), [0.0, FRAC_PI_2, 0.0]),
            (Mat3::from_rotation_x(FRAC_PI_2), [FRAC_PI_2, 0.0, 0.0]),
            (Mat3::from_rotation_z(-FRAC_PI_2), [0.0, 0.0, -FRAC_PI_2]),
        ] {
            let source = source_rows(matrix);
            assert!(source_rotation_is_axis_aligned(source));
            for (actual, expected) in source_rotation_to_euler(source).into_iter().zip(expected) {
                close(actual, expected);
            }
        }
    }

    #[test]
    fn rejects_non_axis_aligned_and_reflected_matrices() {
        assert!(!source_rotation_is_axis_aligned(source_rows(
            Mat3::from_rotation_y(0.25),
        )));
        assert!(!source_rotation_is_axis_aligned([
            [-1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
        ]));
    }

    #[test]
    fn rejects_non_block_shapes_and_dynamic_parts_but_promotes_non_shadowing_parts() {
        let mut value = reference_fixture();
        value["geometry"][0]["shape"] = json!(2);
        value["geometry"][1]["anchored"] = json!(false);
        value["geometry"][2]["castShadow"] = json!(false);
        let reference: ReferenceScene = serde_json::from_value(value).unwrap();
        let selection = select_promoted_primitives(&reference, &[], None);
        assert!(!selection.promoted.iter().any(|part| part.source_path
            == "Workspace:Workspace[1]/Folder:Imported[1]/Part:DirtTrack[1]"));
        assert_eq!(
            selection.statuses["Workspace:Workspace[1]/Folder:Imported[1]/Part:DirtTrack[1]"],
            PromotionStatus::Fallback {
                reason: "unsupported-shape"
            }
        );
        assert_eq!(
            selection.statuses["Workspace:Workspace[1]/Folder:Imported[1]/Part:Decor[1]"],
            PromotionStatus::Fallback {
                reason: "unsupported-dynamic"
            }
        );
        assert_eq!(
            selection.statuses["Workspace:Workspace[1]/Folder:Imported[1]/Part:RotX[1]"],
            PromotionStatus::Promoted {
                node_id: primitive_node_id(
                    "Workspace:Workspace[1]/Folder:Imported[1]/Part:RotX[1]"
                )
            }
        );
        assert!(
            !selection
                .promoted
                .iter()
                .find(|part| {
                    part.source_path == "Workspace:Workspace[1]/Folder:Imported[1]/Part:RotX[1]"
                })
                .expect("CastShadow=false Part should be promoted")
                .cast_shadow
        );
    }

    #[test]
    fn preserves_supported_materials_and_keeps_unmapped_materials_in_fallback() {
        let mut value = reference_fixture();
        value["geometry"][0]["material"]["name"] = json!("Concrete");
        let reference: ReferenceScene = serde_json::from_value(value).unwrap();
        let selection = select_promoted_primitives(&reference, &[], None);
        assert_eq!(selection.promoted[0].runtime_material, Some("builtin:rock"));

        let mut value = reference_fixture();
        value["geometry"][0]["material"]["name"] = json!("Fabric");
        let reference: ReferenceScene = serde_json::from_value(value).unwrap();
        let selection = select_promoted_primitives(&reference, &[], None);
        assert!(!selection.promoted.iter().any(|part| {
            part.source_path == "Workspace:Workspace[1]/Folder:Imported[1]/Part:DirtTrack[1]"
        }));
        assert_eq!(
            selection.statuses["Workspace:Workspace[1]/Folder:Imported[1]/Part:DirtTrack[1]"],
            PromotionStatus::Fallback {
                reason: "unsupported-material"
            }
        );
    }

    fn reference_fixture() -> Value {
        let identity = source_rows(Mat3::IDENTITY);
        let part = |path: &str, name: &str, rotation: [[f32; 3]; 3], can_collide: bool| {
            json!({
                "path": path,
                "parentPath": "Workspace:Workspace[1]/Folder:Imported[1]",
                "class": "Part",
                "name": name,
                "transform": {"position": [1.0, 2.0, 3.0], "rotation": rotation},
                "size": [2.0, 1.0, 4.0],
                "color": [0.5, 0.25, 0.1],
                "material": {"value": 0, "name": "Plastic"},
                "transparency": 0.0,
                "reflectance": 0.0,
                "anchored": true,
                "canCollide": can_collide,
                "castShadow": true
            })
        };
        let instances = [
            json!({"path":"Workspace:Workspace[1]","parentPath":"","class":"Workspace","name":"Workspace"}),
            json!({"path":"Workspace:Workspace[1]/Folder:Imported[1]","parentPath":"Workspace:Workspace[1]","class":"Folder","name":"Imported"}),
            json!({"path":"Workspace:Workspace[1]/Folder:Imported[1]/Part:DirtTrack[1]","parentPath":"Workspace:Workspace[1]/Folder:Imported[1]","class":"Part","name":"DirtTrack"}),
            json!({"path":"Workspace:Workspace[1]/Folder:Imported[1]/Part:Decor[1]","parentPath":"Workspace:Workspace[1]/Folder:Imported[1]","class":"Part","name":"Decor"}),
            json!({"path":"Workspace:Workspace[1]/Folder:Imported[1]/Part:RotX[1]","parentPath":"Workspace:Workspace[1]/Folder:Imported[1]","class":"Part","name":"RotX"}),
            json!({"path":"Workspace:Workspace[1]/Folder:Imported[1]/Part:RotZ[1]","parentPath":"Workspace:Workspace[1]/Folder:Imported[1]","class":"Part","name":"RotZ"}),
            json!({"path":"Workspace:Workspace[1]/Folder:Imported[1]/Part:Tilt[1]","parentPath":"Workspace:Workspace[1]/Folder:Imported[1]","class":"Part","name":"Tilt"}),
            json!({"path":"Workspace:Workspace[1]/Part:Elsewhere[1]","parentPath":"Workspace:Workspace[1]","class":"Part","name":"DirtTrack"}),
            json!({"path":"ServerStorage:ServerStorage[1]/Folder:Templates[1]/Part:Template[1]","parentPath":"ServerStorage:ServerStorage[1]/Folder:Templates[1]","class":"Part","name":"Template"}),
        ];
        json!({
            "formatVersion": 1,
            "kind": "roblox-static-reference-scene",
            "coordinateSystem": "Roblox source coordinates",
            "source": {"place": {"name": "Place.rbxmx", "bytes": 1, "sha256": "0123456789abcdef0123456789abcdef"}},
            "summary": {"instanceCount": 9, "geometryCount": 7, "visibleGeometryCount": 7, "cameraCount": 0, "lightCount": 0, "textureCount": 0, "textCount": 0, "spawnCount": 0},
            "bounds": null,
            "classCounts": {},
            "instances": instances,
            "geometry": [
                part("Workspace:Workspace[1]/Folder:Imported[1]/Part:DirtTrack[1]", "DirtTrack", identity, true),
                part("Workspace:Workspace[1]/Folder:Imported[1]/Part:Decor[1]", "Decor", identity, false),
                part("Workspace:Workspace[1]/Folder:Imported[1]/Part:RotX[1]", "RotX", source_rows(Mat3::from_rotation_x(FRAC_PI_2)), true),
                part("Workspace:Workspace[1]/Folder:Imported[1]/Part:RotZ[1]", "RotZ", source_rows(Mat3::from_rotation_z(-FRAC_PI_2)), true),
                part("Workspace:Workspace[1]/Folder:Imported[1]/Part:Tilt[1]", "Tilt", source_rows(Mat3::from_rotation_y(PI / 4.0)), true),
                part("Workspace:Workspace[1]/Part:Elsewhere[1]", "DirtTrack", identity, true),
                part("ServerStorage:ServerStorage[1]/Folder:Templates[1]/Part:Template[1]", "Template", identity, true)
            ],
            "cameras": [], "lights": [], "textures": [], "texts": [], "spawns": [],
            "projectLighting": null, "terrain": null
        })
    }

    #[test]
    fn import_promotes_all_supported_parts_with_stable_paths_and_groups() {
        let temp = tempfile::tempdir().unwrap();
        let reference = temp.path().join("reference.json");
        let base = temp.path().join("scene.json");
        let output = temp.path().join("out-scene.json");
        let source_index = temp.path().join("imports/roblox/place/index.json");
        let report = temp.path().join("promotion-report.json");
        fs::write(
            &reference,
            serde_json::to_vec(&reference_fixture()).unwrap(),
        )
        .unwrap();
        fs::write(
            &base,
            r#"{"formatVersion":1,"nodes":[{"id":"imported-environment","name":"Environment"}]}"#,
        )
        .unwrap();
        let args = vec![
            "--reference",
            reference.to_str().unwrap(),
            "--base-scene",
            base.to_str().unwrap(),
            "--output",
            output.to_str().unwrap(),
            "--source-index",
            source_index.to_str().unwrap(),
            "--promotion-report-output",
            report.to_str().unwrap(),
        ]
        .into_iter()
        .map(str::to_owned)
        .collect::<Vec<_>>();

        import_roblox_scene_command(&args).unwrap();
        import_roblox_scene_command(&args).unwrap();
        let scene = parse_authoring_scene(&fs::read_to_string(&output).unwrap()).unwrap();
        let promoted = scene
            .nodes
            .iter()
            .filter(|node| node.id.starts_with("imported-part-"))
            .collect::<Vec<_>>();
        assert_eq!(promoted.len(), 5);
        assert_eq!(
            promoted
                .iter()
                .filter(|node| node.components.contains_key("collision"))
                .count(),
            4
        );
        let decor = scene
            .nodes
            .iter()
            .find(|node| node.name.starts_with("Decor —"));
        assert!(decor.is_some(), "non-colliding Parts remain editable");
        assert!(decor.unwrap().components.get("collision").is_none());
        let rot_x = scene
            .nodes
            .iter()
            .find(|node| node.name.starts_with("RotX —"))
            .unwrap();
        close(rot_x.transform.rotation[0], FRAC_PI_2);
        let rot_z = scene
            .nodes
            .iter()
            .find(|node| node.name.starts_with("RotZ —"))
            .unwrap();
        close(rot_z.transform.rotation[2], -FRAC_PI_2);
        assert!(
            scene
                .nodes
                .iter()
                .all(|node| !node.name.starts_with("Tilt —"))
        );
        assert_eq!(
            scene
                .nodes
                .iter()
                .filter(|node| node.id.starts_with("imported-group-"))
                .count(),
            1
        );
        let dirt_tracks = scene
            .nodes
            .iter()
            .filter(|node| node.name.starts_with("DirtTrack —"))
            .collect::<Vec<_>>();
        assert_eq!(
            dirt_tracks.len(),
            2,
            "duplicate names must remain distinct by path"
        );
        assert_ne!(dirt_tracks[0].id, dirt_tracks[1].id);
        let outside = scene
            .nodes
            .iter()
            .find(|node| {
                node.source
                    .as_ref()
                    .and_then(|source| source.path.as_deref())
                    == Some("Workspace:Workspace[1]/Part:Elsewhere[1]")
            })
            .unwrap();
        assert!(outside.editor.locked);
        assert_eq!(
            outside.source.as_ref().unwrap().properties["promotionStatus"],
            "promoted"
        );
        let tilt = scene
            .nodes
            .iter()
            .find(|node| {
                node.source
                    .as_ref()
                    .and_then(|source| source.path.as_deref())
                    == Some("Workspace:Workspace[1]/Folder:Imported[1]/Part:Tilt[1]")
            })
            .unwrap();
        assert_eq!(
            tilt.source.as_ref().unwrap().properties["promotionReason"],
            "unsupported-transform"
        );
        let report: Value = serde_json::from_str(&fs::read_to_string(report).unwrap()).unwrap();
        assert_eq!(report["parts"]["sourceTotal"], 7);
        assert_eq!(report["parts"]["total"], 6);
        assert_eq!(report["parts"]["promoted"], 5);
        assert_eq!(report["parts"]["fallback"], 1);
    }
}
