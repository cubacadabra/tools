use cubacadabra_builder::{BuildOptions, build_game};
use cubacadabra_project::create_game;
use cubacadabra_reference_import::{
    ImportOptions, MeshExportOptions, ReferenceScene, export_reference_mesh, import_reference,
    read_reference_scene,
};
use cubacadabra_scene::{
    AuthoringNode, AuthoringScene, EditorMetadata, SourceMetadata, Transform,
    parse_authoring_scene, serialize_authoring_scene,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
    path::{Path, PathBuf},
    process::ExitCode,
};

const SOURCE_INDEX_FORMAT_VERSION: u32 = 2;
const SOURCE_SHARD_TARGET_BYTES: usize = 3 * 1024 * 1024;
const SOURCE_SHARD_MAX_BYTES: usize = 4 * 1024 * 1024;

fn main() -> ExitCode {
    match run(env::args().skip(1).collect()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("cubacadabra: {error}");
            ExitCode::from(1)
        }
    }
}

fn run(args: Vec<String>) -> Result<(), String> {
    if args.is_empty() || args.iter().any(|arg| arg == "--help" || arg == "-h") {
        print_help();
        return Ok(());
    }
    if args.len() == 1 && args[0] == "--version" {
        println!("cubacadabra 0.4.0");
        return Ok(());
    }
    match args[0].as_str() {
        "build-game" => build_command(&args[1..]),
        "create-game" => create_command(&args[1..]),
        "--create-game" => create_command(&args[1..]),
        "import-roblox-reference" => import_roblox_reference_command(&args[1..]),
        "import-roblox-scene" => import_roblox_scene_command(&args[1..]),
        "migrate-source-index" => migrate_source_index_command(&args[1..]),
        "export-reference-mesh" => export_reference_mesh_command(&args[1..]),
        command => Err(format!("unknown command {command:?}; use --help")),
    }
}

fn migrate_source_index_command(args: &[String]) -> Result<(), String> {
    let mut input = None;
    let mut output = None;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--input" => {
                index += 1;
                input = Some(PathBuf::from(required_arg(args, index, "--input")?));
            }
            "--output" => {
                index += 1;
                output = Some(PathBuf::from(required_arg(args, index, "--output")?));
            }
            value => return Err(format!("unknown migrate-source-index option {value}")),
        }
        index += 1;
    }
    let input = input.ok_or_else(|| "migrate-source-index requires --input".to_owned())?;
    let output = output.ok_or_else(|| "migrate-source-index requires --output".to_owned())?;
    let legacy: Value = serde_json::from_slice(
        &fs::read(&input)
            .map_err(|error| format!("could not read {}: {error}", input.display()))?,
    )
    .map_err(|error| format!("could not decode {}: {error}", input.display()))?;
    if legacy.get("formatVersion").and_then(Value::as_u64) != Some(1)
        || legacy.get("kind").and_then(Value::as_str) != Some("cubacadabra-roblox-source-hierarchy")
    {
        return Err(format!(
            "{} is not a supported v1 source hierarchy index",
            input.display()
        ));
    }
    let nodes = legacy
        .get("nodes")
        .and_then(Value::as_array)
        .ok_or_else(|| format!("{} has no v1 nodes array", input.display()))?
        .iter()
        .map(normalize_legacy_source_node)
        .collect::<Result<Vec<_>, _>>()?;
    let shard_count = write_sharded_source_index(
        &output,
        legacy.get("source").cloned().unwrap_or(Value::Null),
        legacy
            .get("instanceCount")
            .and_then(Value::as_u64)
            .unwrap_or(nodes.len() as u64) as usize,
        legacy
            .get("geometryCount")
            .and_then(Value::as_u64)
            .unwrap_or(0) as usize,
        nodes,
    )?;
    println!(
        "Migrated source hierarchy: {} -> {} shard files in {}",
        input.display(),
        shard_count,
        output.display()
    );
    Ok(())
}

