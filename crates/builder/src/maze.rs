//! Expansion of the bounded, authoring-friendly maze declaration.
//!
//! The native builder is the canonical package path.  Keep this expansion
//! here, rather than in the runtime, so packages contain ordinary terrain,
//! interaction, and checkpoint data that every host can consume.

use serde_json::{Map, Value, json};

use crate::{BuildError, Result};

const MAX_MAZE_WIDTH: usize = 12;
const MAX_MAZE_HEIGHT: usize = 12;
const TERRAIN_MATERIALS: &[&str] = &["grass", "ground", "dirt", "rock", "sand", "mud", "snow"];

#[derive(Clone, Copy, Debug, Default)]
struct Cell {
    north: bool,
    east: bool,
    south: bool,
    west: bool,
}

#[derive(Clone, Copy)]
struct Direction {
    dx: isize,
    dy: isize,
    wall: usize,
    opposite: usize,
}

const DIRECTIONS: [Direction; 4] = [
    Direction {
        dx: 0,
        dy: -1,
        wall: 0,
        opposite: 2,
    },
    Direction {
        dx: 1,
        dy: 0,
        wall: 1,
        opposite: 3,
    },
    Direction {
        dx: 0,
        dy: 1,
        wall: 2,
        opposite: 0,
    },
    Direction {
        dx: -1,
        dy: 0,
        wall: 3,
        opposite: 1,
    },
];

