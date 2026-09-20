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

mod import_scene;
mod project_commands;
mod reference_commands;
mod source_index;

use import_scene::*;
use project_commands::*;
use reference_commands::*;
use source_index::*;

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
pub(crate) fn required_arg<'a>(
    args: &'a [String],
    index: usize,
    option: &str,
) -> Result<&'a str, String> {
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
