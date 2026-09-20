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

mod bundle;
mod collision;
mod maze;
mod package;
mod validation;
use bundle::*;
use cubacadabra_scene::parse_authoring_scene;
use package::*;
use validation::*;

const PREVIEW_SDK_VERSION: &str = "0.3.0";
const TERRAIN_SDK_VERSION: &str = "0.4.0";
const LEGACY_CURRENT_SDK_VERSION: &str = "0.5.0";
const CURRENT_SDK_VERSION: &str = "0.6.0";
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
    if let Some(scene_path) = manifest_path
        .parent()
        .map(|parent| parent.join("scene.json"))
        && scene_path.is_file()
    {
        let source =
            fs::read_to_string(&scene_path).map_err(io_error("could not read scene.json"))?;
        let scene = parse_authoring_scene(&source).map_err(BuildError)?;
        scene
            .compile_into_manifest(&mut manifest)
            .map_err(BuildError)?;
    }
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
        if sdk_version != PREVIEW_SDK_VERSION
            && sdk_version != TERRAIN_SDK_VERSION
            && sdk_version != LEGACY_CURRENT_SDK_VERSION
            && sdk_version != CURRENT_SDK_VERSION
        {
            return Err(BuildError(format!(
                "manifest.sdkVersion {sdk_version:?} is unsupported; this builder supports {PREVIEW_SDK_VERSION}, {TERRAIN_SDK_VERSION}, {LEGACY_CURRENT_SDK_VERSION}, and {CURRENT_SDK_VERSION}"
            )));
        }
    }
    maze::expand_manifest_mazes(manifest)?;
    validate_sdk_features(manifest, sdk_version.as_deref())?;
    resolve_effects_source(manifest, manifest_path.parent().unwrap_or(Path::new(".")))?;
    collision::resolve_manifest_sources(
        manifest,
        manifest_path.parent().unwrap_or(Path::new(".")),
    )?;
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