fn normalize_legacy_source_node(node: &Value) -> Result<Value, String> {
    let path = node
        .get("path")
        .and_then(Value::as_str)
        .ok_or_else(|| "legacy source node is missing path".to_owned())?;
    let parent_path = node.get("parentPath").and_then(Value::as_str).unwrap_or("");
    let class = node
        .get("class")
        .and_then(Value::as_str)
        .ok_or_else(|| format!("legacy source node {path:?} is missing class"))?;
    let name = node
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| format!("legacy source node {path:?} is missing name"))?;
    let mut normalized = json!({
        "id": source_node_id(path),
        "parent": if parent_path.is_empty() {
            Value::Null
        } else {
            json!(source_node_id(parent_path))
        },
        "sourceSegment": source_segment(path),
        "class": class,
        "name": name,
    });
    if let Some(transform) = node.get("transform").filter(|value| !value.is_null()) {
        normalized["transform"] = transform.clone();
    }
    if let Some(geometry_count) = node
        .get("geometryCount")
        .and_then(Value::as_u64)
        .filter(|count| *count > 0)
    {
        normalized["geometryCount"] = json!(geometry_count);
    }
    Ok(normalized)
}

fn import_roblox_scene_command(args: &[String]) -> Result<(), String> {
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

fn generated_scene_nodes(
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

fn write_source_index(path: &Path, reference: &ReferenceScene) -> Result<usize, String> {
    let mut geometry_by_path = BTreeMap::<&str, usize>::new();
    for geometry in &reference.geometry {
        *geometry_by_path.entry(geometry.path.as_str()).or_default() += 1;
    }
    let nodes = reference
        .instances
        .iter()
        .map(|instance| {
            let geometry_count = geometry_by_path
                .get(instance.path.as_str())
                .copied()
                .unwrap_or(0);
            let mut node = json!({
                "id": source_node_id(&instance.path),
                "parent": if instance.parent_path.is_empty() {
                    Value::Null
                } else {
                    json!(source_node_id(&instance.parent_path))
                },
                "sourceSegment": source_segment(&instance.path),
                "class": instance.class,
                "name": instance.name,
            });
            if let Some(transform) = &instance.transform {
                node["transform"] = json!(transform);
            }
            if geometry_count > 0 {
                node["geometryCount"] = json!(geometry_count);
            }
            node
        })
        .collect::<Vec<_>>();

    write_sharded_source_index(
        path,
        json!(reference.source),
        reference.instances.len(),
        reference.geometry.len(),
        nodes,
    )
}

fn write_sharded_source_index(
    path: &Path,
    source: Value,
    instance_count: usize,
    geometry_count: usize,
    nodes: Vec<Value>,
) -> Result<usize, String> {
    let node_store_name = if path.file_name().and_then(|name| name.to_str()) == Some("index.json") {
        "nodes".to_owned()
    } else {
        format!(
            "{}.nodes",
            path.file_stem()
                .and_then(|stem| stem.to_str())
                .unwrap_or("source-hierarchy")
        )
    };
    let node_store = path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(&node_store_name);
    if node_store.exists() {
        fs::remove_dir_all(&node_store)
            .map_err(|error| format!("could not replace {}: {error}", node_store.display()))?;
    }
    fs::create_dir_all(&node_store)
        .map_err(|error| format!("could not create {}: {error}", node_store.display()))?;

    let shards = shard_source_nodes(&nodes)?;
    for shard in &shards {
        let shard_path = node_store.join(&shard.file_name);
        if let Some(parent) = shard_path.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| format!("could not create {}: {error}", parent.display()))?;
        }
        write_text(&shard_path, &format!("{}\n", shard.encoded))?;
    }

    let index = json!({
        "formatVersion": SOURCE_INDEX_FORMAT_VERSION,
        "kind": "cubacadabra-roblox-source-hierarchy",
        "source": source,
        "instanceCount": instance_count,
        "geometryCount": geometry_count,
        "nodeStore": {
            "kind": "sharded-json",
            "path": node_store_name,
            "targetBytes": SOURCE_SHARD_TARGET_BYTES,
            "maxBytes": SOURCE_SHARD_MAX_BYTES,
            "shards": shards.iter().map(|shard| json!({
                "path": shard.file_name,
                "nodeCount": shard.node_count,
                "bytes": shard.bytes,
            })).collect::<Vec<_>>(),
        },
    });
    let encoded = serde_json::to_string_pretty(&index)
        .map_err(|error| format!("could not encode source hierarchy: {error}"))?;
    write_text(path, &format!("{encoded}\n"))?;
    Ok(shards.len())
}

