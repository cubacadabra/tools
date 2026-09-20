//! Expansion of the bounded, authoring-friendly maze declaration.
//!
//! The native builder is the canonical package path.  Keep this expansion
//! here, rather than in the runtime, so packages contain ordinary terrain,
//! interaction, and checkpoint data that every host can consume.

use crate::{BuildError, Result};
use serde_json::{Map, Value, json};

const MAX_MAZE_WIDTH: usize = 20;
const MAX_MAZE_HEIGHT: usize = 20;

#[path = "maze_config.rs"]
mod config;
#[path = "maze_generation.rs"]
mod generation;
use config::*;
use generation::*;

pub(crate) fn expand_manifest_mazes(manifest: &mut Map<String, Value>) -> Result<()> {
    let mut generated_maze = false;
    if let Some(worlds) = manifest.get_mut("worlds") {
        let worlds = worlds
            .as_object_mut()
            .ok_or_else(|| BuildError("manifest.worlds must be an object".to_owned()))?;
        for world in worlds.values_mut() {
            if let Some(world) = world.as_object_mut() {
                generated_maze |= expand_world(world)?;
            }
        }
    }
    if manifest.get("maze").is_some() {
        generated_maze |= expand_world(manifest)?;
    }
    if generated_maze {
        let effects = maze_effects();
        match manifest.get_mut("effects") {
            None | Some(Value::Null) => {
                manifest.insert("effects".to_owned(), effects);
            }
            Some(Value::Object(existing)) if !existing.contains_key("source") => {
                let templates = existing
                    .entry("templates".to_owned())
                    .or_insert_with(|| Value::Object(Map::new()));
                if let Some(templates) = templates.as_object_mut() {
                    if let Some(generated) = effects.get("templates").and_then(Value::as_object) {
                        for (id, value) in generated {
                            templates.insert(id.clone(), value.clone());
                        }
                    }
                }
            }
            Some(_) => {}
        }
        let generated = manifest
            .entry("generated".to_owned())
            .or_insert_with(|| Value::Object(Map::new()));
        let generated = generated
            .as_object_mut()
            .ok_or_else(|| BuildError("manifest.generated must be an object".to_owned()))?;
        generated.insert("mazeFormat".to_owned(), json!(1));
    }
    Ok(())
}

