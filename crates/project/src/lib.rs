//! Project creation primitives shared by Studio and the command-line tool.

use serde_json::json;
use std::{
    fs,
    path::{Path, PathBuf},
};
use unicode_normalization::UnicodeNormalization;

const VERSION: &str = "0.3.0";
const SDK_FILES: &[(&str, &str, &str)] = &[
    (
        "cycle.luau",
        include_str!("../../../src/cubacadabra/sdk/cycle.luau"),
        "cycle",
    ),
    (
        "disclosure.luau",
        include_str!("../../../src/cubacadabra/sdk/disclosure.luau"),
        "disclosure",
    ),
    (
        "obby.luau",
        include_str!("../../../src/cubacadabra/sdk/obby.luau"),
        "obby",
    ),
    (
        "shared-state.luau",
        include_str!("../../../src/cubacadabra/sdk/shared-state.luau"),
        "shared-state",
    ),
    (
        "survival.luau",
        include_str!("../../../src/cubacadabra/sdk/survival.luau"),
        "survival",
    ),
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateResult {
    pub game_id: String,
    pub display_name: String,
    pub project: PathBuf,
}

pub fn create_game(title: &str, parent: &Path, vendor_sdk: bool) -> Result<CreateResult, String> {
    let title = title.trim();
    if title.is_empty() {
        return Err("title is required".to_owned());
    }
    let game_id = game_id(title)?;
    let parent = if parent.exists() {
        parent.canonicalize()
    } else {
        fs::create_dir_all(parent).and_then(|_| parent.canonicalize())
    }
    .map_err(|error| {
        format!(
            "could not use the parent directory {}: {error}",
            parent.display()
        )
    })?;
    let project = parent.join(&game_id);
    if project.exists() {
        return Err(format!(
            "game directory already exists: {}",
            project.display()
        ));
    }
    fs::create_dir(&project).map_err(|error| {
        format!(
            "could not create game directory {}: {error}",
            project.display()
        )
    })?;
    let result = write_project(&project, title, &game_id, vendor_sdk);
    if let Err(error) = result {
        let _ = fs::remove_dir_all(&project);
        return Err(error);
    }
    Ok(CreateResult {
        game_id,
        display_name: title.to_owned(),
        project,
    })
}

pub fn game_id(title: &str) -> Result<String, String> {
    let normalized: String = title
        .nfkd()
        .filter(|character| character.is_ascii())
        .collect();
    let mut result = String::new();
    for character in normalized.to_ascii_lowercase().chars() {
        if character.is_ascii_alphanumeric() {
            result.push(character);
        } else if !result.ends_with('-') {
            result.push('-');
        }
    }
    let result = result.trim_matches('-').to_owned();
    if result.is_empty() {
        return Err("title must contain at least one letter or number".to_owned());
    }
    if !(3..=64).contains(&result.len()) {
        return Err("title must produce a game id between 3 and 64 characters".to_owned());
    }
    Ok(result)
}

fn write_project(
    project: &Path,
    title: &str,
    game_id: &str,
    vendor_sdk: bool,
) -> Result<(), String> {
    fs::create_dir(project.join("src"))
        .map_err(|error| format!("could not create source directory: {error}"))?;
    fs::create_dir_all(project.join("assets/audio"))
        .map_err(|error| format!("could not create audio asset directory: {error}"))?;
    fs::create_dir_all(project.join("assets/images"))
        .map_err(|error| format!("could not create image asset directory: {error}"))?;
    if vendor_sdk {
        let sdk = project.join(".cubacadabra/sdk");
        fs::create_dir_all(&sdk)
            .map_err(|error| format!("could not create SDK directory: {error}"))?;
        for (name, source, _) in SDK_FILES {
            fs::write(sdk.join(name), source)
                .map_err(|error| format!("could not write SDK module {name}: {error}"))?;
        }
        fs::write(
            project.join(".luaurc"),
            "{\n  \"aliases\": {\n    \"cubacadabra\": \".cubacadabra/sdk\"\n  }\n}\n",
        )
        .map_err(|error| format!("could not write .luaurc: {error}"))?;
    }
    fs::write(
        project.join("manifest.json"),
        format!(
            "{}\n",
            serde_json::to_string_pretty(&manifest(title, game_id)).unwrap()
        ),
    )
    .map_err(|error| format!("could not write manifest.json: {error}"))?;
    fs::write(
        project.join("scene.json"),
        format!(
            "{}\n",
            serde_json::to_string_pretty(&starter_scene()).unwrap()
        ),
    )
    .map_err(|error| format!("could not write scene.json: {error}"))?;
    fs::write(project.join("src/main.luau"), source(title, game_id))
        .map_err(|error| format!("could not write src/main.luau: {error}"))?;
    Ok(())
}

fn starter_scene() -> serde_json::Value {
    json!({
        "formatVersion": 1,
        "worldId": "starter-world",
        "nodes": [{
            "id": "world-starter-world",
            "name": "Starter World",
            "transform": {
                "position": [0, 0, 0],
                "rotation": [0, 0, 0],
                "scale": [1, 1, 1]
            },
            "components": {},
            "editor": { "visible": true, "locked": false }
        }]
    })
}

fn manifest(title: &str, game_id: &str) -> serde_json::Value {
    json!({
        "id": game_id, "version": VERSION, "sdkVersion": VERSION,
        "package": {"formatVersion": 3, "entry": "game.luau"},
        "displayName": title, "lobby": false, "startWorld": "lobby",
        "launch": {"destinationWorld": "starter-world", "authoritative": true},
        "scene": {"eyebrow": "cubacadabra", "title": title, "description": "A new Cubacadabra game.", "maxPlayers": 18},
        "palette": {"sky": "#151A3F", "ground": "#20295D", "groundEdge": "#080D26", "grid": "#3B4C86", "signal": "#57E5D0", "hot": "#FF5B85", "coral": "#FF8A3D", "butter": "#F1E95B", "periwinkle": "#9F7BFF", "ink": "#0B102B", "paper": "#F7F5E9"},
        "avatars": {"player": {"skin": "#E8AE86", "shirt": "#57E5D0", "pants": "#4C3F91", "shoes": "#0B102B"}, "npcs": []},
        "world": {"groundSize": 70, "gridSize": 64, "gridDivisions": 32, "spawn": [0, 0, 23], "showSpawnPad": false},
        "launchPads": [], "blocks": [], "worlds": {"starter-world": {"blocks": [], "signs": [], "interactions": []}}
    })
}

fn source(title: &str, game_id: &str) -> String {
    let title = serde_json::to_string(title).unwrap();
    let game_id = serde_json::to_string(game_id).unwrap();
    format!(
        "-- Welcome to Cubacadabra. Add your game rules and UI here.\nlocal Game = {{}}\n\nfunction Game.on_start(api)\n    api.lobby:set_enabled(false)\n    api.lobby:set_status({title} .. \" is ready\")\n    api.session:start({game_id}, {{ mode = \"preview\" }})\nend\n\nreturn Game\n"
    )
}
