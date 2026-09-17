//! The canonical Cubacadabra source-project builder.
//!
//! This crate deliberately owns package semantics rather than the command-line
//! parser. Studio links it directly, while the `cubacadabra` binary is only a
//! thin frontend around the same implementation.

use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    io::{self, Read, Write},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

mod maze;

const PREVIEW_SDK_VERSION: &str = "0.3.0";
const TERRAIN_SDK_VERSION: &str = "0.4.0";
const BUILD_MARKER: &str = ".cubacadabra-build";
const BUILD_MARKER_CONTENT: &str = "cubacadabra-game-package-v1\n";
const MAX_AUTHORITY_SOURCE_BYTES: usize = 1024 * 1024;
const MAX_IMAGE_ASSET_BYTES: u64 = 8 * 1024 * 1024;
const MAX_AUDIO_ASSET_BYTES: u64 = 4 * 1024 * 1024;
const MAX_MODEL_ASSET_BYTES: u64 = 16 * 1024 * 1024;

const SDK_MODULES: &[(&str, &str, &str)] = &[
    (
        "@cubacadabra/cycle",
        "cycle.luau",
        include_str!("../../../src/cubacadabra/sdk/cycle.luau"),
    ),
    (
        "@cubacadabra/disclosure",
        "disclosure.luau",
        include_str!("../../../src/cubacadabra/sdk/disclosure.luau"),
    ),
    (
        "@cubacadabra/obby",
        "obby.luau",
        include_str!("../../../src/cubacadabra/sdk/obby.luau"),
    ),
    (
        "@cubacadabra/shared-state",
        "shared-state.luau",
        include_str!("../../../src/cubacadabra/sdk/shared-state.luau"),
    ),
    (
        "@cubacadabra/survival",
        "survival.luau",
        include_str!("../../../src/cubacadabra/sdk/survival.luau"),
    ),
];

