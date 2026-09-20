use super::*;

pub(crate) fn import_roblox_scene_command(args: &[String]) -> Result<(), String> {
    let mut reference = None;
    let mut base_scene = None;
    let mut output = None;
    let mut source_index = None;
    let mut parent_id = None;
    let mut tree_depth = 4usize;
    let mut focus_paths = Vec::new();
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
        node.id != root_id
            && node
                .source
                .as_ref()
                .and_then(|source| source.properties.get("generatedBy"))
                .and_then(Value::as_str)
                != Some("import-roblox-scene")
    });
    let generated_ids = generated_scene_nodes(
        &reference_scene,
        &root_id,
        parent_id.as_deref(),
        tree_depth,
        &focus_paths,
        &source_index_reference,
    );
    scene.nodes.extend(generated_ids);
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

pub(crate) fn generated_scene_nodes(
    reference: &ReferenceScene,
    root_id: &str,
    parent_id: Option<&str>,
    tree_depth: usize,
    focus_paths: &[String],
    source_index_reference: &str,
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
    nodes
}
