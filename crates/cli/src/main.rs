use cubacadabra_builder::{
    AuthoringNode, AuthoringScene, BuildOptions, EditorMetadata, SourceMetadata, Transform,
    build_game, parse_authoring_scene, serialize_authoring_scene,
};
use cubacadabra_project::create_game;
use cubacadabra_reference_import::{
    ImportOptions, MeshExportOptions, ReferenceScene, export_reference_mesh, import_reference,
    read_reference_scene,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
    path::{Path, PathBuf},
    process::ExitCode,
};

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
        "export-reference-mesh" => export_reference_mesh_command(&args[1..]),
        command => Err(format!("unknown command {command:?}; use --help")),
    }
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
            .join("source-hierarchy.json")
    });
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
    );
    scene.nodes.extend(generated_ids);
    scene.validate()?;
    let scene_source = serialize_authoring_scene(&scene)?;
    write_text(&output, &scene_source)?;
    write_source_index(&source_index, &reference_scene)?;
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
    println!("  complete source index -> {}", source_index.display());
    Ok(())
}

fn generated_scene_nodes(
    reference: &ReferenceScene,
    root_id: &str,
    parent_id: Option<&str>,
    tree_depth: usize,
    focus_paths: &[String],
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
                ("sourceIndex".to_owned(), json!("source-hierarchy.json")),
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

fn write_source_index(path: &Path, reference: &ReferenceScene) -> Result<(), String> {
    let mut geometry_by_path = BTreeMap::<&str, usize>::new();
    for geometry in &reference.geometry {
        *geometry_by_path.entry(geometry.path.as_str()).or_default() += 1;
    }
    let nodes = reference
        .instances
        .iter()
        .map(|instance| {
            json!({
                "id": source_node_id(&instance.path),
                "path": instance.path,
                "parentPath": instance.parent_path,
                "class": instance.class,
                "name": instance.name,
                "transform": instance.transform,
                "geometryCount": geometry_by_path.get(instance.path.as_str()).copied().unwrap_or(0),
            })
        })
        .collect::<Vec<_>>();
    let index = json!({
        "formatVersion": 1,
        "kind": "cubacadabra-roblox-source-hierarchy",
        "source": reference.source,
        "instanceCount": reference.instances.len(),
        "geometryCount": reference.geometry.len(),
        "nodes": nodes,
    });
    let encoded = serde_json::to_string_pretty(&index)
        .map_err(|error| format!("could not encode source hierarchy: {error}"))?;
    write_text(path, &format!("{encoded}\n"))
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
        "Cubacadabra creator tools\n\nCommands:\n  build-game                Build a portable game package\n  create-game               Create a starter project\n  import-roblox-reference   Extract a deterministic static reference scene from Roblox XML\n  import-roblox-scene       Generate a native scene tree and complete source hierarchy index\n  export-reference-mesh     Bake a reference-scene hierarchy into a package GLB\n\nExamples:\n  cubacadabra build-game ../first-game\n  cubacadabra build-game --source ../first-game --output /tmp/first-game\n  cubacadabra create-game --title \"My Game\" --path ~/games\n  cubacadabra import-roblox-reference --place Place.rbxmx --terrain PlaceTerrain.rbxmx --project default.project.json --output /tmp/reference-scene.json\n  cubacadabra import-roblox-scene --reference /tmp/reference-scene.json --base-scene scene.json --output scene.json --source-index source-hierarchy.json\n  cubacadabra export-reference-mesh --scene /tmp/reference-scene.json --output assets/models/reference.glb --path-prefix 'Folder:Place[1]/Folder:Main[1]/Model:MainIsland[1]'"
    );
}