#[derive(Debug, Clone)]
pub struct BuildOptions {
    pub source_root: PathBuf,
    pub manifest_path: PathBuf,
    pub output: PathBuf,
    pub zip_path: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildResult {
    pub game_id: String,
    pub version: Value,
    pub output: PathBuf,
    pub zip_path: Option<PathBuf>,
}

#[derive(Debug)]
pub struct BuildError(pub String);

impl std::fmt::Display for BuildError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for BuildError {}

type Result<T> = std::result::Result<T, BuildError>;

#[derive(Clone)]
struct Module {
    source: String,
    routes: BTreeMap<String, String>,
}

pub fn build_game(options: &BuildOptions) -> Result<BuildResult> {
    let source_root = absolute_path(&options.source_root)?;
    let manifest_path = absolute_path(&options.manifest_path)?;
    let output = absolute_path(&options.output)?;
    let zip_path = options
        .zip_path
        .as_ref()
        .map(|path| absolute_path(path))
        .transpose()?;

    if !source_root.is_dir() {
        return Err(BuildError(format!(
            "source directory not found: {}",
            source_root.display()
        )));
    }
    let entry = source_root.join("main.luau");
    if !entry.is_file() {
        return Err(BuildError(format!(
            "Luau entry point not found: {}",
            entry.display()
        )));
    }
    if !manifest_path.is_file() {
        return Err(BuildError(format!(
            "manifest not found: {}",
            manifest_path.display()
        )));
    }
    validate_output(&output, &source_root)?;

    let mut manifest = read_json(&manifest_path)?;
    let manifest = manifest
        .as_object_mut()
        .ok_or_else(|| BuildError("manifest must contain a JSON object".to_owned()))?;
    let game_id = required_string(manifest, "id")?.to_owned();
    if !valid_game_id(&game_id) {
        return Err(BuildError(
            "manifest.id must use lowercase letters, numbers, and single dashes".to_owned(),
        ));
    }
    let version = manifest
        .get("version")
        .cloned()
        .ok_or_else(|| BuildError("manifest.version must be present".to_owned()))?;
    validate_version(&version, "manifest.version")?;
    if !manifest.contains_key("package") {
        manifest.insert(
            "package".to_owned(),
            json!({"formatVersion": 3, "entry": "game.luau"}),
        );
    }
    if manifest
        .get("package")
        .and_then(Value::as_object)
        .and_then(|package| package.get("entry"))
        .and_then(Value::as_str)
        != Some("game.luau")
    {
        return Err(BuildError(
            "manifest.package.entry must be 'game.luau'".to_owned(),
        ));
    }
    if let Some(display_name) = manifest.get("displayName") {
        if display_name
            .as_str()
            .is_none_or(|value| value.trim().is_empty() || value.trim().len() > 120)
        {
            return Err(BuildError(
                "manifest.displayName must be a non-empty string of at most 120 characters"
                    .to_owned(),
            ));
        }
    } else {
        manifest.insert("displayName".to_owned(), Value::String(game_id.clone()));
    }
    let sdk_version = manifest
        .get("sdkVersion")
        .and_then(Value::as_str)
        .map(str::to_owned);
    if let Some(sdk_version) = &sdk_version {
        validate_version(
            &Value::String(sdk_version.to_owned()),
            "manifest.sdkVersion",
        )?;
        if sdk_version != PREVIEW_SDK_VERSION && sdk_version != TERRAIN_SDK_VERSION {
            return Err(BuildError(format!(
                "manifest.sdkVersion {sdk_version:?} is unsupported; this builder supports {PREVIEW_SDK_VERSION} and {TERRAIN_SDK_VERSION}"
            )));
        }
    }
    maze::expand_manifest_mazes(manifest)?;
    resolve_effects_source(manifest, manifest_path.parent().unwrap_or(Path::new(".")))?;
    validate_assets(manifest, manifest_path.parent().unwrap_or(Path::new(".")))?;

    let authority_source = source_root.join("server.luau");
    let authority_entry = manifest
        .get("package")
        .and_then(Value::as_object)
        .and_then(|package| package.get("authorityEntry"))
        .and_then(Value::as_str);
    if authority_entry.is_some_and(|entry| entry != "authority.luau") {
        return Err(BuildError(
            "manifest.package.authorityEntry must be 'authority.luau'".to_owned(),
        ));
    }
    if authority_source.is_file() {
        let package = manifest
            .get_mut("package")
            .and_then(Value::as_object_mut)
            .ok_or_else(|| BuildError("manifest.package must be an object".to_owned()))?;
        package.insert(
            "authorityEntry".to_owned(),
            Value::String("authority.luau".to_owned()),
        );
    } else if authority_entry.is_some() {
        return Err(BuildError(
            "manifest.package.authorityEntry requires src/server.luau".to_owned(),
        ));
    }
    let package_format_version = manifest
        .get("package")
        .and_then(Value::as_object)
        .and_then(|package| package.get("formatVersion"))
        .and_then(Value::as_i64)
        .unwrap_or(3);

    let zip_path = zip_path
        .map(|path| {
            if path.starts_with(&output) {
                Err(BuildError(
                    "zip path cannot be inside the package output directory".to_owned(),
                ))
            } else if path.starts_with(&source_root) {
                Err(BuildError(
                    "zip path cannot be inside the source directory".to_owned(),
                ))
            } else {
                Ok(path)
            }
        })
        .transpose()?;
    if let Some(zip_path) = &zip_path {
        if zip_path.is_dir() {
            return Err(BuildError(format!(
                "zip path is a directory: {}",
                zip_path.display()
            )));
        }
        fs::create_dir_all(zip_path.parent().unwrap_or(Path::new(".")))
            .map_err(io_error("could not create archive parent"))?;
    }
    validate_existing_output(&output)?;
    fs::create_dir_all(output.parent().unwrap_or(Path::new(".")))
        .map_err(io_error("could not create package parent"))?;
    let staging = unique_sibling(&output, "staging")?;
    let archive_temp = zip_path
        .as_ref()
        .map(|path| unique_file_path(path, "archive"));
    let archive_temp = archive_temp.transpose()?;

    let result = (|| {
        let mut dependencies = Map::new();
        let generated = format!(
            "-- GENERATED FILE: do not edit; edit src/ and run cubacadabra build-game.\n-- game: {game_id}\n-- version: {}\n\n{}\n",
            json_scalar(&version),
            bundle_modules(&entry, &source_root, &mut dependencies)?,
        );
        write_text(&staging.join("game.luau"), &generated)?;

        if authority_source.is_file() {
            let mut authority_dependencies = Map::new();
            let authority = format!(
                "-- GENERATED FILE: do not edit; edit src/server.luau and run cubacadabra build-game.\n-- game: {game_id}\n-- version: {}\n\n{}\n",
                json_scalar(&version),
                bundle_modules(&authority_source, &source_root, &mut authority_dependencies)?,
            );
            if authority.len() > MAX_AUTHORITY_SOURCE_BYTES {
                return Err(BuildError(
                    "bundled trusted authority rules exceed the 1 MiB limit".to_owned(),
                ));
            }
            if !authority_dependencies.is_empty() {
                return Err(BuildError(
                    "trusted authority rules cannot require client Cubacadabra SDK modules"
                        .to_owned(),
                ));
            }
            write_text(&staging.join("authority.luau"), &authority)?;
        }
        let rendered_manifest = pretty_json(&Value::Object(manifest.clone()))?;
        write_text(&staging.join("manifest.json"), &rendered_manifest)?;
        copy_tree_if_present(
            &manifest_path.parent().unwrap().join("assets"),
            &staging.join("assets"),
        )?;

        let payload_files = files_under(&staging)?;
        let hashes = payload_files
            .iter()
            .map(|name| {
                Ok((
                    name.clone(),
                    Value::String(sha256_file(&staging.join(name))?),
                ))
            })
            .collect::<Result<Map<String, Value>>>()?;
        let mut package_info = json!({
            "formatVersion": package_format_version,
            "id": game_id,
            "version": version,
            "runtime": {"api": sdk_version.as_deref().unwrap_or(PREVIEW_SDK_VERSION)},
            "dependencies": dependencies,
            "dependencyPolicy": "canonical-toolchain",
            "entry": "game.luau",
            "manifest": "manifest.json",
            "files": payload_files,
            "sha256": hashes,
        });
        if authority_source.is_file() {
            package_info["authorityEntry"] = Value::String("authority.luau".to_owned());
        }
        let rendered_package = pretty_json(&package_info)?;
        write_text(&staging.join("package.json"), &rendered_package)?;
        write_text(&staging.join(BUILD_MARKER), BUILD_MARKER_CONTENT)?;

        if let (Some(zip_path), Some(archive_temp)) = (&zip_path, &archive_temp) {
            write_zip(&staging, archive_temp)?;
            let _ = zip_path;
        }
        install_staging(&staging, &output)?;
        if let (Some(zip_path), Some(archive_temp)) = (&zip_path, &archive_temp) {
            fs::rename(archive_temp, zip_path)
                .map_err(io_error("could not install package archive"))?;
        }
        Ok(())
    })();

    if result.is_err() {
        let _ = fs::remove_dir_all(&staging);
        if let Some(archive_temp) = &archive_temp {
            let _ = fs::remove_file(archive_temp);
        }
    }
    result?;
    Ok(BuildResult {
        game_id,
        version,
        output,
        zip_path,
    })
}

fn absolute_path(path: &Path) -> Result<PathBuf> {
    let candidate = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(io_error("could not resolve current directory"))?
            .join(path)
    };
    if candidate.exists() {
        return fs::canonicalize(candidate).map_err(io_error("could not resolve path"));
    }
    let mut missing = Vec::new();
    let mut existing = candidate.as_path();
    while !existing.exists() {
        let name = existing
            .file_name()
            .ok_or_else(|| BuildError(format!("could not resolve path: {}", path.display())))?;
        missing.push(name.to_owned());
        existing = existing
            .parent()
            .ok_or_else(|| BuildError(format!("could not resolve path: {}", path.display())))?;
    }
    let mut resolved = fs::canonicalize(existing).map_err(io_error("could not resolve path"))?;
    for component in missing.iter().rev() {
        resolved.push(component);
    }
    Ok(resolved)
}

