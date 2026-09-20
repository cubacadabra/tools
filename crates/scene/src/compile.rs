use crate::AuthoringScene;
use crate::transforms::{
    affine_transform, axis_aligned_runtime_size, runtime_ladder_axis,
    runtime_mesh_transform_is_lossless, uniform_runtime_scale,
};
use crate::validation::{component_error, copy_component_value, vector_value};
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;

impl AuthoringScene {
    /// Expand the supported component combinations into the existing runtime
    /// manifest. Unknown components are rejected rather than being silently
    /// dropped from a build.
    pub fn compile_into_manifest(&self, manifest: &mut Value) -> Result<(), String> {
        self.validate()?;
        let world_id = manifest
            .get("launch")
            .and_then(|launch| launch.get("destinationWorld"))
            .and_then(Value::as_str)
            .or_else(|| manifest.get("startWorld").and_then(Value::as_str))
            .unwrap_or("lobby")
            .to_owned();
        if let Some(scene_world_id) = self.world_id.as_deref()
            && scene_world_id != world_id
        {
            return Err(format!(
                "scene.json worldId {scene_world_id:?} does not match manifest world {world_id:?}"
            ));
        }
        let world = if world_id == "lobby" {
            manifest
        } else {
            manifest
                .get_mut("worlds")
                .and_then(Value::as_object_mut)
                .and_then(|worlds| worlds.get_mut(&world_id))
                .ok_or_else(|| format!("scene target world {world_id:?} was not found"))?
        };
        let world = world
            .as_object_mut()
            .ok_or_else(|| format!("scene target world {world_id:?} must be an object"))?;

        let mut blocks = Vec::new();
        let mut decorations = Vec::new();
        let mut signs = Vec::new();
        let mut interactions = Vec::new();
        let mut ladders = Vec::new();
        let mut checkpoints = Vec::new();
        let mut hazards = Vec::new();
        let mut safe_zones = Vec::new();
        let nodes = self.index()?;
        let mut world_cache = BTreeMap::new();
        for node in &self.nodes {
            if !node.editor.visible {
                continue;
            }
            let world = self.world_affine(&node.id, &nodes, &mut world_cache)?;
            let world_transform = affine_transform(world);
            if let Some(primitive) = node.components.get("primitive") {
                let primitive = primitive
                    .as_object()
                    .ok_or_else(|| component_error(node, "primitive must be an object"))?;
                let size = primitive
                    .get("size")
                    .and_then(vector_value)
                    .ok_or_else(|| component_error(node, "primitive requires a size"))?;
                let size = axis_aligned_runtime_size(world, size).map_err(|message| {
                    component_error(node, &format!("primitive transform {message}"))
                })?;
                let mut block = json!({
                    "id": primitive
                        .get("runtimeId")
                        .and_then(Value::as_str)
                        .unwrap_or(&node.id),
                    "position": world_transform.position,
                    "size": size,
                });
                if let Some(material) = primitive.get("material") {
                    block
                        .as_object_mut()
                        .expect("primitive block is an object")
                        .insert("color".to_owned(), material.clone());
                }
                if let Some(material) = primitive.get("runtimeMaterial") {
                    block
                        .as_object_mut()
                        .expect("primitive block is an object")
                        .insert("material".to_owned(), material.clone());
                }
                copy_component_value(
                    primitive,
                    block.as_object_mut().expect("primitive block is an object"),
                    "outline",
                );
                blocks.push(block);
            }
            if let Some(render) = node.components.get("render") {
                let render = render
                    .as_object()
                    .ok_or_else(|| component_error(node, "render must be an object"))?;
                if let Some(mesh) = render.get("mesh").and_then(Value::as_str) {
                    if !runtime_mesh_transform_is_lossless(world) {
                        return Err(component_error(
                            node,
                            "render transform contains shear or unsupported rotation and cannot compile losslessly",
                        ));
                    }
                    let scale = world_transform.scale;
                    if world_transform.rotation[0].abs() > 0.0001
                        || world_transform.rotation[2].abs() > 0.0001
                    {
                        return Err(component_error(
                            node,
                            "render rotation is limited to the runtime mesh adapter's Y axis",
                        ));
                    }
                    let mut decoration = json!({
                        "kind": "mesh",
                        "asset": mesh,
                        "position": world_transform.position,
                        "scale": scale[0],
                        "yaw": world_transform.rotation[1],
                        "color": render.get("color").cloned().unwrap_or_else(|| json!("#FFFFFF")),
                    });
                    if scale
                        .iter()
                        .zip([scale[0]; 3])
                        .any(|(value, uniform)| (*value - uniform).abs() > 0.0001)
                    {
                        decoration
                            .as_object_mut()
                            .expect("mesh decoration is an object")
                            .insert("scale3".to_owned(), json!(scale));
                    }
                    if let Some(material) = render.get("material") {
                        decoration
                            .as_object_mut()
                            .expect("mesh decoration is an object")
                            .insert("material".to_owned(), material.clone());
                    }
                    decorations.push(decoration);
                }
            }
            if let Some(text) = node.components.get("text") {
                let text = text
                    .as_object()
                    .ok_or_else(|| component_error(node, "text must be an object"))?;
                let mut sign = Map::new();
                if !runtime_mesh_transform_is_lossless(world) {
                    return Err(component_error(
                        node,
                        "text transform contains shear and cannot compile losslessly",
                    ));
                }
                sign.insert(
                    "text".to_owned(),
                    text.get("text")
                        .cloned()
                        .unwrap_or_else(|| json!(node.name)),
                );
                sign.insert("position".to_owned(), json!(world_transform.position));
                if world_transform.rotation[0].abs() > 0.0001
                    || world_transform.rotation[2].abs() > 0.0001
                {
                    return Err(component_error(
                        node,
                        "text rotation is limited to the runtime sign adapter's Y axis",
                    ));
                }
                sign.insert("yaw".to_owned(), json!(world_transform.rotation[1]));
                copy_component_value(text, &mut sign, "maxWidth");
                copy_component_value(text, &mut sign, "color");
                copy_component_value(text, &mut sign, "id");
                signs.push(Value::Object(sign));
            }
            if let Some(interaction) = node.components.get("interaction") {
                let interaction = interaction
                    .as_object()
                    .ok_or_else(|| component_error(node, "interaction must be an object"))?;
                let mut output = Map::new();
                output.insert(
                    "id".to_owned(),
                    interaction
                        .get("id")
                        .cloned()
                        .unwrap_or_else(|| Value::String(node.id.clone())),
                );
                output.insert(
                    "kind".to_owned(),
                    interaction
                        .get("kind")
                        .cloned()
                        .unwrap_or_else(|| json!("zone")),
                );
                output.insert(
                    "label".to_owned(),
                    interaction
                        .get("label")
                        .cloned()
                        .unwrap_or_else(|| Value::String(node.name.clone())),
                );
                output.insert("position".to_owned(), json!(world_transform.position));
                let scale = uniform_runtime_scale(world).map_err(|message| {
                    component_error(node, &format!("interaction transform {message}"))
                })?;
                output.insert(
                    "radius".to_owned(),
                    json!(
                        interaction
                            .get("radius")
                            .and_then(Value::as_f64)
                            .unwrap_or(4.0)
                            * f64::from(scale)
                    ),
                );
                for key in ["color", "visual"] {
                    copy_component_value(interaction, &mut output, key);
                }
                interactions.push(Value::Object(output));
            }
            if let Some(ladder) = node.components.get("ladder") {
                let ladder = ladder
                    .as_object()
                    .ok_or_else(|| component_error(node, "ladder must be an object"))?;
                let size = ladder
                    .get("size")
                    .and_then(vector_value)
                    .ok_or_else(|| component_error(node, "ladder requires a size"))?;
                let size = axis_aligned_runtime_size(world, size).map_err(|message| {
                    component_error(node, &format!("ladder transform {message}"))
                })?;
                let mut output = Map::new();
                copy_component_value(ladder, &mut output, "id");
                output.insert("position".to_owned(), json!(world_transform.position));
                output.insert("size".to_owned(), json!(size));
                let climb_axis = runtime_ladder_axis(
                    world,
                    ladder
                        .get("climbAxis")
                        .and_then(Value::as_str)
                        .unwrap_or("z"),
                )
                .map_err(|message| component_error(node, message))?;
                output.insert("climbAxis".to_owned(), json!(climb_axis));
                for key in ["color", "climbSpeed"] {
                    copy_component_value(ladder, &mut output, key);
                }
                ladders.push(Value::Object(output));
            }
            if let Some(checkpoint) = node.components.get("checkpoint") {
                let checkpoint = checkpoint
                    .as_object()
                    .ok_or_else(|| component_error(node, "checkpoint must be an object"))?;
                let scale = uniform_runtime_scale(world).map_err(|message| {
                    component_error(node, &format!("checkpoint transform {message}"))
                })?;
                let mut output = Map::new();
                copy_component_value(checkpoint, &mut output, "id");
                output.insert("position".to_owned(), json!(world_transform.position));
                output.insert(
                    "radius".to_owned(),
                    json!(
                        checkpoint
                            .get("radius")
                            .and_then(Value::as_f64)
                            .unwrap_or(2.7)
                            * f64::from(scale)
                    ),
                );
                checkpoints.push(Value::Object(output));
            }
            if let Some(hazard) = node.components.get("hazard") {
                let hazard = hazard
                    .as_object()
                    .ok_or_else(|| component_error(node, "hazard must be an object"))?;
                let size = hazard
                    .get("size")
                    .and_then(vector_value)
                    .ok_or_else(|| component_error(node, "hazard requires a size"))?;
                let size = axis_aligned_runtime_size(world, size).map_err(|message| {
                    component_error(node, &format!("hazard transform {message}"))
                })?;
                let mut output = Map::new();
                copy_component_value(hazard, &mut output, "id");
                output.insert("position".to_owned(), json!(world_transform.position));
                output.insert("size".to_owned(), json!(size));
                for key in ["kind", "damagePerSecond"] {
                    copy_component_value(hazard, &mut output, key);
                }
                hazards.push(Value::Object(output));
            }
            if let Some(safe_zone) = node.components.get("safeZone") {
                let safe_zone = safe_zone
                    .as_object()
                    .ok_or_else(|| component_error(node, "safeZone must be an object"))?;
                let scale = uniform_runtime_scale(world).map_err(|message| {
                    component_error(node, &format!("safeZone transform {message}"))
                })?;
                let mut output = Map::new();
                copy_component_value(safe_zone, &mut output, "id");
                output.insert("position".to_owned(), json!(world_transform.position));
                output.insert(
                    "radius".to_owned(),
                    json!(
                        safe_zone
                            .get("radius")
                            .and_then(Value::as_f64)
                            .unwrap_or(5.0)
                            * f64::from(scale)
                    ),
                );
                copy_component_value(safe_zone, &mut output, "healPerSecond");
                safe_zones.push(Value::Object(output));
            }
        }
        world.insert("blocks".to_owned(), Value::Array(blocks));
        world.insert("decorations".to_owned(), Value::Array(decorations));
        world.insert("signs".to_owned(), Value::Array(signs));
        world.insert("interactions".to_owned(), Value::Array(interactions));
        world.insert("ladders".to_owned(), Value::Array(ladders));
        world.insert("checkpoints".to_owned(), Value::Array(checkpoints));
        world.insert("hazards".to_owned(), Value::Array(hazards));
        world.insert("safeZones".to_owned(), Value::Array(safe_zones));
        Ok(())
    }
}