fn expand_world(world: &mut Map<String, Value>) -> Result<bool> {
    let Some(config) = world.get("maze").cloned() else {
        return Ok(false);
    };
    let config = config
        .as_object()
        .ok_or_else(|| BuildError("manifest.worlds.<id>.maze must be an object".to_owned()))?;
    let width = integer(config, "width", 10, 2, MAX_MAZE_WIDTH)?;
    let height = integer(config, "height", 10, 2, MAX_MAZE_HEIGHT)?;
    let cell_size = number(config, "cellSize", 8.0, 3.0, 32.0)?;
    let wall_height = number(config, "wallHeight", 7.0, 2.0, 24.0)?;
    let wall_thickness = number(config, "wallThickness", 0.7, 0.2, 3.0)?;
    let seed = integer(config, "seed", 1, 0, u32::MAX as usize)? as u32;
    let start = cell(config, "start", (0, 0), width, height)?;
    let finish = cell(config, "finish", (width - 1, height - 1), width, height)?;
    let origin = origin(config, width, height, cell_size)?;
    let cells = carve(width, height, seed);
    let route = route(&cells, width, height, start, finish)?;

    let terrain_config = config
        .get("terrain")
        .cloned()
        .unwrap_or_else(|| Value::Object(Map::new()));
    let terrain_config = terrain_config
        .as_object()
        .ok_or_else(|| BuildError("manifest.maze.terrain must be an object".to_owned()))?;
    let terrain_cell_size = number(terrain_config, "cellSize", 0.5, 0.5, 8.0)?;
    if terrain_cell_size > wall_thickness {
        return Err(BuildError(format!(
            "maze.terrain.cellSize ({terrain_cell_size}) must not exceed maze.wallThickness ({wall_thickness})"
        )));
    }
    let wall_material = material(terrain_config, "wallMaterial", "grass")?;
    let floor_material = material(terrain_config, "floorMaterial", "ground")?;

    let blocks = array_clone(world, "blocks")?;
    let mut terrain = object_clone(world, "terrain")?;
    let operations = terrain
        .remove("operations")
        .unwrap_or_else(|| Value::Array(Vec::new()));
    let mut operations = operations
        .as_array()
        .ok_or_else(|| BuildError("world.terrain.operations must be an array".to_owned()))?
        .clone();
    let extent_x = width as f64 * cell_size;
    let extent_z = height as f64 * cell_size;
    // Keep the playable top at origin.y, but make the floor thicker than one
    // terrain sample interval so voxelization always captures an interior.
    let floor_thickness = terrain_cell_size * 2.0;
    // The maze owns its playable floor and walls. Authored environments belong
    // to the package that uses this generic expansion, so a maze declaration
    // does not silently inherit a particular island or art direction.
    operations.push(json!({
        "operation": "fill", "shape": "block",
        "position": [
            origin[0] + extent_x * 0.5,
            origin[1] - floor_thickness * 0.5,
            origin[2] + extent_z * 0.5
        ],
        "size": [extent_x, floor_thickness, extent_z],
        "material": floor_material
    }));
    // Joining exactly at an SDF zero plane leaves an air sheet between the
    // sampled solids. Overlap one sample into the floor, preserving wall tops.
    let wall_base = origin[1] - terrain_cell_size;
    let solid_wall_height = wall_height + terrain_cell_size;
    let wall_center_y = wall_base + solid_wall_height / 2.0;
    for y in 0..height {
        for x in 0..width {
            let cell = cells[y * width + x];
            let cell_x = origin[0] + x as f64 * cell_size;
            let cell_z = origin[2] + y as f64 * cell_size;
            if cell.north {
                operations.push(json!({
                    "operation": "fill", "shape": "block",
                    "position": [cell_x + cell_size / 2.0, wall_center_y, cell_z],
                    "size": [cell_size + wall_thickness, solid_wall_height, wall_thickness],
                    "material": wall_material
                }));
            }
            if cell.west {
                operations.push(json!({
                    "operation": "fill", "shape": "block",
                    "position": [cell_x, wall_center_y, cell_z + cell_size / 2.0],
                    "size": [wall_thickness, solid_wall_height, cell_size + wall_thickness],
                    "material": wall_material
                }));
            }
            if x == width - 1 && cell.east {
                operations.push(json!({
                    "operation": "fill", "shape": "block",
                    "position": [cell_x + cell_size, wall_center_y, cell_z + cell_size / 2.0],
                    "size": [wall_thickness, solid_wall_height, cell_size + wall_thickness],
                    "material": wall_material
                }));
            }
            if y == height - 1 && cell.south {
                operations.push(json!({
                    "operation": "fill", "shape": "block",
                    "position": [cell_x + cell_size / 2.0, wall_center_y, cell_z + cell_size],
                    "size": [cell_size + wall_thickness, solid_wall_height, wall_thickness],
                    "material": wall_material
                }));
            }
        }
    }
    terrain.insert("cellSize".to_owned(), json!(terrain_cell_size));
    terrain.insert("hideDefaultGround".to_owned(), json!(true));
    terrain.insert("operations".to_owned(), Value::Array(operations));

    let mut interactions = array_clone(world, "interactions")?;
    let mut decorations = array_clone(world, "decorations")?;
    let landmarks = config.get("landmarks").map_or(Ok(true), |value| {
        value
            .as_bool()
            .ok_or_else(|| BuildError("manifest.maze.landmarks must be a boolean".to_owned()))
    })?;
    if landmarks {
        decorations.push(json!({
            "id": "maze-start-gate", "kind": "gate",
            "position": position(origin, cell_size, start, 1.4),
            "scale": 1.0, "color": "signal"
        }));
        decorations.push(json!({
            "id": "maze-finish-gate", "kind": "finish",
            "position": position(origin, cell_size, finish, 1.4),
            "scale": 1.15, "color": "hot"
        }));
    }
    // Start and finish gates are generic maze landmarks. Package-owned
    // scenery is declared by the package rather than synthesized here.
    let finish_id = config
        .get("finishId")
        .and_then(Value::as_str)
        .unwrap_or("maze-finish");
    interactions.push(json!({
        "id": finish_id,
        "kind": "finish",
        "label": "EXIT",
        "position": position(origin, cell_size, finish, 0.0),
        "radius": (cell_size * 0.4).min(3.2),
        "color": config.get("finishColor").cloned().unwrap_or_else(|| json!("signal")),
        "visual": "maze-finish"
    }));

    let collectibles = config
        .get("collectibles")
        .cloned()
        .unwrap_or_else(|| Value::Object(Map::new()));
    let collectibles = collectibles
        .as_object()
        .ok_or_else(|| BuildError("manifest.maze.collectibles must be an object".to_owned()))?;
    let collectible_count = integer(collectibles, "count", 12, 0, 64)?;
    let mut collectible_cells = route[1..route.len().saturating_sub(1)].to_vec();
    let route_collectible_cells = collectible_cells.clone();
    collectible_cells.extend(
        (0..width * height)
            .filter(|index| {
                let cell = (index % width, index / width);
                cell != start && cell != finish && !route_collectible_cells.contains(&cell)
            })
            .map(|index| (index % width, index / width)),
    );
    let mut rng = PythonRandom::new(seed ^ 0xC0FF_EE);
    rng.shuffle(&mut collectible_cells);
    let collectible_color = collectibles
        .get("color")
        .cloned()
        .unwrap_or_else(|| json!("butter"));
    for (index, cell) in collectible_cells
        .into_iter()
        .take(collectible_count)
        .enumerate()
    {
        interactions.push(json!({
            "id": format!("maze-coin-{:02}", index + 1),
            "kind": "collectible",
            "label": "COIN",
            "position": position(origin, cell_size, cell, 0.0),
            "radius": (cell_size * 0.32).min(2.4),
            "color": collectible_color,
            "visual": "maze-coin"
        }));
    }

    let checkpoint_every = integer(config, "checkpointEvery", 0, 0, 64)?;
    let mut checkpoints = array_clone(world, "checkpoints")?;
    if checkpoint_every > 0 {
        for index in (checkpoint_every..route.len().saturating_sub(1)).step_by(checkpoint_every) {
            checkpoints.push(json!({
                "id": format!("maze-checkpoint-{}", index / checkpoint_every),
                "position": position(origin, cell_size, route[index], 0.0),
                "radius": (cell_size * 0.35).min(2.7)
            }));
        }
    }

    let mut settings = object_clone(world, "world")?;
    let extent = (width.max(height) as f64 * cell_size) + cell_size * 2.0;
    settings.insert(
        "groundSize".to_owned(),
        json!(
            settings
                .get("groundSize")
                .and_then(Value::as_f64)
                .unwrap_or(0.0)
                .max(extent)
        ),
    );
    settings.insert(
        "gridSize".to_owned(),
        json!(
            settings
                .get("gridSize")
                .and_then(Value::as_f64)
                .unwrap_or(0.0)
                .max(extent)
        ),
    );
    settings.insert("gridDivisions".to_owned(), json!(width.max(height) * 2));
    settings.insert("spawn".to_owned(), position(origin, cell_size, start, 1.5));
    settings.insert("showSpawnPad".to_owned(), json!(false));

    world.insert("blocks".to_owned(), Value::Array(blocks));
    world.insert("terrain".to_owned(), Value::Object(terrain));
    world.insert("interactions".to_owned(), Value::Array(interactions));
    world.insert("checkpoints".to_owned(), Value::Array(checkpoints));
    world.insert("decorations".to_owned(), Value::Array(decorations));
    world.insert("world".to_owned(), Value::Object(settings));
    world.remove("maze");
    Ok(true)
}