fn required_string<'a>(object: &'a Map<String, Value>, key: &str) -> Result<&'a str> {
    object
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| BuildError(format!("manifest.{key} must be present and a string")))
}

fn valid_game_id(value: &str) -> bool {
    (3..=64).contains(&value.len())
        && value.chars().enumerate().all(|(index, c)| {
            c.is_ascii_lowercase()
                || c.is_ascii_digit()
                || (c == '-'
                    && index > 0
                    && index + 1 < value.len()
                    && !value[..index].ends_with('-'))
        })
}

fn validate_version(value: &Value, field: &str) -> Result<()> {
    if value.as_u64().is_some_and(|number| number >= 1) {
        return Ok(());
    }
    let Some(value) = value.as_str() else {
        return Err(BuildError(format!(
            "{field} must be SemVer or a legacy positive integer"
        )));
    };
    let core = value.split_once('+').map_or(value, |(core, _)| core);
    let core = core.split_once('-').map_or(core, |(core, _)| core);
    let parts: Vec<_> = core.split('.').collect();
    if parts.len() != 3
        || parts
            .iter()
            .any(|part| part.is_empty() || part.parse::<u64>().is_err())
    {
        return Err(BuildError(format!(
            "{field} must be SemVer or a legacy positive integer"
        )));
    }
    Ok(())
}

