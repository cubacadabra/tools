use super::*;

pub(crate) fn build_command(args: &[String]) -> Result<(), String> {
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

pub(crate) fn create_command(args: &[String]) -> Result<(), String> {
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