fn maze_effects() -> Value {
    json!({
        "version": 1,
        "templates": {
            "maze-coin": {
                "duration": 1,
                "nodes": [
                    {"shape": "cylinder", "position": [0, 0.5, 0], "size": [0.55, 0.12, 1], "color": "butter", "opacity": 0.9, "visibleStates": ["available"], "animation": {"orbitSpeed": 1.8, "bobAmount": 0.12, "bobSpeed": 2.2}},
                    {"shape": "ring", "position": [0, 0.18, 0], "size": [1.3, 0.08, 1], "color": "$interaction", "opacity": 0.55, "visibleStates": ["available", "collected"]}
                ]
            },
            "maze-finish": {
                "duration": 1,
                "nodes": [
                    {"shape": "ring", "position": [0, 0.12, 0], "size": [3.2, 0.16, 1], "color": "$interaction", "opacity": 0.85, "visibleStates": ["available", "reached"], "animation": {"pulseAmount": 0.08, "pulseSpeed": 2.5}},
                    {"shape": "box", "position": [0, 1.0, 0], "size": [0.24, 1.8, 0.24], "color": "$interaction", "opacity": 0.9, "visibleStates": ["available", "reached"], "count": 3, "animation": {"orbitRadius": 1.0, "orbitSpeed": 0.5, "bobAmount": 0.12}}
                ]
            },
            "maze-finish-burst": {
                "duration": 1.5,
                "nodes": [
                    {"shape": "ring", "position": [0, 0.55, 0], "size": [0.8, 0.12, 1], "color": "signal", "animation": {"expandAmount": 4.5, "fade": true}},
                    {"shape": "sphere", "position": [0, 1.0, 0], "size": [0.25, 1, 1], "color": "butter", "count": 12, "animation": {"orbitRadius": 1.4, "orbitSpeed": 3.5, "radialAmount": 4.8, "fade": true}}
                ]
            }
        }
    })
}