struct SourceShard {
    file_name: String,
    node_count: usize,
    bytes: usize,
    encoded: String,
}

fn shard_source_nodes(nodes: &[Value]) -> Result<Vec<SourceShard>, String> {
    let mut buckets = BTreeMap::<String, Vec<Value>>::new();
    for node in nodes {
        let id = node
            .get("id")
            .and_then(Value::as_str)
            .ok_or_else(|| "source node is missing a string id".to_owned())?;
        let prefix = id
            .strip_prefix("source-")
            .and_then(|value| value.chars().next())
            .ok_or_else(|| format!("source node id {id:?} has no hash prefix"))?;
        buckets
            .entry(prefix.to_string())
            .or_default()
            .push(node.clone());
    }

    let mut shards = Vec::new();
    for (prefix, nodes) in buckets {
        split_source_bucket(prefix, nodes, &mut shards)?;
    }
    Ok(shards)
}

fn split_source_bucket(
    prefix: String,
    nodes: Vec<Value>,
    shards: &mut Vec<SourceShard>,
) -> Result<(), String> {
    let encoded = serde_json::to_string_pretty(&nodes)
        .map_err(|error| format!("could not encode source shard {prefix}: {error}"))?;
    let bytes = encoded.len() + 1;
    if bytes <= SOURCE_SHARD_TARGET_BYTES || prefix.len() >= 16 {
        if bytes > SOURCE_SHARD_MAX_BYTES {
            return Err(format!(
                "source shard {prefix} is {} bytes, above the {} byte limit",
                bytes, SOURCE_SHARD_MAX_BYTES
            ));
        }
        let file_name = if prefix.len() == 1 {
            format!("{prefix}.json")
        } else {
            format!("{}/{prefix}.json", &prefix[..1])
        };
        shards.push(SourceShard {
            file_name,
            node_count: nodes.len(),
            bytes,
            encoded,
        });
        return Ok(());
    }

    let next_index = prefix.len();
    let mut children = BTreeMap::<char, Vec<Value>>::new();
    for node in nodes {
        let id = node
            .get("id")
            .and_then(Value::as_str)
            .ok_or_else(|| "source node is missing a string id".to_owned())?;
        let hash = id
            .strip_prefix("source-")
            .ok_or_else(|| format!("source node id {id:?} has no hash prefix"))?;
        let next = hash
            .chars()
            .nth(next_index)
            .ok_or_else(|| format!("source node id {id:?} has a short hash"))?;
        children.entry(next).or_default().push(node);
    }
    for (next, child_nodes) in children {
        split_source_bucket(format!("{prefix}{next}"), child_nodes, shards)?;
    }
    Ok(())
}

fn source_segment(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

fn source_dataset_name(place_name: &str) -> String {
    let stem = Path::new(place_name)
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or(place_name);
    let sanitized = stem
        .chars()
        .map(|value| {
            if value.is_ascii_alphanumeric() || matches!(value, '-' | '_') {
                value
            } else {
                '_'
            }
        })
        .collect::<String>();
    if sanitized.is_empty() {
        "import".to_owned()
    } else {
        sanitized
    }
}

fn source_index_reference(scene_path: &Path, source_index: &Path) -> String {
    let base = scene_path.parent().unwrap_or_else(|| Path::new("."));
    let value = if source_index.is_absolute() {
        source_index
            .strip_prefix(base)
            .unwrap_or(source_index)
            .to_path_buf()
    } else {
        source_index.to_path_buf()
    };
    value.to_string_lossy().replace('\\', "/")
}

fn source_node_id(path: &str) -> String {
    format!("source-{}", short_hash(path))
}

fn short_hash(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))[..16].to_owned()
}

