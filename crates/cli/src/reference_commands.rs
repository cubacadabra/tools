use super::*;

pub(crate) fn export_reference_mesh_command(args: &[String]) -> Result<(), String> {
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
