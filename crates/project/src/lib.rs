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

/// The complete source graph for the built-in new-game project.
///
/// Keeping these sources together lets creator hosts use the same starter
/// content for both a newly-created project and an in-process preview.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StarterGameSources {
    pub manifest: String,
    pub scene: String,
    pub main_luau: String,
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

pub fn starter_game_sources(title: &str, game_id: &str) -> StarterGameSources {
    StarterGameSources {
        manifest: format!(
            "{}\n",
            serde_json::to_string_pretty(&manifest(title, game_id)).unwrap()
        ),
        scene: format!(
            "{}\n",
            serde_json::to_string_pretty(&starter_scene()).unwrap()
        ),
        main_luau: source(title, game_id),
    }
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
    let sources = starter_game_sources(title, game_id);
    fs::write(project.join("manifest.json"), sources.manifest)
        .map_err(|error| format!("could not write manifest.json: {error}"))?;
    fs::write(project.join("scene.json"), sources.scene)
        .map_err(|error| format!("could not write scene.json: {error}"))?;
    fs::write(project.join("src/main.luau"), sources.main_luau)
        .map_err(|error| format!("could not write src/main.luau: {error}"))?;
    Ok(())
}

fn starter_scene() -> serde_json::Value {
    json!({
        "formatVersion": 1,
        "worldId": "starter-world",
        "nodes": starter_scene_nodes()
    })
}

const STARTER_CUBE_SIZE: f32 = 2.0;
const STARTER_CUBE_SPACING: f32 = STARTER_CUBE_SIZE;
const STARTER_WALL_Z: f32 = -4.0;
const STARTER_FACE_Z: f32 = 1.03;
const STARTER_STROKE_DEPTH: f32 = 0.06;
const STARTER_STROKE_THICKNESS: f32 = 0.08;
const STARTER_CUBE_COLOR: &str = "#F7F5E9";
const STARTER_STROKE_COLOR: &str = "#0B102B";

#[derive(Clone, Copy)]
struct StarterStroke {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    rotation: f32,
}

struct StarterLine {
    cube_index: usize,
    stroke_index: usize,
    stroke: StarterStroke,
    loose_position: [f32; 3],
}

const fn stroke(x: f32, y: f32, width: f32, height: f32) -> StarterStroke {
    StarterStroke {
        x,
        y,
        width,
        height,
        rotation: 0.0,
    }
}

const fn rotated_stroke(x: f32, y: f32, width: f32, height: f32, rotation: f32) -> StarterStroke {
    StarterStroke {
        x,
        y,
        width,
        height,
        rotation,
    }
}

// Each tile is one character of "CUBACADABRA". The marks intentionally use
// the edges of the square as part of the glyph, matching the supplied block
// lettering reference.
const STARTER_GLYPHS: &[&[StarterStroke]] = &[
    &[stroke(0.5, 0.0, 1.0, STARTER_STROKE_THICKNESS)],
    &[stroke(0.15, 0.55, STARTER_STROKE_THICKNESS, 0.9)],
    &[
        stroke(0.0, 0.4, 0.3, STARTER_STROKE_THICKNESS),
        stroke(0.0, -0.4, 0.3, STARTER_STROKE_THICKNESS),
        stroke(0.85, 0.0, 0.3, STARTER_STROKE_THICKNESS),
    ],
    &[
        stroke(0.0, 0.4, STARTER_STROKE_THICKNESS, 0.65),
        stroke(0.0, -0.75, STARTER_STROKE_THICKNESS, 0.5),
    ],
    &[stroke(0.5, 0.0, 1.0, STARTER_STROKE_THICKNESS)],
    &[
        stroke(0.0, 0.4, STARTER_STROKE_THICKNESS, 0.65),
        stroke(0.0, -0.75, STARTER_STROKE_THICKNESS, 0.5),
    ],
    &[stroke(0.0, 0.0, STARTER_STROKE_THICKNESS, 0.85)],
    &[
        stroke(0.0, 0.4, STARTER_STROKE_THICKNESS, 0.65),
        stroke(0.0, -0.75, STARTER_STROKE_THICKNESS, 0.5),
    ],
    &[
        stroke(0.0, 0.4, 0.3, STARTER_STROKE_THICKNESS),
        stroke(0.0, -0.4, 0.3, STARTER_STROKE_THICKNESS),
        stroke(0.85, 0.0, 0.3, STARTER_STROKE_THICKNESS),
    ],
    &[
        stroke(0.0, 0.4, 0.3, STARTER_STROKE_THICKNESS),
        stroke(0.85, 0.0, 0.3, STARTER_STROKE_THICKNESS),
        // Roblox-style thin Parts can rotate freely in the plane. This is
        // one exact diagonal stroke instead of a stair-step approximation.
        rotated_stroke(0.06, -0.62, 0.08, 0.78, 0.34),
    ],
    &[
        stroke(0.0, 0.4, STARTER_STROKE_THICKNESS, 0.65),
        stroke(0.0, -0.75, STARTER_STROKE_THICKNESS, 0.5),
    ],
];

