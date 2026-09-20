use super::*;

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
    let promoted_primitives = reference_scene
        .geometry
        .iter()
        .filter(|geometry| {
            geometry.class == "Part"
                && geometry.can_collide
                && source_rotation_is_axis_aligned(geometry.transform.rotation)
                && editable_part_names
                    .iter()
                    .any(|name| name == &geometry.name)
                && editable_part_path_prefix
                    .as_deref()
                    .is_none_or(|prefix| geometry.path.starts_with(prefix))
        })
        .map(|geometry| PromotedPrimitive {
            source_path: geometry.path.clone(),
            position: geometry.transform.position,
            rotation: [0.0, source_yaw(geometry.transform.rotation), 0.0],
            size: geometry.size,
            material: source_color(geometry.color),
        })
        .collect::<Vec<_>>();
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
        let editable_generated = node
            .source
            .as_ref()
            .and_then(|source| source.properties.get("representation"))
            .and_then(Value::as_str)
            .is_some_and(|representation| {
                matches!(representation, "editable-imported-instance" | "primitive")
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
    let scene_source = serialize_authoring_scene(&scene)?;
    write_text(&output, &scene_source)?;
    let shard_count = write_source_index(&source_index, &reference_scene)?;
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
        ]);
        properties.insert("asset".to_owned(), json!(promoted.mesh));
        nodes.push(AuthoringNode {
            id,
            parent_id: editable_parent_id.map(str::to_owned),
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
        let id = format!("imported-part-{}", short_hash(&instance.path));
        let mut properties = BTreeMap::from([
            ("generatedBy".to_owned(), json!("import-roblox-scene")),
            ("representation".to_owned(), json!("primitive")),
            ("sourceColor".to_owned(), json!(promoted.material.clone())),
            (
                "sourceFrame".to_owned(),
                json!("geometry-transform-and-bounds"),
            ),
        ]);
        if let Some(material) = reference
            .geometry
            .iter()
            .find(|geometry| geometry.path == promoted.source_path)
            .and_then(|geometry| geometry.material.name.as_deref())
        {
            properties.insert("sourceMaterial".to_owned(), json!(material));
        }
        nodes.push(AuthoringNode {
            id,
            parent_id: editable_parent_id.map(str::to_owned),
            name: format!("{} — {}", instance.name, primitive_number),
            transform: Transform {
                position: promoted.position,
                rotation: promoted.rotation,
                scale: [1.0; 3],
            },
            components: BTreeMap::from([
                (
                    "primitive".to_owned(),
                    json!({
                        "shape": "box",
                        "size": promoted.size,
                        "material": promoted.material.clone(),
                    }),
                ),
                ("collision".to_owned(), json!({"kind": "box"})),
            ]),
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
    rows_are_axes && columns_are_axes
}
