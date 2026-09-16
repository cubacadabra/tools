use cubacadabra_builder::{BuildOptions, build_game};
use cubacadabra_project::create_game;
use std::{env, path::PathBuf, process::ExitCode};

fn main() -> ExitCode {
    match run(env::args().skip(1).collect()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => { eprintln!("cubacadabra: {error}"); ExitCode::from(1) }
    }
}

fn run(args: Vec<String>) -> Result<(), String> {
    if args.is_empty() || args.iter().any(|arg| arg == "--help" || arg == "-h") { print_help(); return Ok(()); }
    if args.len() == 1 && args[0] == "--version" { println!("cubacadabra 0.4.0"); return Ok(()); }
    match args[0].as_str() {
        "build-game" => build_command(&args[1..]),
        "create-game" => create_command(&args[1..]),
        "--create-game" => create_command(&args[1..]),
        command => Err(format!("unknown command {command:?}; use --help")),
    }
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
            "--source" => { index += 1; source = Some(PathBuf::from(required_arg(args, index, "--source")?)); }
            "--manifest" => { index += 1; manifest = PathBuf::from(required_arg(args, index, "--manifest")?); }
            "--output" => { index += 1; output = Some(PathBuf::from(required_arg(args, index, "--output")?)); }
            "--zip" => { index += 1; zip = Some(PathBuf::from(required_arg(args, index, "--zip")?)); }
            value if value.starts_with('-') => return Err(format!("unknown build-game option {value}")),
            value if !positional_seen => { project = PathBuf::from(value); positional_seen = true; }
            value => return Err(format!("unexpected build-game argument {value}")),
        }
        index += 1;
    }
    let project = project.canonicalize().map_err(|error| format!("could not resolve project {}: {error}", project.display()))?;
    let source_root = source
        .map(|source| if source.is_absolute() { source } else { project.join(source) })
        .unwrap_or_else(|| project.join("src"));
    let source_root = if !source_root.join("main.luau").is_file() && source_root.join("src/main.luau").is_file() {
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
    let result = build_game(&BuildOptions { source_root, manifest_path, output, zip_path: zip }).map_err(|error| error.to_string())?;
    let version = result.version.as_str().map(str::to_owned).unwrap_or_else(|| result.version.to_string());
    println!("Built {} v{} -> {}", result.game_id, version, result.output.display());
    if let Some(zip) = result.zip_path { println!("Wrote package archive -> {}", zip.display()); }
    Ok(())
}

fn create_command(args: &[String]) -> Result<(), String> {
    let mut title = None;
    let mut path = None;
    let mut vendor_sdk = false;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--title" => { index += 1; title = Some(required_arg(args, index, "--title")?.to_owned()); }
            "--path" => { index += 1; path = Some(PathBuf::from(required_arg(args, index, "--path")?)); }
            "--vendor-sdk" => vendor_sdk = true,
            value => return Err(format!("unknown create-game option {value}")),
        }
        index += 1;
    }
    let title = title.ok_or_else(|| "create-game requires --title".to_owned())?;
    let path = path.unwrap_or_else(|| PathBuf::from("."));
    let result = create_game(&title, &path, vendor_sdk)?;
    println!("Created {} ({}) -> {}", result.display_name, result.game_id, result.project.display());
    Ok(())
}

fn required_arg<'a>(args: &'a [String], index: usize, option: &str) -> Result<&'a str, String> { args.get(index).map(String::as_str).filter(|value| !value.starts_with('-')).ok_or_else(|| format!("{option} requires a value")) }

fn print_help() {
    println!("Cubacadabra creator tools\n\nCommands:\n  build-game   Build a portable game package\n  create-game  Create a starter project\n\nExamples:\n  cubacadabra build-game ../first-game\n  cubacadabra build-game --source ../first-game --output /tmp/first-game\n  cubacadabra create-game --title \"My Game\" --path ~/games");
}