fn resolve_effects_source(manifest: &mut Map<String, Value>, project_root: &Path) -> Result<()> {
    let Some(effects) = manifest.get("effects").cloned() else {
        return Ok(());
    };
    let Some(object) = effects.as_object() else {
        return Ok(());
    };
    let Some(source) = object.get("source").and_then(Value::as_str) else {
        return Ok(());
    };
    if object.len() != 1
        || source.is_empty()
        || source.starts_with('/')
        || source.contains("..")
        || !source.ends_with(".json")
    {
        return Err(BuildError(
            "manifest.effects.source must be a relative JSON path inside the game project"
                .to_owned(),
        ));
    }
    let path = project_root.join(source);
    let value = read_json(&path)?;
    if !value.is_object() {
        return Err(BuildError(
            "manifest.effects.source must contain a JSON object".to_owned(),
        ));
    }
    manifest.insert("effects".to_owned(), value);
    Ok(())
}

fn validate_assets(manifest: &Map<String, Value>, project_root: &Path) -> Result<()> {
    let Some(assets) = manifest.get("assets").and_then(Value::as_object) else {
        return Ok(());
    };
    for (kind, max, extensions, max_bytes) in [
        (
            "images",
            16usize,
            &["png", "jpg", "jpeg"][..],
            MAX_IMAGE_ASSET_BYTES,
        ),
        ("audio", 64usize, &["wav"][..], MAX_AUDIO_ASSET_BYTES),
        (
            "models",
            64usize,
            &["glb", "gltf"][..],
            MAX_MODEL_ASSET_BYTES,
        ),
    ] {
        let Some(entries) = assets.get(kind) else {
            continue;
        };
        let Some(entries) = entries.as_object() else {
            return Err(BuildError(format!(
                "manifest.assets.{kind} must be an object"
            )));
        };
        if entries.len() > max {
            return Err(BuildError(format!(
                "manifest.assets.{kind} cannot contain more than {max} assets"
            )));
        }
        for (id, definition) in entries {
            if id.is_empty()
                || id.len() > 64
                || !id
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
            {
                return Err(BuildError(format!(
                    "manifest.assets.{kind} ids must use ASCII letters, numbers, dots, dashes, or underscores"
                )));
            }
            let object = definition.as_object().ok_or_else(|| {
                BuildError(format!("manifest.assets.{kind}.{id} must be an object"))
            })?;
            let path = object.get("path").and_then(Value::as_str).ok_or_else(|| {
                BuildError(format!("manifest.assets.{kind}.{id}.path must be a string"))
            })?;
            if path.starts_with('/')
                || path.contains("..")
                || !path.starts_with("assets/")
                || !extensions
                    .iter()
                    .any(|extension| path.to_ascii_lowercase().ends_with(extension))
            {
                return Err(BuildError(format!(
                    "manifest.assets.{kind}.{id}.path is invalid: {path}"
                )));
            }
            let full = project_root.join(path);
            let metadata = fs::metadata(&full).map_err(|_| {
                BuildError(format!(
                    "manifest.assets.{kind}.{id}.path was not found: {path}"
                ))
            })?;
            if metadata.len() > max_bytes {
                return Err(BuildError(format!(
                    "manifest.assets.{kind}.{id}.path exceeds the asset size limit: {path}"
                )));
            }
        }
    }
    Ok(())
}

fn read_json(path: &Path) -> Result<Value> {
    let source = fs::read_to_string(path).map_err(io_error("could not read JSON file"))?;
    serde_json::from_str(&source)
        .map_err(|error| BuildError(format!("invalid JSON in {}: {error}", path.display())))
}