fn write_text(path: &Path, content: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("could not create {}: {error}", parent.display()))?;
    }
    fs::write(path, content).map_err(|error| format!("could not write {}: {error}", path.display()))
}

fn export_reference_mesh_command(args: &[String]) -> Result<(), String> {
    let mut scene = None;
    let mut output = None;
    let mut path_prefixes = Vec::new();
    let mut exclude_paths = Vec::new();
    let mut scale = 1.0;
    let mut origin = [0.0; 3];
    let mut collision_output = None;
    let mut bounds_output = None;
    let mut mesh_overrides = None;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--scene" => {
                index += 1;
                scene = Some(PathBuf::from(required_arg(args, index, "--scene")?));
            }
            "--output" => {
                index += 1;
                output = Some(PathBuf::from(required_arg(args, index, "--output")?));
            }
            "--path-prefix" => {
                index += 1;
                path_prefixes.push(required_arg(args, index, "--path-prefix")?.to_owned());
            }
            "--exclude-path" => {
                index += 1;
                exclude_paths.push(required_arg(args, index, "--exclude-path")?.to_owned());
            }
            "--scale" => {
                index += 1;
                scale = required_arg(args, index, "--scale")?
                    .parse::<f32>()
                    .map_err(|_| "--scale must be a positive number".to_owned())?;
            }
            "--origin" => {
                index += 1;
                origin = args
                    .get(index)
                    .map(String::as_str)
                    .ok_or_else(|| "--origin requires a value".to_owned())?
                    .split(',')
                    .map(|value| {
                        value.trim().parse::<f32>().map_err(|_| {
                            "--origin must be three comma-separated numbers".to_owned()
                        })
                    })
                    .collect::<Result<Vec<_>, _>>()?
                    .try_into()
                    .map_err(|_| "--origin must be three comma-separated numbers".to_owned())?;
            }
            "--collision-output" => {
                index += 1;
                collision_output = Some(PathBuf::from(required_arg(
                    args,
                    index,
                    "--collision-output",
                )?));
            }
            "--bounds-output" => {
                index += 1;
                bounds_output = Some(PathBuf::from(required_arg(args, index, "--bounds-output")?));
            }
            "--mesh-overrides" => {
                index += 1;
                mesh_overrides = Some(PathBuf::from(required_arg(
                    args,
                    index,
                    "--mesh-overrides",
                )?));
            }
            value => return Err(format!("unknown export-reference-mesh option {value}")),
        }
        index += 1;
    }
    let scene = scene.ok_or_else(|| "export-reference-mesh requires --scene".to_owned())?;
    let output = output.ok_or_else(|| "export-reference-mesh requires --output".to_owned())?;
    let result = export_reference_mesh(&MeshExportOptions {
        scene_path: scene,
        output_path: output,
        path_prefixes,
        exclude_paths,
        scale,
        origin,
        collision_output,
        bounds_output,
        mesh_overrides,
    })?;
    println!(
        "Exported reference mesh: {} geometry, {} triangles, {} vertices -> {}",
        result.geometry_count,
        result.triangle_count,
        result.vertex_count,
        result.output.display()
    );
    println!(
        "  local bounds: [{:.3}, {:.3}, {:.3}]",
        result.bounds.maximum[0] - result.bounds.minimum[0],
        result.bounds.maximum[1] - result.bounds.minimum[1],
        result.bounds.maximum[2] - result.bounds.minimum[2]
    );
    Ok(())
}