fn starter_scene_nodes() -> Vec<serde_json::Value> {
    let mut nodes = vec![json!({
        "id": "world-starter-world",
        "name": "Starter World",
        "transform": {
            "position": [0, 0, 0],
            "rotation": [0, 0, 0],
            "scale": [1, 1, 1]
        },
        "components": {},
        "editor": { "visible": true, "locked": false }
    })];

    for (cube_index, _glyph) in STARTER_GLYPHS.iter().enumerate() {
        let cube_number = cube_index + 1;
        let cube_id = format!("starter-cube-{cube_number}");
        let cube_x = (cube_index as f32 - 5.0) * STARTER_CUBE_SPACING;
        nodes.push(json!({
            "id": cube_id,
            "parentId": "world-starter-world",
            "name": format!("Letter Cube {cube_number}"),
            "transform": {
                "position": [cube_x, STARTER_CUBE_SIZE / 2.0, STARTER_WALL_Z],
                "rotation": [0, 0, 0],
                "scale": [1, 1, 1]
            },
            "components": {
                "primitive": {
                    "shape": "box",
                    "size": [STARTER_CUBE_SIZE, STARTER_CUBE_SIZE, STARTER_CUBE_SIZE],
                    "color": STARTER_CUBE_COLOR,
                    "outline": false
                }
            },
            "editor": { "visible": true, "locked": false }
        }));

        // Keep the tile boundary as a crisp, dark front-face square. The
        // runtime block outline is intentionally translucent and wraps every
        // 3D edge, while the starter lettering reference uses a clear square
        // around each tile.
        let border = [
            (
                "Border Top",
                0.0,
                STARTER_CUBE_SIZE / 2.0 - STARTER_STROKE_THICKNESS / 2.0,
                STARTER_CUBE_SIZE,
                STARTER_STROKE_THICKNESS,
            ),
            (
                "Border Bottom",
                0.0,
                -(STARTER_CUBE_SIZE / 2.0 - STARTER_STROKE_THICKNESS / 2.0),
                STARTER_CUBE_SIZE,
                STARTER_STROKE_THICKNESS,
            ),
            (
                "Border Left",
                -(STARTER_CUBE_SIZE / 2.0 - STARTER_STROKE_THICKNESS / 2.0),
                0.0,
                STARTER_STROKE_THICKNESS,
                STARTER_CUBE_SIZE,
            ),
            (
                "Border Right",
                STARTER_CUBE_SIZE / 2.0 - STARTER_STROKE_THICKNESS / 2.0,
                0.0,
                STARTER_STROKE_THICKNESS,
                STARTER_CUBE_SIZE,
            ),
        ];
        for (border_index, (name, x, y, width, height)) in border.iter().copied().enumerate() {
            // Adjacent tiles share a separator. Keep the first tile's left
            // edge, then let each tile's right edge draw the shared line once.
            if border_index == 2 && cube_index > 0 {
                continue;
            }
            nodes.push(json!({
                "id": format!("{cube_id}-border-{}", border_index + 1),
                "parentId": cube_id,
                "name": format!("Letter {cube_number} {name}"),
                "transform": {
                    "position": [x, y, STARTER_FACE_Z],
                    "rotation": [0, 0, 0],
                    "scale": [1, 1, 1]
                },
                "components": {
                    "primitive": {
                        "shape": "box",
                        "size": [width, height, STARTER_STROKE_DEPTH],
                        "color": STARTER_STROKE_COLOR,
                        "collidable": false,
                        "castShadow": false,
                        "outline": false
                    }
                },
                "editor": { "visible": true, "locked": false }
            }));
        }
    }

    for line in starter_lines() {
        let line_number = starter_line_number(&line);
        nodes.push(json!({
            "id": format!("loose-line-{line_number}"),
            "parentId": "world-starter-world",
            "name": format!("Loose Line {line_number}"),
            "transform": {
                "position": line.loose_position,
                "rotation": [0, 0, 0],
                "scale": [1, 1, 1]
            },
            "components": {
                "interaction": {
                    "id": format!("loose-line-{line_number}"),
                    "kind": "pickup",
                    // Keep the interaction geometry visible while suppressing
                    // the runtime's fallback label derived from the scene name.
                    "label": " ",
                    "radius": 0.9,
                    "color": STARTER_STROKE_COLOR,
                    "visual": format!("letter-line-{line_number}")
                }
            },
            "editor": { "visible": true, "locked": false }
        }));
    }

    nodes
}