/// Expand every `world.maze` declaration in a manifest in place.
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
    let rock_asset = config
        .get("rockAsset")
        .and_then(Value::as_str)
        .filter(|asset| !asset.trim().is_empty());
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
    let center = [
        origin[0] + extent_x * 0.5,
        origin[1],
        origin[2] + extent_z * 0.5,
    ];
    // Build the island as a tapered stack instead of one deep rectangular
    // slab. The top remains a precise maze plateau, while the lower layers
    // step inward and the rounded shoulder balls break up the silhouette.
    operations.push(json!({
        "operation": "fill", "shape": "block",
        "position": [center[0], origin[1] - 4.2, center[2]],
        "size": [extent_x + 10.0, 4.2, extent_z + 10.0],
        "material": floor_material
    }));
    operations.push(json!({
        "operation": "fill", "shape": "block",
        "position": [center[0], origin[1] - 1.5, center[2]],
        "size": [extent_x + 5.0, 3.0, extent_z + 5.0],
        "material": floor_material
    }));
    let half_x = extent_x * 0.5 + 5.0;
    let half_z = extent_z * 0.5 + 5.0;
    for (x, z, radius) in [
        (center[0] - half_x, center[2] - half_z, 5.0),
        (center[0] + half_x, center[2] - half_z, 5.8),
        (center[0] - half_x, center[2] + half_z, 5.6),
        (center[0] + half_x, center[2] + half_z, 5.2),
        (center[0], center[2] - half_z, 4.3),
        (center[0], center[2] + half_z, 4.7),
        (center[0] - half_x, center[2], 4.5),
        (center[0] + half_x, center[2], 4.8),
    ] {
        operations.push(json!({
            "operation": "fill", "shape": "ball",
            "position": [x, origin[1] - 3.7, z],
            "radius": radius,
            "material": floor_material
        }));
    }
    // Background islands are intentionally simple silhouettes. They create
    // depth and scale without becoming additional playable maze worlds.
    let background_islands = [
        (
            center[0] - extent_x * 0.92,
            origin[1] - 2.4,
            center[2] - extent_z * 0.72,
            5.5,
        ),
        (
            center[0] + extent_x * 0.98,
            origin[1] - 3.2,
            center[2] - extent_z * 0.30,
            4.5,
        ),
        (
            center[0] + extent_x * 0.68,
            origin[1] - 2.8,
            center[2] + extent_z * 0.95,
            5.0,
        ),
    ];
    for (x, y, z, radius) in background_islands {
        operations.push(json!({
            "operation": "fill", "shape": "ball",
            "position": [x, y, z], "radius": radius, "material": floor_material
        }));
        operations.push(json!({
            "operation": "fill", "shape": "block",
            "position": [x, origin[1] + 0.35, z],
            "size": [radius * 1.15, 0.7, radius * 1.15],
            "material": wall_material
        }));
    }
    operations.push(json!({
        "operation": "fill",
        "shape": "block",
        "position": [
            origin[0] + width as f64 * cell_size / 2.0,
            origin[1] - 1.0,
            origin[2] + height as f64 * cell_size / 2.0
        ],
        "size": [width as f64 * cell_size, 2.0, height as f64 * cell_size],
        "material": floor_material
    }));
    for y in 0..height {
        for x in 0..width {
            let cell = cells[y * width + x];
            let cell_x = origin[0] + x as f64 * cell_size;
            let cell_z = origin[2] + y as f64 * cell_size;
            if cell.north {
                operations.push(json!({
                    "operation": "fill", "shape": "block",
                    "position": [cell_x + cell_size / 2.0, origin[1] + wall_height / 2.0, cell_z],
                    "size": [cell_size + wall_thickness, wall_height, wall_thickness],
                    "material": wall_material
                }));
            }
            if cell.west {
                operations.push(json!({
                    "operation": "fill", "shape": "block",
                    "position": [cell_x, origin[1] + wall_height / 2.0, cell_z + cell_size / 2.0],
                    "size": [wall_thickness, wall_height, cell_size + wall_thickness],
                    "material": wall_material
                }));
            }
            if x == width - 1 && cell.east {
                operations.push(json!({
                    "operation": "fill", "shape": "block",
                    "position": [cell_x + cell_size, origin[1] + wall_height / 2.0, cell_z + cell_size / 2.0],
                    "size": [wall_thickness, wall_height, cell_size + wall_thickness],
                    "material": wall_material
                }));
            }
            if y == height - 1 && cell.south {
                operations.push(json!({
                    "operation": "fill", "shape": "block",
                    "position": [cell_x + cell_size / 2.0, origin[1] + wall_height / 2.0, cell_z + cell_size],
                    "size": [cell_size + wall_thickness, wall_height, wall_thickness],
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
    // Dress quiet wall-side pockets, not the solution route. This keeps the
    // path readable while putting props where the source game's environment
    // art naturally accumulates: against walls and at the island perimeter.
    let route_cells = route.clone();
    let mut dressing_index = 0usize;
    for y in 0..height {
        for x in 0..width {
            let cell = (x, y);
            if route_cells.contains(&cell) || cell == start || cell == finish {
                continue;
            }
            let cell_data = cells[y * width + x];
            let decoration_position = wall_side_position(origin, cell_size, cell, cell_data);
            let index = dressing_index;
            dressing_index += 1;
            if index % 5 != 0 {
                continue;
            }
            if index % 15 == 0 {
                decorations.push(json!({
                    "id": format!("maze-palm-{index:02}"), "kind": "palm",
                    "position": decoration_position,
                    "scale": 0.9 + (index % 3) as f64 * 0.12,
                    "yaw": (index % 6) as f64 * 0.7, "variant": index % 2
                }));
            } else if index % 2 == 0 {
                let mut rock = json!({
                    "id": format!("maze-rock-{index:02}"),
                    "kind": "rock",
                    "position": decoration_position,
                    "scale": 0.8 + (index % 4) as f64 * 0.12,
                    "yaw": (index % 8) as f64 * 0.4,
                    "variant": index % 3
                });
                if let Some(asset) = rock_asset {
                    rock["kind"] = json!("mesh");
                    rock["asset"] = json!(asset);
                }
                decorations.push(rock);
            } else {
                decorations.push(json!({
                    "id": format!("maze-grass-{index:02}"), "kind": "grass-clump",
                    "position": decoration_position,
                    "scale": 0.8 + (index % 2) as f64 * 0.2,
                    "variant": index % 2
                }));
            }
        }
    }
    // Give the distant silhouettes a small amount of readable scale and
    // repetition without turning them into playable content.
    for (index, (x, _y, z, _)) in background_islands.iter().enumerate() {
        decorations.push(json!({
            "id": format!("maze-background-palm-{index:02}"), "kind": "palm",
            "position": [*x, origin[1] + 1.0, *z],
            "scale": 0.55 + index as f64 * 0.08,
            "yaw": index as f64 * 1.4,
            "variant": index % 2
        }));
    }
    decorations.push(json!({
        "id": "maze-bridge", "kind": "bridge",
        "position": [center[0], origin[1] + 1.0, origin[2] - 7.0],
        "scale": 1.2, "yaw": 0.0
    }));
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

fn integer(
    config: &Map<String, Value>,
    key: &str,
    default: usize,
    minimum: usize,
    maximum: usize,
) -> Result<usize> {
    let Some(raw) = config.get(key) else {
        return Ok(default);
    };
    let Some(raw) = raw.as_u64() else {
        return Err(BuildError(format!(
            "manifest.maze.{key} must be an integer between {minimum} and {maximum}"
        )));
    };
    let value = usize::try_from(raw).unwrap_or(usize::MAX);
    if value < minimum || value > maximum {
        return Err(BuildError(format!(
            "manifest.maze.{key} must be an integer between {minimum} and {maximum}"
        )));
    }
    Ok(value)
}

fn number(
    config: &Map<String, Value>,
    key: &str,
    default: f64,
    minimum: f64,
    maximum: f64,
) -> Result<f64> {
    let Some(value) = config.get(key) else {
        return Ok(default);
    };
    let value = value.as_f64().ok_or_else(|| {
        BuildError(format!(
            "manifest.maze.{key} must be between {minimum} and {maximum}"
        ))
    })?;
    if !value.is_finite() || value < minimum || value > maximum {
        return Err(BuildError(format!(
            "manifest.maze.{key} must be between {minimum} and {maximum}"
        )));
    }
    Ok(value)
}

fn cell(
    config: &Map<String, Value>,
    key: &str,
    default: (usize, usize),
    width: usize,
    height: usize,
) -> Result<(usize, usize)> {
    let Some(value) = config.get(key) else {
        return Ok(default);
    };
    let values = value.as_array().ok_or_else(|| {
        BuildError(format!(
            "manifest.maze.{key} must be a two-item cell coordinate inside the maze"
        ))
    })?;
    if values.len() != 2 || values.iter().any(|value| value.as_u64().is_none()) {
        return Err(BuildError(format!(
            "manifest.maze.{key} must be a two-item cell coordinate inside the maze"
        )));
    }
    let result = (
        values[0].as_u64().unwrap() as usize,
        values[1].as_u64().unwrap() as usize,
    );
    if result.0 >= width || result.1 >= height {
        return Err(BuildError(format!(
            "manifest.maze.{key} must be a two-item cell coordinate inside the maze"
        )));
    }
    Ok(result)
}

fn origin(
    config: &Map<String, Value>,
    width: usize,
    height: usize,
    cell_size: f64,
) -> Result<[f64; 3]> {
    let default = [
        -(width as f64 * cell_size) / 2.0,
        0.0,
        -(height as f64 * cell_size) / 2.0,
    ];
    let Some(value) = config.get("origin") else {
        return Ok(default);
    };
    let values = value.as_array().ok_or_else(|| {
        BuildError("manifest.maze.origin must contain three finite numbers".to_owned())
    })?;
    if values.len() != 3 {
        return Err(BuildError(
            "manifest.maze.origin must contain three finite numbers".to_owned(),
        ));
    }
    let mut result = [0.0; 3];
    for (index, value) in values.iter().enumerate() {
        result[index] = value.as_f64().ok_or_else(|| {
            BuildError("manifest.maze.origin must contain three finite numbers".to_owned())
        })?;
        if !result[index].is_finite() {
            return Err(BuildError(
                "manifest.maze.origin must contain three finite numbers".to_owned(),
            ));
        }
    }
    Ok(result)
}

fn material(config: &Map<String, Value>, key: &str, default: &str) -> Result<String> {
    let value = config.get(key).and_then(Value::as_str).unwrap_or(default);
    let value = value
        .strip_prefix("builtin:")
        .unwrap_or(value)
        .to_ascii_lowercase();
    if !TERRAIN_MATERIALS.contains(&value.as_str()) {
        return Err(BuildError(format!(
            "manifest.maze.terrain.{key} must be a built-in terrain material"
        )));
    }
    Ok(value)
}

fn array_clone(world: &Map<String, Value>, key: &str) -> Result<Vec<Value>> {
    match world.get(key) {
        None => Ok(Vec::new()),
        Some(Value::Array(values)) => Ok(values.clone()),
        Some(_) => Err(BuildError(format!("world.{key} must be an array"))),
    }
}

fn object_clone(world: &Map<String, Value>, key: &str) -> Result<Map<String, Value>> {
    match world.get(key) {
        None => Ok(Map::new()),
        Some(Value::Object(values)) => Ok(values.clone()),
        Some(_) => Err(BuildError(format!("world.{key} must be an object"))),
    }
}

fn position(origin: [f64; 3], cell_size: f64, cell: (usize, usize), y: f64) -> Value {
    json!([
        origin[0] + (cell.0 as f64 + 0.5) * cell_size,
        origin[1] + y,
        origin[2] + (cell.1 as f64 + 0.5) * cell_size
    ])
}

fn wall_side_position(
    origin: [f64; 3],
    cell_size: f64,
    cell: (usize, usize),
    walls: Cell,
) -> Value {
    let center_x = origin[0] + (cell.0 as f64 + 0.5) * cell_size;
    let center_z = origin[2] + (cell.1 as f64 + 0.5) * cell_size;
    let inset = (cell_size * 0.22).max(0.9);
    let (x, z) = if walls.north {
        (center_x, center_z - cell_size * 0.5 + inset)
    } else if walls.west {
        (center_x - cell_size * 0.5 + inset, center_z)
    } else if walls.south {
        (center_x, center_z + cell_size * 0.5 - inset)
    } else {
        (center_x + cell_size * 0.5 - inset, center_z)
    };
    json!([x, origin[1] + 0.55, z])
}

fn carve(width: usize, height: usize, seed: u32) -> Vec<Cell> {
    let mut cells = vec![
        Cell {
            north: true,
            east: true,
            south: true,
            west: true
        };
        width * height
    ];
    let mut visited = vec![false; width * height];
    let mut stack = vec![(0usize, 0usize)];
    let mut rng = PythonRandom::new(seed);
    visited[0] = true;
    while let Some(&(x, y)) = stack.last() {
        let choices: Vec<_> = DIRECTIONS
            .iter()
            .copied()
            .filter(|direction| {
                let next_x = x as isize + direction.dx;
                let next_y = y as isize + direction.dy;
                next_x >= 0
                    && next_x < width as isize
                    && next_y >= 0
                    && next_y < height as isize
                    && !visited[next_y as usize * width + next_x as usize]
            })
            .collect();
        if choices.is_empty() {
            stack.pop();
            continue;
        }
        let direction = choices[rng.randbelow(choices.len())];
        let next_x = (x as isize + direction.dx) as usize;
        let next_y = (y as isize + direction.dy) as usize;
        cells[y * width + x].set(direction.wall, false);
        cells[next_y * width + next_x].set(direction.opposite, false);
        visited[next_y * width + next_x] = true;
        stack.push((next_x, next_y));
    }
    cells
}

impl Cell {
    fn set(&mut self, wall: usize, value: bool) {
        match wall {
            0 => self.north = value,
            1 => self.east = value,
            2 => self.south = value,
            3 => self.west = value,
            _ => unreachable!(),
        }
    }
    fn open(&self, wall: usize) -> bool {
        match wall {
            0 => !self.north,
            1 => !self.east,
            2 => !self.south,
            3 => !self.west,
            _ => false,
        }
    }
}

fn route(
    cells: &[Cell],
    width: usize,
    height: usize,
    start: (usize, usize),
    finish: (usize, usize),
) -> Result<Vec<(usize, usize)>> {
    let mut previous = vec![None; width * height];
    let mut queue = std::collections::VecDeque::from([start]);
    previous[start.1 * width + start.0] = Some(start);
    while let Some((x, y)) = queue.pop_front() {
        if (x, y) == finish {
            break;
        }
        for direction in DIRECTIONS {
            let next_x = x as isize + direction.dx;
            let next_y = y as isize + direction.dy;
            if next_x < 0
                || next_x >= width as isize
                || next_y < 0
                || next_y >= height as isize
                || !cells[y * width + x].open(direction.wall)
            {
                continue;
            }
            let next = (next_x as usize, next_y as usize);
            let slot = next.1 * width + next.0;
            if previous[slot].is_none() {
                previous[slot] = Some((x, y));
                queue.push_back(next);
            }
        }
    }
    if previous[finish.1 * width + finish.0].is_none() {
        return Err(BuildError(
            "manifest.maze generated a maze without a start-to-finish path".to_owned(),
        ));
    }
    let mut result = Vec::new();
    let mut cursor = finish;
    loop {
        result.push(cursor);
        if cursor == start {
            break;
        }
        cursor = previous[cursor.1 * width + cursor.0].unwrap();
    }
    result.reverse();
    Ok(result)
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
struct PythonRandom {
    state: [u32; 624],
    index: usize,
}

impl PythonRandom {
    fn new(seed: u32) -> Self {
        let mut state = [0u32; 624];
        state[0] = 19650218;
        for i in 1..624 {
            state[i] = (1812433253u64
                .wrapping_mul((state[i - 1] ^ (state[i - 1] >> 30)) as u64)
                .wrapping_add(i as u64)) as u32;
        }
        let key = [seed];
        let mut i = 1usize;
        let mut j = 0usize;
        let mut k = 624usize;
        while k > 0 {
            state[i] = (state[i] ^ ((state[i - 1] ^ (state[i - 1] >> 30)).wrapping_mul(1664525)))
                .wrapping_add(key[j])
                .wrapping_add(j as u32);
            i += 1;
            j += 1;
            if i >= 624 {
                state[0] = state[623];
                i = 1;
            }
            if j >= key.len() {
                j = 0;
            }
            k -= 1;
        }
        k = 623;
        while k > 0 {
            state[i] = (state[i]
                ^ ((state[i - 1] ^ (state[i - 1] >> 30)).wrapping_mul(1566083941)))
            .wrapping_sub(i as u32);
            i += 1;
            if i >= 624 {
                state[0] = state[623];
                i = 1;
            }
            k -= 1;
        }
        state[0] = 0x8000_0000;
        Self { state, index: 624 }
    }

    fn next_u32(&mut self) -> u32 {
        if self.index >= 624 {
            for i in 0..624 {
                let y = (self.state[i] & 0x8000_0000) | (self.state[(i + 1) % 624] & 0x7fff_ffff);
                self.state[i] = self.state[(i + 397) % 624]
                    ^ (y >> 1)
                    ^ if y & 1 != 0 { 0x9908_b0df } else { 0 };
            }
            self.index = 0;
        }
        let mut y = self.state[self.index];
        self.index += 1;
        y ^= y >> 11;
        y ^= (y << 7) & 0x9d2c_5680;
        y ^= (y << 15) & 0xefc6_0000;
        y ^= y >> 18;
        y
    }

    fn randbelow(&mut self, upper: usize) -> usize {
        assert!(upper > 0, "random upper bound must be positive");
        // Python's _randbelow uses ``n.bit_length()``, including one bit for
        // an upper bound of one.
        let bits = (usize::BITS - upper.leading_zeros()).min(32) as usize;
        loop {
            let value = (self.next_u32() >> (32 - bits)) as usize;
            if value < upper {
                return value;
            }
        }
    }

    fn shuffle<T>(&mut self, values: &mut [T]) {
        for index in (1..values.len()).rev() {
            values.swap(index, self.randbelow(index + 1));
        }
    }
}

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
    fn expansion_is_deterministic_for_a_seed() {
        let source = json!({"worlds": {"maze": {"maze": {"width": 6, "height": 5, "seed": 101, "collectibles": {"count": 5}}}}});
        let mut first = source.as_object().unwrap().clone();
        let mut second = source.as_object().unwrap().clone();
        expand_manifest_mazes(&mut first).unwrap();
        expand_manifest_mazes(&mut second).unwrap();
        assert_eq!(first, second);
    }

    #[test]
    fn rejects_out_of_bounds_maze_dimensions() {
        let mut manifest = json!({"worlds": {"maze": {"maze": {"width": 13}}}})
            .as_object()
            .unwrap()
            .clone();
        let error = expand_manifest_mazes(&mut manifest)
            .unwrap_err()
            .to_string();
        assert!(error.contains("width must be an integer between 2 and 12"));
    }
}