fn pretty_json(value: &Value) -> Result<String> {
    serde_json::to_string_pretty(value)
        .map(|value| value + "\n")
        .map_err(|error| BuildError(format!("could not serialize JSON: {error}")))
}
fn json_scalar(value: &Value) -> String {
    value
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| value.to_string())
}
fn write_text(path: &Path, text: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(io_error("could not create output directory"))?;
    }
    fs::write(path, text).map_err(io_error("could not write output file"))
}
fn io_error(context: &'static str) -> impl FnOnce(io::Error) -> BuildError {
    move |error| BuildError(format!("{context}: {error}"))
}

fn validate_output(output: &Path, source_root: &Path) -> Result<()> {
    if output.starts_with(source_root) || source_root.starts_with(output) {
        return Err(BuildError(
            "output directory cannot overlap the source directory in either direction".to_owned(),
        ));
    }
    Ok(())
}

fn validate_existing_output(output: &Path) -> Result<()> {
    if !output.exists() {
        return Ok(());
    }
    if !output.is_dir() {
        return Err(BuildError(format!(
            "output exists and is not a directory: {}",
            output.display()
        )));
    }
    let marker = fs::read_to_string(output.join(BUILD_MARKER)).map_err(|_| {
        BuildError(format!(
            "refusing to replace existing output without a Cubacadabra build marker: {}",
            output.display()
        ))
    })?;
    if marker != BUILD_MARKER_CONTENT {
        return Err(BuildError(format!(
            "refusing to replace existing output with an invalid Cubacadabra build marker: {}",
            output.display()
        )));
    }
    Ok(())
}

fn unique_sibling(path: &Path, label: &str) -> Result<PathBuf> {
    let parent = path.parent().unwrap_or(Path::new("."));
    fs::create_dir_all(parent).map_err(io_error("could not create temporary directory parent"))?;
    for attempt in 0..100u32 {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default();
        let candidate = parent.join(format!(
            ".{}.{}-{}-{}",
            path.file_name().unwrap_or_default().to_string_lossy(),
            label,
            std::process::id(),
            suffix + attempt as u128
        ));
        if !candidate.exists() {
            fs::create_dir_all(&candidate)
                .map_err(io_error("could not create staging directory"))?;
            return Ok(candidate);
        }
    }
    Err(BuildError(format!(
        "could not create temporary sibling for {}",
        path.display()
    )))
}

fn unique_file_path(path: &Path, label: &str) -> Result<PathBuf> {
    let parent = path.parent().unwrap_or(Path::new("."));
    fs::create_dir_all(parent).map_err(io_error("could not create temporary file parent"))?;
    for attempt in 0..100u32 {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default();
        let candidate = parent.join(format!(
            ".{}.{}-{}-{}",
            path.file_name().unwrap_or_default().to_string_lossy(),
            label,
            std::process::id(),
            suffix + attempt as u128
        ));
        if !candidate.exists() {
            return Ok(candidate);
        }
    }
    Err(BuildError(format!(
        "could not create temporary file beside {}",
        path.display()
    )))
}

fn install_staging(staging: &Path, output: &Path) -> Result<()> {
    let backup = if output.exists() {
        Some(unique_sibling(output, "old")?)
    } else {
        None
    };
    if let Some(backup) = &backup {
        fs::remove_dir(backup).map_err(io_error("could not prepare output backup"))?;
        fs::rename(output, backup).map_err(io_error("could not retain previous package"))?;
    }
    if let Err(error) = fs::rename(staging, output) {
        if let Some(backup) = &backup {
            let _ = fs::rename(backup, output);
        }
        return Err(io_error("could not install package output")(error));
    }
    if let Some(backup) = backup {
        let _ = fs::remove_dir_all(backup);
    }
    Ok(())
}

fn copy_tree_if_present(source: &Path, destination: &Path) -> Result<()> {
    if !source.is_dir() {
        return Ok(());
    }
    fs::create_dir_all(destination).map_err(io_error("could not create asset directory"))?;
    for entry in fs::read_dir(source).map_err(io_error("could not read asset directory"))? {
        let entry = entry.map_err(io_error("could not inspect asset directory"))?;
        let name = entry.file_name();
        if name == ".DS_Store" {
            continue;
        }
        let source_path = entry.path();
        let destination_path = destination.join(name);
        if entry
            .file_type()
            .map_err(io_error("could not inspect asset type"))?
            .is_dir()
        {
            copy_tree_if_present(&source_path, &destination_path)?;
        } else if entry
            .file_type()
            .map_err(io_error("could not inspect asset type"))?
            .is_file()
        {
            fs::copy(&source_path, &destination_path).map_err(io_error("could not copy asset"))?;
        }
    }
    Ok(())
}