fn starter_lines() -> Vec<StarterLine> {
    STARTER_GLYPHS
        .iter()
        .enumerate()
        .flat_map(|(cube_index, glyph)| {
            glyph
                .iter()
                .copied()
                .enumerate()
                .map(move |(stroke_index, stroke)| {
                    let line_index = STARTER_GLYPHS[..cube_index]
                        .iter()
                        .map(|glyph| glyph.len())
                        .sum::<usize>()
                        + stroke_index;
                    let column = line_index % 7;
                    let row = line_index / 7;
                    StarterLine {
                        cube_index,
                        stroke_index,
                        stroke,
                        loose_position: [(column as f32 - 3.0) * 2.8, 0.0, 13.0 - row as f32 * 3.0],
                    }
                })
        })
        .collect()
}

fn starter_line_number(line: &StarterLine) -> usize {
    STARTER_GLYPHS[..line.cube_index]
        .iter()
        .map(|glyph| glyph.len())
        .sum::<usize>()
        + line.stroke_index
        + 1
}

fn starter_target_position(line: &StarterLine) -> [f32; 3] {
    [
        (line.cube_index as f32 - 5.0) * STARTER_CUBE_SPACING + line.stroke.x,
        STARTER_CUBE_SIZE / 2.0 + line.stroke.y,
        STARTER_WALL_Z + STARTER_FACE_Z,
    ]
}