fn import_roblox_reference_command(args: &[String]) -> Result<(), String> {
    let mut place = None;
    let mut terrain = None;
    let mut project = None;
    let mut output = None;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--place" => {
                index += 1;
                place = Some(PathBuf::from(required_arg(args, index, "--place")?));
            }
            "--terrain" => {
                index += 1;
                terrain = Some(PathBuf::from(required_arg(args, index, "--terrain")?));
            }
            "--project" => {
                index += 1;
                project = Some(PathBuf::from(required_arg(args, index, "--project")?));
            }
            "--output" => {
                index += 1;
                output = Some(PathBuf::from(required_arg(args, index, "--output")?));
            }
            value => return Err(format!("unknown import-roblox-reference option {value}")),
        }
        index += 1;
    }
    let place = place.ok_or_else(|| "import-roblox-reference requires --place".to_owned())?;
    let output = output.ok_or_else(|| "import-roblox-reference requires --output".to_owned())?;
    let result = import_reference(&ImportOptions {
        place_path: place,
        terrain_path: terrain,
        project_path: project,
        output_path: output,
    })?;
    println!(
        "Imported Roblox reference: {} geometry ({} visible), {} cameras, {} lights, {} textures, {} text nodes -> {}",
        result.geometry_count,
        result.visible_geometry_count,
        result.camera_count,
        result.light_count,
        result.texture_count,
        result.text_count,
        result.output.display()
    );
    if result.has_terrain_payload {
        println!(
            "Recorded terrain payload metadata; voxel decoding remains a separate renderer/import step"
        );
    }
    Ok(())
}

fn build_command(args: &[String]) -> Result<(), String> {
    let mut project = PathBuf::from(".");
    let mut source = None;
    let mut manifest = PathBuf::from("manifest.json");
    let mut output = None;
    let mut zip = None;
    let mut positional_seen = false;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--source" => {
                index += 1;
                source = Some(PathBuf::from(required_arg(args, index, "--source")?));
            }
            "--manifest" => {
                index += 1;
                manifest = PathBuf::from(required_arg(args, index, "--manifest")?);
            }
            "--output" => {
                index += 1;
                output = Some(PathBuf::from(required_arg(args, index, "--output")?));
            }
            "--zip" => {
                index += 1;
                zip = Some(PathBuf::from(required_arg(args, index, "--zip")?));
            }
            value if value.starts_with('-') => {
                return Err(format!("unknown build-game option {value}"));
            }
            value if !positional_seen => {
                project = PathBuf::from(value);
                positional_seen = true;
            }
            value => return Err(format!("unexpected build-game argument {value}")),
        }
        index += 1;
    }
    let project = project
        .canonicalize()
        .map_err(|error| format!("could not resolve project {}: {error}", project.display()))?;
    let source_root = source
        .map(|source| {
            if source.is_absolute() {
                source
            } else {
                project.join(source)
            }
        })
        .unwrap_or_else(|| project.join("src"));
    let source_root = if !source_root.join("main.luau").is_file()
        && source_root.join("src/main.luau").is_file()
    {
        source_root.join("src")
    } else {
        source_root
    };
    let manifest_path = if manifest.is_absolute() {
        manifest
    } else if project.join(&manifest).is_file() {
        project.join(manifest)
    } else {
        source_root.parent().unwrap_or(&project).join(manifest)
    };
    let output = output.unwrap_or_else(|| project.join("build/package"));
    let result = build_game(&BuildOptions {
        source_root,
        manifest_path,
        output,
        zip_path: zip,
    })
    .map_err(|error| error.to_string())?;
    let version = result
        .version
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| result.version.to_string());
    println!(
        "Built {} v{} -> {}",
        result.game_id,
        version,
        result.output.display()
    );
    if let Some(zip) = result.zip_path {
        println!("Wrote package archive -> {}", zip.display());
    }
    Ok(())
}

fn create_command(args: &[String]) -> Result<(), String> {
    let mut title = None;
    let mut path = None;
    let mut vendor_sdk = false;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--title" => {
                index += 1;
                title = Some(required_arg(args, index, "--title")?.to_owned());
            }
            "--path" => {
                index += 1;
                path = Some(PathBuf::from(required_arg(args, index, "--path")?));
            }
            "--vendor-sdk" => vendor_sdk = true,
            value => return Err(format!("unknown create-game option {value}")),
        }
        index += 1;
    }
    let title = title.ok_or_else(|| "create-game requires --title".to_owned())?;
    let path = path.unwrap_or_else(|| PathBuf::from("."));
    let result = create_game(&title, &path, vendor_sdk)?;
    println!(
        "Created {} ({}) -> {}",
        result.display_name,
        result.game_id,
        result.project.display()
    );
    Ok(())
}