fn files_under(root: &Path) -> Result<Vec<String>> {
    let mut files = Vec::new();
    collect_files(root, root, &mut files)?;
    files.sort();
    Ok(files)
}
fn collect_files(root: &Path, current: &Path, files: &mut Vec<String>) -> Result<()> {
    for entry in fs::read_dir(current).map_err(io_error("could not enumerate package"))? {
        let entry = entry.map_err(io_error("could not inspect package"))?;
        let path = entry.path();
        if entry
            .file_type()
            .map_err(io_error("could not inspect package entry"))?
            .is_dir()
        {
            collect_files(root, &path, files)?;
        } else if path.file_name().and_then(|name| name.to_str()) != Some(BUILD_MARKER) {
            files.push(
                path.strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/"),
            );
        }
    }
    Ok(())
}

fn sha256_file(path: &Path) -> Result<String> {
    let mut file = fs::File::open(path).map_err(io_error("could not open package file"))?;
    let mut digest = Sha256::new();
    let mut buffer = [0u8; 1024 * 1024];
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(io_error("could not hash package file"))?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

fn write_zip(source: &Path, destination: &Path) -> Result<()> {
    let file =
        fs::File::create(destination).map_err(io_error("could not create package archive"))?;
    let mut archive = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    for name in files_under(source)? {
        archive
            .start_file(&name, options)
            .map_err(|error| BuildError(format!("could not write package archive: {error}")))?;
        let bytes =
            fs::read(source.join(&name)).map_err(io_error("could not read package file"))?;
        archive
            .write_all(&bytes)
            .map_err(io_error("could not write package archive"))?;
    }
    archive
        .finish()
        .map_err(|error| BuildError(format!("could not finish package archive: {error}")))?;
    Ok(())
}

fn sdk_source(module: &str) -> Option<&'static str> {
    SDK_MODULES
        .iter()
        .find(|(name, _, _)| *name == module)
        .map(|(_, _, source)| *source)
}
fn bundle_modules(
    entry: &Path,
    source_root: &Path,
    dependencies: &mut Map<String, Value>,
) -> Result<String> {
    let mut modules = BTreeMap::new();
    visit_local(
        entry,
        source_root,
        &mut modules,
        &mut Vec::new(),
        dependencies,
    )?;
    let entry_id = entry
        .strip_prefix(source_root)
        .map_err(|_| BuildError("entry point must be inside source directory".to_owned()))?
        .to_string_lossy()
        .replace('\\', "/");
    let mut output = vec![
        "local __modules = {}".to_owned(),
        "local __routes = {}".to_owned(),
        "local __cache = {}".to_owned(),
        "local __loading = {}".to_owned(),
        String::new(),
    ];
    for (module_id, module) in &modules {
        output.push(format!("-- begin module: {module_id}"));
        output.push(format!("__routes[{}] = {{", json_string(module_id)));
        for (specifier, dependency) in &module.routes {
            output.push(format!(
                "    [{}] = {},",
                json_string(specifier),
                json_string(dependency)
            ));
        }
        output.push("}".to_owned());
        output.push(format!(
            "__modules[{}] = function(require)",
            json_string(module_id)
        ));
        output.push(module.source.clone());
        output.push("end".to_owned());
        output.push(format!("-- end module: {module_id}"));
        output.push("".to_owned());
    }
    output.extend([
        "local function __require(module_id)".to_owned(),
        "    local cached = __cache[module_id]".to_owned(),
        "    if cached ~= nil then".to_owned(),
        "        return cached".to_owned(),
        "    end".to_owned(),
        "    if __loading[module_id] then".to_owned(),
        "        error(\"cyclic bundled require: \" .. module_id)".to_owned(),
        "    end".to_owned(),
        "    local loader = __modules[module_id]".to_owned(),
        "    if loader == nil then".to_owned(),
        "        error(\"bundled module not found: \" .. module_id)".to_owned(),
        "    end".to_owned(),
        "    local routes = __routes[module_id]".to_owned(),
        "    local function module_require(path)".to_owned(),
        "        local dependency_id = routes[path]".to_owned(),
        "        if dependency_id == nil then".to_owned(),
        "            error(\"undeclared bundled require from \" .. module_id .. \": \" .. tostring(path))".to_owned(),
        "        end".to_owned(),
        "        return __require(dependency_id)".to_owned(),
        "    end".to_owned(),
        "    __loading[module_id] = true".to_owned(),
        "    local result = loader(module_require)".to_owned(),
        "    __loading[module_id] = nil".to_owned(),
        "    if result == nil then".to_owned(),
        "        error(\"bundled module returned nil: \" .. module_id)".to_owned(),
        "    end".to_owned(),
        "    __cache[module_id] = result".to_owned(),
        "    return result".to_owned(),
        "end".to_owned(),
        "".to_owned(),
        format!("return __require({})", json_string(&entry_id)),
    ]);
    Ok(output.join("\n"))
}