/// CPython's `random.Random` uses MT19937.  This small implementation keeps
/// native and legacy builders on the same seeded maze/collectible sequence.
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expands_maze_into_runtime_world_content() {
        let mut manifest = serde_json::from_value::<Map<String, Value>>(json!({
            "worlds": {"maze": {"maze": {"width": 4, "height": 4, "seed": 101, "collectibles": {"count": 3}, "checkpointEvery": 2}}}
        })).unwrap();
        expand_manifest_mazes(&mut manifest).unwrap();
        let world = manifest["worlds"]["maze"].as_object().unwrap();
        assert!(world.get("maze").is_none());
        assert!(world["terrain"]["operations"].as_array().unwrap().len() > 1);
        assert_eq!(world["interactions"].as_array().unwrap().len(), 4);
        assert_eq!(world["checkpoints"].as_array().unwrap().len(), 2);
        assert_eq!(manifest["generated"]["mazeFormat"], 1);
        assert!(manifest["effects"]["templates"]["maze-coin"].is_object());
    }

    #[test]
    fn decorative_landmarks_can_be_omitted_without_removing_gameplay() {
        let mut world = json!({"maze": {"width": 5, "height": 5, "landmarks": false}})
            .as_object()
            .unwrap()
            .clone();
        expand_world(&mut world).unwrap();
        assert!(world["decorations"].as_array().unwrap().is_empty());
        assert!(
            world["interactions"]
                .as_array()
                .unwrap()
                .iter()
                .any(|zone| zone["id"] == "maze-finish")
        );
        assert!(
            !world["terrain"]["operations"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        let mut invalid = json!({"maze": {"landmarks": "false"}})
            .as_object()
            .unwrap()
            .clone();
        assert!(
            expand_world(&mut invalid)
                .unwrap_err()
                .to_string()
                .contains("landmarks must be a boolean")
        );
    }

    #[test]
    fn expansion_is_deterministic_for_a_seed() {
        let source = json!({"worlds": {"maze": {"maze": {"width": 6, "height": 5, "seed": 101, "collectibles": {"count": 5}}}}});
        let mut first = source.as_object().unwrap().clone();
        let mut second = source.as_object().unwrap().clone();
        expand_manifest_mazes(&mut first).unwrap();
        expand_manifest_mazes(&mut second).unwrap();
        assert_eq!(first, second);
    }

    #[test]
    fn maze_expansion_leaves_environment_and_review_bounds_to_the_package() {
        let mut manifest = serde_json::from_value::<Map<String, Value>>(json!({
            "worlds": {"maze": {"maze": {
                "width": 10,
                "height": 10,
                "seed": 101
            }}}
        }))
        .unwrap();
        expand_manifest_mazes(&mut manifest).unwrap();
        let world = manifest["worlds"]["maze"].as_object().unwrap();
        let operations = world["terrain"]["operations"].as_array().unwrap();
        assert_eq!(operations[0]["shape"], "block");
        assert_eq!(operations[0]["material"], "ground");
        let decorations = world["decorations"].as_array().unwrap();
        assert_eq!(decorations.len(), 2);
        assert!(
            decorations.iter().all(|decoration| {
                decoration["kind"] == "gate" || decoration["kind"] == "finish"
            })
        );
        assert!(world["world"].get("presentationBounds").is_none());
    }

    #[test]
    fn rejects_out_of_bounds_maze_dimensions() {
        let mut manifest = json!({"worlds": {"maze": {"maze": {"width": 21}}}})
            .as_object()
            .unwrap()
            .clone();
        let error = expand_manifest_mazes(&mut manifest)
            .unwrap_err()
            .to_string();
        assert!(error.contains("width must be an integer between 2 and 20"));
    }

    #[test]
    fn rejects_terrain_resolution_that_cannot_represent_maze_walls() {
        let mut manifest = json!({
            "worlds": {"maze": {"maze": {
                "width": 4,
                "height": 4,
                "wallThickness": 0.7,
                "terrain": {"cellSize": 1.0}
            }}}
        })
        .as_object()
        .unwrap()
        .clone();
        let error = expand_manifest_mazes(&mut manifest)
            .unwrap_err()
            .to_string();
        assert!(error.contains("cellSize (1") && error.contains("wallThickness (0.7)"));
    }

    #[test]
    fn accepts_terrain_resolution_without_environment_caps() {
        let mut manifest = json!({
            "worlds": {"maze": {"maze": {
                "width": 4,
                "height": 4,
                "wallThickness": 1.0,
                "terrain": {"cellSize": 1.0}
            }}}
        })
        .as_object()
        .unwrap()
        .clone();
        expand_manifest_mazes(&mut manifest).unwrap();
        let operations = &manifest["worlds"]["maze"]["terrain"]["operations"];
        assert_eq!(operations[0]["size"][1], json!(2.0));
    }

    #[test]
    fn walls_overlap_the_floor_without_changing_their_top_height() {
        let mut manifest = json!({"worlds": {"maze": {"maze": {
            "width": 4, "height": 4, "wallHeight": 12.0,
            "wallThickness": 2.0,
            "terrain": {"cellSize": 1.0, "wallMaterial": "builtin:leafygrass"}
        }}}})
        .as_object()
        .unwrap()
        .clone();
        expand_manifest_mazes(&mut manifest).unwrap();
        let operations = manifest["worlds"]["maze"]["terrain"]["operations"]
            .as_array()
            .unwrap();
        for wall in &operations[1..] {
            let center = wall["position"][1].as_f64().unwrap();
            let height = wall["size"][1].as_f64().unwrap();
            assert_eq!(center - height / 2.0, -1.0);
            assert_eq!(center + height / 2.0, 12.0);
            assert_eq!(wall["material"], "leafygrass");
        }
    }

    #[test]
    fn twenty_cell_maze_stays_inside_the_runtime_operation_budget() {
        let mut manifest = json!({"worlds": {"maze": {"maze": {
            "width": 20, "height": 20, "cellSize": 15, "wallHeight": 12,
            "wallThickness": 1.8, "terrain": {"cellSize": 1.5}
        }}}})
        .as_object()
        .unwrap()
        .clone();
        expand_manifest_mazes(&mut manifest).unwrap();
        assert!(
            manifest["worlds"]["maze"]["terrain"]["operations"]
                .as_array()
                .unwrap()
                .len()
                <= 512
        );
    }
}