fn required_arg<'a>(args: &'a [String], index: usize, option: &str) -> Result<&'a str, String> {
    args.get(index)
        .map(String::as_str)
        .filter(|value| !value.starts_with('-'))
        .ok_or_else(|| format!("{option} requires a value"))
}

fn print_help() {
    println!(
        "Cubacadabra creator tools\n\nCommands:\n  build-game                Build a portable game package\n  create-game               Create a starter project\n  import-roblox-reference   Extract a deterministic static reference scene from Roblox XML\n  import-roblox-scene       Generate a native scene tree and sharded source hierarchy index\n  migrate-source-index      Migrate a v1 source hierarchy into sharded JSON\n  export-reference-mesh     Bake a reference-scene hierarchy into a package GLB\n\nExamples:\n  cubacadabra build-game ../first-game\n  cubacadabra build-game --source ../first-game --output /tmp/first-game\n  cubacadabra create-game --title \"My Game\" --path ~/games\n  cubacadabra import-roblox-reference --place Place.rbxmx --terrain PlaceTerrain.rbxmx --project default.project.json --output /tmp/reference-scene.json\n  cubacadabra import-roblox-scene --reference /tmp/reference-scene.json --base-scene scene.json --output scene.json --source-index imports/roblox/place/index.json\n  cubacadabra migrate-source-index --input source-hierarchy.json --output imports/roblox/place/index.json\n  cubacadabra export-reference-mesh --scene /tmp/reference-scene.json --output assets/models/reference.glb --path-prefix 'Folder:Place[1]/Folder:Main[1]/Model:MainIsland[1]'"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_records_store_parent_ids_and_local_segments() {
        let nodes = vec![
            json!({
                "id": "source-0123456789abcdef",
                "parent": Value::Null,
                "sourceSegment": "Workspace:Workspace[1]",
                "class": "Workspace",
                "name": "Workspace",
                "geometryCount": 0,
            }),
            json!({
                "id": "source-abcdef0123456789",
                "parent": "source-0123456789abcdef",
                "sourceSegment": "Model:Casino[1]",
                "class": "Model",
                "name": "Casino",
                "geometryCount": 0,
            }),
        ];
        let shards = shard_source_nodes(&nodes).unwrap();
        assert_eq!(shards.len(), 2);
        assert!(
            shards
                .iter()
                .all(|shard| shard.bytes <= SOURCE_SHARD_MAX_BYTES)
        );
        assert!(
            shards
                .iter()
                .all(|shard| !shard.encoded.contains("parentPath"))
        );
        assert!(
            shards
                .iter()
                .all(|shard| !shard.encoded.contains("Workspace:Workspace[1]/Model"))
        );
    }

    #[test]
    fn source_dataset_name_is_stable_and_safe_for_paths() {
        assert_eq!(source_dataset_name("vegas.rbxlx"), "vegas");
        assert_eq!(
            source_dataset_name("Vegas Place (copy).rbxlx"),
            "Vegas_Place__copy_"
        );
        assert_eq!(source_dataset_name(""), "import");
    }

    #[test]
    fn source_index_reference_is_relative_to_scene() {
        assert_eq!(
            source_index_reference(
                Path::new("/tmp/project/scene.json"),
                Path::new("/tmp/project/imports/roblox/vegas/index.json"),
            ),
            "imports/roblox/vegas/index.json"
        );
        assert_eq!(
            source_index_reference(
                Path::new("scene.json"),
                Path::new("imports/roblox/vegas/index.json"),
            ),
            "imports/roblox/vegas/index.json"
        );
    }
}