fn visit_local(
    path: &Path,
    source_root: &Path,
    modules: &mut BTreeMap<String, Module>,
    stack: &mut Vec<String>,
    dependencies: &mut Map<String, Value>,
) -> Result<String> {
    let module_id = path
        .strip_prefix(source_root)
        .map_err(|_| BuildError("required module must stay inside src/".to_owned()))?
        .to_string_lossy()
        .replace('\\', "/");
    let source = fs::read_to_string(path).map_err(io_error("could not read Luau source"))?;
    visit(
        module_id,
        source,
        Some(path),
        source_root,
        modules,
        stack,
        dependencies,
    )
}

fn visit(
    module_id: String,
    source: String,
    path: Option<&Path>,
    source_root: &Path,
    modules: &mut BTreeMap<String, Module>,
    stack: &mut Vec<String>,
    dependencies: &mut Map<String, Value>,
) -> Result<String> {
    if stack.iter().any(|item| item == &module_id) {
        stack.push(module_id.clone());
        return Err(BuildError(format!(
            "cyclic Luau require: {}",
            stack.join(" -> ")
        )));
    }
    if modules.contains_key(&module_id) {
        return Ok(module_id);
    }
    for (line, value) in source.lines().enumerate() {
        if value.trim_start().starts_with("-- @include") {
            return Err(BuildError(format!(
                "{module_id}:{}: @include is no longer supported; use a Luau require()",
                line + 1
            )));
        }
    }
    stack.push(module_id.clone());
    let mut routes = BTreeMap::new();
    for specifier in require_specifiers(&source, &module_id)? {
        let dependency_id = if specifier.starts_with("@cubacadabra/") {
            let dependency_source = sdk_source(&specifier).ok_or_else(|| {
                BuildError(format!("unknown Cubacadabra SDK module: {specifier}"))
            })?;
            dependencies.insert(specifier.clone(), json!({"source": "cubacadabra-preview-sdk", "sha256": sha256_bytes(dependency_source.as_bytes())}));
            visit(
                specifier.clone(),
                dependency_source.to_owned(),
                None,
                source_root,
                modules,
                stack,
                dependencies,
            )?
        } else {
            let requiring_path = path.ok_or_else(|| {
                BuildError(format!(
                    "{module_id}: SDK modules cannot require game source modules"
                ))
            })?;
            let dependency =
                resolve_local_module(&specifier, requiring_path, source_root, &module_id)?;
            visit_local(&dependency, source_root, modules, stack, dependencies)?
        };
        routes.insert(specifier, dependency_id);
    }
    stack.pop();
    modules.insert(module_id.clone(), Module { source, routes });
    Ok(module_id)
}

fn resolve_local_module(
    specifier: &str,
    requiring_path: &Path,
    source_root: &Path,
    module_id: &str,
) -> Result<PathBuf> {
    if !(specifier.starts_with("./") || specifier.starts_with("../")) || specifier.contains('\\') {
        return Err(BuildError(format!(
            "{module_id}: require path must start with './', '../', or '@cubacadabra/' and use forward slashes"
        )));
    }
    let base = requiring_path.parent().unwrap_or(source_root);
    let mut clean = PathBuf::new();
    for component in Path::new(specifier).components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                if !clean.pop() {
                    return Err(BuildError(format!(
                        "{module_id}: require must stay inside src/"
                    )));
                }
            }
            std::path::Component::Normal(value) => clean.push(value),
            _ => return Err(BuildError(format!("{module_id}: invalid require path"))),
        }
    }
    let unresolved = base.join(clean);
    let candidates = if unresolved.extension().is_some() {
        vec![unresolved.clone()]
    } else {
        vec![
            unresolved.with_extension("luau"),
            unresolved.with_extension("lua"),
            unresolved.join("init.luau"),
            unresolved.join("init.lua"),
        ]
    };
    let matches: Vec<_> = candidates
        .into_iter()
        .filter(|candidate| candidate.is_file())
        .collect();
    match matches.as_slice() {
        [] => Err(BuildError(format!(
            "{module_id}: required module not found: {specifier}"
        ))),
        [match_path] => Ok(match_path.clone()),
        _ => Err(BuildError(format!(
            "{module_id}: required module is ambiguous: {specifier}"
        ))),
    }
}