fn starter_effects() -> serde_json::Value {
    let mut templates = serde_json::Map::new();
    for line in starter_lines() {
        let number = starter_line_number(&line);
        let target = starter_target_position(&line);
        let delta = [
            target[0] - line.loose_position[0],
            target[1] - line.loose_position[1] - 0.04,
            target[2] - line.loose_position[2],
        ];
        let stroke = line.stroke;
        let target_size = [stroke.width, stroke.height, STARTER_STROKE_DEPTH];
        let loose_size = [stroke.width, STARTER_STROKE_DEPTH, stroke.height];
        templates.insert(
            format!("letter-line-{number}"),
            json!({
                "duration": 1.15,
                "nodes": [
                    {
                        "shape": "box",
                        "position": [0, 0.04, 0],
                        "size": loose_size,
                        "color": STARTER_STROKE_COLOR,
                        "visibleStates": ["available"],
                        "rotation": [1.5707963, 0, stroke.rotation]
                    },
                    {
                        "shape": "box",
                        "position": delta,
                        "size": target_size,
                        "color": STARTER_STROKE_COLOR,
                        "visibleStates": ["complete"],
                        "rotation": [0, 0, stroke.rotation]
                    },
                    {
                        "shape": "box",
                        "position": [0, 0.04, 0],
                        "size": loose_size,
                        "color": STARTER_STROKE_COLOR,
                        "visibleStates": ["default"],
                        "rotation": [1.5707963, 0, stroke.rotation],
                        "animation": {
                            "travelTo": delta,
                            "travelSize": target_size,
                            "travelRotation": [0, 0, stroke.rotation],
                            "fade": true
                        }
                    }
                ]
            }),
        );
    }
    templates.insert(
        "line-settle".to_owned(),
        json!({
            "duration": 0.55,
            "nodes": [
                {
                    "shape": "ring",
                    "position": [0, 0.08, 0],
                    "size": [0.55, 0.08, 1],
                    "color": "signal",
                    "animation": {"expandAmount": 2.2, "fade": true}
                },
                {
                    "shape": "sphere",
                    "position": [0, 0.3, 0],
                    "size": [0.08, 1, 1],
                    "color": "butter",
                    "count": 4,
                    "animation": {"orbitRadius": 0.4, "orbitSpeed": 4.0, "radialAmount": 0.8, "fade": true}
                }
            ]
        }),
    );
    json!({ "version": 1, "templates": templates })
}

fn starter_target_positions_lua() -> String {
    let positions = starter_lines()
        .iter()
        .map(|line| {
            let position = starter_target_position(line);
            format!(
                "{{ {:.3}, {:.3}, {:.3} }}",
                position[0], position[1], position[2]
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!("{{ {positions} }}")
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
        "effects": starter_effects(),
        "launchPads": [], "blocks": [], "worlds": {"starter-world": {"blocks": [], "signs": [], "interactions": []}}
    })
}

fn source(title: &str, game_id: &str) -> String {
    let title = serde_json::to_string(title).unwrap();
    let game_id = serde_json::to_string(game_id).unwrap();
    let target_positions = starter_target_positions_lua();
    let line_count = starter_lines().len();
    format!(
        r#"-- Welcome to Cubacadabra. Restore every loose letter line to the wall.
local Game = {{}}

local line_count = {line_count}
local completed = {{}}
local moving = {{}}
local target_positions = {target_positions}

local function effect_id(index)
    return "letter-line-" .. index
end

local function interaction_id(index)
    return "loose-line-" .. index
end

local function completed_count()
    local count = 0
    for index = 1, line_count do
        if completed[index] then
            count += 1
        end
    end
    return count
end

local function update_status(api)
    local count = completed_count()
    if count == line_count then
        api.lobby:set_status("All {line_count} lines are back on the cubes!")
    elseif count == 0 then
        api.lobby:set_status({title} .. " — walk over a dark line.")
    else
        api.lobby:set_status(tostring(count) .. "/" .. line_count .. " lines restored — find another dark line.")
    end
end

function Game.on_start(api)
    api.lobby:set_enabled(false)
    api.session:start({game_id}, {{ mode = "preview" }})
    for index = 1, line_count do
        api.effects:set_state(interaction_id(index), "available")
    end
    update_status(api)
end

function Game.on_interaction(api, event)
    if event.phase ~= "enter" then
        return
    end
    local index = tonumber(string.match(event.id, "^loose%-line%-(%d+)$"))
    if not index or completed[index] or moving[index] then
        return
    end
    moving[index] = true
    api.effects:set_state(interaction_id(index), "carried")
    api.effects:play(effect_id(index), {{ position = event.position }})
    api.lobby:set_status("Line " .. index .. " is snapping into place…")
    api.task:delay(1.15, function()
        moving[index] = nil
        completed[index] = true
        api.effects:set_state(interaction_id(index), "complete")
        api.effects:play("line-settle", {{ position = target_positions[index] }})
        update_status(api)
    end)
end

return Game
"#
    )
}
