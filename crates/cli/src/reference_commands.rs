use super::*;

pub(crate) fn export_reference_mesh_command(args: &[String]) -> Result<(), String> {
    let mut scene = None;
    let mut output = None;
    let mut path_prefixes = Vec::new();
    let mut exclude_paths = Vec::new();
    let mut exclude_authoring_scene = None;
    let mut instance_root = None;
    let mut local_space = false;
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
            "--exclude-authoring-scene" => {
                index += 1;
                exclude_authoring_scene = Some(PathBuf::from(required_arg(
                    args,
                    index,
                    "--exclude-authoring-scene",
                )?));
            }
            "--instance-root" => {
                index += 1;
                instance_root = Some(required_arg(args, index, "--instance-root")?.to_owned());
            }
            "--local-space" => {
                local_space = true;
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
    let exclude_exact_paths = exclude_authoring_scene
        .map(|path| {
            let source = fs::read_to_string(&path).map_err(|error| {
                format!("could not read authoring scene {}: {error}", path.display())
            })?;
            let scene = parse_authoring_scene(&source)?;
            Ok::<Vec<String>, String>(
                scene
                    .nodes
                    .iter()
                    .filter(|node| {
                        node.source
                            .as_ref()
                            .and_then(|source| source.properties.get("representation"))
                            .and_then(Value::as_str)
                            == Some("primitive")
                    })
                    .filter_map(|node| node.source.as_ref()?.path.clone())
                    .collect::<Vec<_>>(),
            )
        })
        .transpose()?
        .unwrap_or_default();
    let result = export_reference_mesh(&MeshExportOptions {
        scene_path: scene,
        output_path: output,
        path_prefixes,
        exclude_paths,
        exclude_exact_paths,
        instance_root,
        local_space,
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

pub(crate) fn export_reference_instances_command(args: &[String]) -> Result<(), String> {
    let mut scene = None;
    let mut path_prefix = None;
    let mut instance_name = None;
    let mut asset_prefix = None;
    let mut asset_directory = None;
    let mut asset_path_prefix = "assets/models".to_owned();
    let mut mapping_output = None;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--scene" => {
                index += 1;
                scene = Some(PathBuf::from(required_arg(args, index, "--scene")?));
            }
            "--path-prefix" => {
                index += 1;
                path_prefix = Some(required_arg(args, index, "--path-prefix")?.to_owned());
            }
            "--instance-name" => {
                index += 1;
                instance_name = Some(required_arg(args, index, "--instance-name")?.to_owned());
            }
            "--asset-prefix" => {
                index += 1;
                asset_prefix = Some(required_arg(args, index, "--asset-prefix")?.to_owned());
            }
            "--asset-directory" => {
                index += 1;
                asset_directory = Some(PathBuf::from(required_arg(
                    args,
                    index,
                    "--asset-directory",
                )?));
            }
            "--asset-path-prefix" => {
                index += 1;
                asset_path_prefix = required_arg(args, index, "--asset-path-prefix")?.to_owned();
            }
            "--mapping-output" => {
                index += 1;
                mapping_output = Some(PathBuf::from(required_arg(
                    args,
                    index,
                    "--mapping-output",
                )?));
            }
            value => return Err(format!("unknown export-reference-instances option {value}")),
        }
        index += 1;
    }
    let scene_path =
        scene.ok_or_else(|| "export-reference-instances requires --scene".to_owned())?;
    let path_prefix = path_prefix
        .ok_or_else(|| "export-reference-instances requires --path-prefix".to_owned())?;
    let instance_name = instance_name
        .ok_or_else(|| "export-reference-instances requires --instance-name".to_owned())?;
    let asset_prefix = asset_prefix
        .ok_or_else(|| "export-reference-instances requires --asset-prefix".to_owned())?;
    let asset_directory = asset_directory
        .ok_or_else(|| "export-reference-instances requires --asset-directory".to_owned())?;
    let mapping_output = mapping_output
        .ok_or_else(|| "export-reference-instances requires --mapping-output".to_owned())?;
    let reference = read_reference_scene(&scene_path)?;
    let mut roots = reference
        .instances
        .iter()
        .filter(|instance| {
            instance.class == "Model"
                && instance.name == instance_name
                && instance.path.starts_with(&path_prefix)
        })
        .collect::<Vec<_>>();
    roots.sort_by(|left, right| left.path.cmp(&right.path));
    if roots.is_empty() {
        return Err(format!(
            "no Model instances named {instance_name:?} matched {path_prefix:?}"
        ));
    }
    fs::create_dir_all(&asset_directory).map_err(|error| {
        format!(
            "could not create reference asset directory {}: {error}",
            asset_directory.display()
        )
    })?;
    let mut asset_ids = BTreeMap::<String, String>::new();
    let mut assets = Vec::new();
    let mut instances = Vec::new();
    for root in roots {
        let fingerprint = reference_instance_fingerprint(&reference, &root.path)?;
        let asset_id = if let Some(asset_id) = asset_ids.get(&fingerprint) {
            asset_id.clone()
        } else {
            let asset_id = if asset_ids.is_empty() {
                asset_prefix.clone()
            } else {
                format!("{}-{}", asset_prefix, asset_ids.len() + 1)
            };
            let output_path = asset_directory.join(format!("{asset_id}.glb"));
            let collision_path = asset_directory.join(format!("{asset_id}.collision.json"));
            let result = export_reference_mesh(&MeshExportOptions {
                scene_path: scene_path.clone(),
                output_path,
                path_prefixes: vec![root.path.clone()],
                exclude_paths: Vec::new(),
                exclude_exact_paths: Vec::new(),
                instance_root: Some(root.path.clone()),
                local_space: true,
                scale: 1.0,
                origin: [0.0; 3],
                collision_output: Some(collision_path),
                bounds_output: None,
                mesh_overrides: None,
            })?;
            let bounds = [
                result.bounds.maximum[0] - result.bounds.minimum[0],
                result.bounds.maximum[1] - result.bounds.minimum[1],
                result.bounds.maximum[2] - result.bounds.minimum[2],
            ];
            assets.push(json!({
                "id": asset_id,
                "path": format!("{asset_path_prefix}/{asset_id}.glb"),
                "collision": format!("{asset_path_prefix}/{asset_id}.collision.json"),
                "bounds": bounds,
                "fingerprint": fingerprint,
            }));
            asset_ids.insert(fingerprint.clone(), asset_id.clone());
            asset_id
        };
        let frame = reference_instance_frame(&reference, &root.path)?;
        instances.push(json!({
            "sourcePath": root.path,
            "asset": asset_id,
            "position": frame.position,
            "rotation": [0.0, frame.rotation[0][2].atan2(frame.rotation[0][0]), 0.0],
            "scale": [1.0, 1.0, 1.0],
        }));
    }
    let asset_count = assets.len();
    let instance_count = instances.len();
    let mapping = json!({
        "formatVersion": 1,
        "instanceName": instance_name,
        "assets": assets,
        "instances": instances,
    });
    write_text(
        &mapping_output,
        &format!(
            "{}\n",
            serde_json::to_string_pretty(&mapping)
                .map_err(|error| format!("could not encode editable instance map: {error}"))?
        ),
    )?;
    println!(
        "Exported {} {} instances into {} reusable assets -> {}",
        instance_count,
        instance_name,
        asset_count,
        mapping_output.display()
    );
    Ok(())
}

pub(crate) fn import_roblox_reference_command(args: &[String]) -> Result<(), String> {
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