fn require_specifiers(source: &str, module_id: &str) -> Result<Vec<String>> {
    let mut result = Vec::new();
    let bytes = source.as_bytes();
    let mut cursor = 0;
    let mut previous_token = String::new();
    while cursor < bytes.len() {
        if source[cursor..].starts_with("--") {
            cursor = skip_comment(source, cursor);
            continue;
        }
        let character = bytes[cursor] as char;
        if matches!(character, '\'' | '"' | '`') {
            cursor = skip_quoted(source, cursor, character);
            previous_token = "string".to_owned();
            continue;
        }
        if character.is_ascii_alphabetic() || character == '_' {
            let start = cursor;
            cursor += 1;
            while cursor < bytes.len()
                && ((bytes[cursor] as char).is_ascii_alphanumeric() || bytes[cursor] as char == '_')
            {
                cursor += 1;
            }
            let identifier = &source[start..cursor];
            if identifier != "require" || previous_token == "." || previous_token == ":" {
                previous_token = identifier.to_owned();
                continue;
            }
            let open = skip_space_comments(source, cursor);
            if open >= bytes.len() || bytes[open] as char != '(' {
                continue;
            }
            let argument = skip_space_comments(source, open + 1);
            if argument >= bytes.len() || !matches!(bytes[argument] as char, '\'' | '"') {
                return Err(BuildError(format!(
                    "{module_id}: require paths must be static quoted strings"
                )));
            }
            let quote = bytes[argument] as char;
            let mut end = argument + 1;
            while end < bytes.len() && bytes[end] as char != quote {
                if bytes[end] == b'\\' {
                    return Err(BuildError(format!(
                        "{module_id}: require paths cannot contain escapes"
                    )));
                }
                end += 1;
            }
            if end >= bytes.len() {
                return Err(BuildError(format!(
                    "{module_id}: unterminated require path"
                )));
            }
            let close = skip_space_comments(source, end + 1);
            if close >= bytes.len() || bytes[close] as char != ')' {
                return Err(BuildError(format!(
                    "{module_id}: require must contain exactly one string path"
                )));
            }
            let specifier = source[argument + 1..end].to_owned();
            if !result.contains(&specifier) {
                result.push(specifier);
            }
            previous_token = ")".to_owned();
            cursor = close + 1;
        } else {
            if !character.is_ascii_whitespace() {
                previous_token = character.to_string();
            }
            cursor += 1;
        }
    }
    Ok(result)
}

fn skip_space_comments(source: &str, mut cursor: usize) -> usize {
    while cursor < source.len() {
        if source.as_bytes()[cursor].is_ascii_whitespace() {
            cursor += 1;
        } else if source[cursor..].starts_with("--") {
            cursor = skip_comment(source, cursor);
        } else {
            break;
        }
    }
    cursor
}
fn skip_quoted(source: &str, mut cursor: usize, quote: char) -> usize {
    cursor += 1;
    while cursor < source.len() {
        if source.as_bytes()[cursor] == b'\\' {
            cursor = (cursor + 2).min(source.len());
        } else if source.as_bytes()[cursor] as char == quote {
            return cursor + 1;
        } else {
            cursor += 1;
        }
    }
    source.len()
}
fn skip_comment(source: &str, mut cursor: usize) -> usize {
    cursor += 2;
    while cursor < source.len() && source.as_bytes()[cursor] != b'\n' {
        cursor += 1;
    }
    cursor
}
fn json_string(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "\"\"".to_owned())
}
fn sha256_bytes(bytes: &[u8]) -> String {
    let mut digest = Sha256::new();
    digest.update(bytes);
    format!("{:x}", digest.finalize())
}
