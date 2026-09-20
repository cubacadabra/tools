//! The project-owned authoring scene format.
//!
//! This is deliberately an entity/component document rather than a copy of
//! Roblox's class hierarchy. Builders and editor hosts consume this model;
//! they do not own its serialized representation.

use glam::{Affine3A, EulerRot, Quat, Vec3};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, BTreeSet};

pub const AUTHORING_SCENE_FORMAT_VERSION: u32 = 1;
pub const MIN_AUTHORING_SCALE: f32 = 0.05;
const AUTHORING_FLOAT_PRECISION: f64 = 1_000_000.0;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AuthoringScene {
    pub format_version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub world_id: Option<String>,
    pub nodes: Vec<AuthoringNode>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AuthoringNode {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
    pub name: String,
    #[serde(default)]
    pub transform: Transform,
    #[serde(default)]
    pub components: BTreeMap<String, Value>,
    #[serde(default)]
    pub editor: EditorMetadata,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<SourceMetadata>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct Transform {
    #[serde(default)]
    pub position: [f32; 3],
    #[serde(default)]
    pub rotation: [f32; 3],
    #[serde(default = "identity_scale")]
    pub scale: [f32; 3],
}

/// The derived world-space transform used by both the builder and Studio.
/// Authoring rotations are XYZ Euler angles in radians; serialized position
/// and scale remain ordinary world units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AuthoringWorldTransform {
    pub position: [f32; 3],
    pub rotation: [f32; 3],
    pub scale: [f32; 3],
}

impl Default for Transform {
    fn default() -> Self {
        Self {
            position: [0.0; 3],
            rotation: [0.0; 3],
            scale: identity_scale(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct EditorMetadata {
    #[serde(default = "default_true")]
    pub visible: bool,
    #[serde(default)]
    pub locked: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lock_reason: Option<String>,
}

impl Default for EditorMetadata {
    fn default() -> Self {
        Self {
            visible: true,
            locked: false,
            lock_reason: None,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SourceMetadata {
    pub format: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub class: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(flatten)]
    pub properties: BTreeMap<String, Value>,
}

fn identity_scale() -> [f32; 3] {
    [1.0; 3]
}

fn default_true() -> bool {
    true
}

pub fn parse_authoring_scene(source: &str) -> Result<AuthoringScene, String> {
    let scene: AuthoringScene = serde_json::from_str(source)
        .map_err(|error| format!("could not parse scene.json: {error}"))?;
    scene.validate()?;
    Ok(scene)
}

pub fn serialize_authoring_scene(scene: &AuthoringScene) -> Result<String, String> {
    scene.validate()?;
    let mut value = serde_json::to_value(scene)
        .map_err(|error| format!("could not prepare scene.json for serialization: {error}"))?;
    normalize_authoring_numbers(&mut value);
    serde_json::to_string_pretty(&value)
        .map(|source| format!("{source}\n"))
        .map_err(|error| format!("could not serialize scene.json: {error}"))
}

fn normalize_authoring_numbers(value: &mut Value) {
    match value {
        Value::Array(values) => {
            for value in values {
                normalize_authoring_numbers(value);
            }
        }
        Value::Object(values) => {
            for value in values.values_mut() {
                normalize_authoring_numbers(value);
            }
        }
        Value::Number(number) if !number.is_i64() && !number.is_u64() => {
            if let Some(number_value) = number.as_f64() {
                let rounded =
                    (number_value * AUTHORING_FLOAT_PRECISION).round() / AUTHORING_FLOAT_PRECISION;
                let rounded = if rounded == -0.0 { 0.0 } else { rounded };
                if let Some(normalized) = serde_json::Number::from_f64(rounded) {
                    *number = normalized;
                }
            }
        }
        _ => {}
    }
}

impl AuthoringScene {
    pub fn validate(&self) -> Result<(), String> {
        if self.format_version != AUTHORING_SCENE_FORMAT_VERSION {
            return Err(format!(
                "scene formatVersion must be {AUTHORING_SCENE_FORMAT_VERSION}, found {}",
                self.format_version
            ));
        }

        let mut ids = BTreeSet::new();
        let mut nodes_by_id = BTreeMap::new();
        for node in &self.nodes {
            if node.id.trim().is_empty() {
                return Err(format!("scene node {:?} has an empty id", node.name));
            }
            if node.name.trim().is_empty() {
                return Err(format!("scene node {} has an empty name", node.id));
            }
            if !ids.insert(node.id.clone()) {
                return Err(format!(
                    "scene node id {:?} is duplicated (name {:?})",
                    node.id, node.name
                ));
            }
            validate_transform(node)?;
            validate_components(node)?;
            nodes_by_id.insert(node.id.as_str(), node);
        }
        for node in &self.nodes {
            if let Some(parent_id) = &node.parent_id
                && !ids.contains(parent_id)
            {
                return Err(format!(
                    "scene node {} ({}) refers to missing parent {}",
                    node.id, node.name, parent_id
                ));
            }
        }
        let mut state = BTreeMap::<&str, u8>::new();
        for node in &self.nodes {
            if state.get(node.id.as_str()) == Some(&2) {
                continue;
            }
            let mut path = Vec::new();
            let mut current = node.id.as_str();
            loop {
                if state.get(current) == Some(&2) {
                    break;
                }
                if state.get(current) == Some(&1) {
                    return Err(format!(
                        "scene node {} ({}) is part of a parent cycle",
                        node.id, node.name
                    ));
                }
                state.insert(current, 1);
                path.push(current);
                let Some(parent_id) = nodes_by_id
                    .get(current)
                    .and_then(|candidate| candidate.parent_id.as_deref())
                else {
                    break;
                };
                current = parent_id;
            }
            for id in path {
                state.insert(id, 2);
            }
        }
        Ok(())
    }

    fn index(&self) -> Result<BTreeMap<&str, &AuthoringNode>, String> {
        let mut nodes = BTreeMap::new();
        for node in &self.nodes {
            if nodes.insert(node.id.as_str(), node).is_some() {
                return Err(format!(
                    "scene node id {:?} is duplicated (name {:?})",
                    node.id, node.name
                ));
            }
        }
        Ok(nodes)
    }

    pub fn world_transform(&self, id: &str) -> Result<AuthoringWorldTransform, String> {
        self.world_transforms()?
            .remove(id)
            .ok_or_else(|| format!("scene node {id} was not found"))
    }

    pub fn world_transforms(&self) -> Result<BTreeMap<String, AuthoringWorldTransform>, String> {
        let nodes = self.index()?;
        let mut cache = BTreeMap::new();
        self.nodes
            .iter()
            .map(|node| {
                self.world_affine(&node.id, &nodes, &mut cache)
                    .map(|transform| (node.id.clone(), affine_transform(transform)))
            })
            .collect()
    }

    pub fn local_position_for_world(
        &self,
        id: &str,
        world_position: [f32; 3],
    ) -> Result<[f32; 3], String> {
        if world_position.iter().any(|value| !value.is_finite()) {
            return Err(format!(
                "scene node {id} position must contain finite values"
            ));
        }
        let nodes = self.index()?;
        let node = nodes
            .get(id)
            .ok_or_else(|| format!("scene node {id} was not found"))?;
        let Some(parent_id) = node.parent_id.as_deref() else {
            return Ok(world_position);
        };
        let mut cache = BTreeMap::new();
        let parent = self.world_affine(parent_id, &nodes, &mut cache)?;
        Ok(parent
            .inverse()
            .transform_point3(Vec3::from_array(world_position))
            .to_array())
    }

    pub fn local_transform_for_world(
        &self,
        id: &str,
        world_position: [f32; 3],
        world_scale: [f32; 3],
    ) -> Result<([f32; 3], [f32; 3]), String> {
        if world_scale
            .iter()
            .any(|value| !value.is_finite() || *value < MIN_AUTHORING_SCALE)
        {
            return Err(format!(
                "scene node {id} world scale must contain finite values of at least {MIN_AUTHORING_SCALE}"
            ));
        }
        let nodes = self.index()?;
        let node = nodes
            .get(id)
            .ok_or_else(|| format!("scene node {id} was not found"))?;
        let local_position = self.local_position_for_world(id, world_position)?;
        let Some(parent_id) = node.parent_id.as_deref() else {
            return Ok((local_position, world_scale));
        };
        let parent = self.world_transform(parent_id)?;
        if parent.rotation[0].abs() > 0.0001 || parent.rotation[2].abs() > 0.0001 {
            return Err(format!(
                "scene node {id} has a parent with unsupported X/Z rotation"
            ));
        }
        if parent.scale.iter().any(|value| value.abs() <= f32::EPSILON) {
            return Err(format!("scene node {id} has a singular parent scale"));
        }
        let local_scale = [
            world_scale[0] / parent.scale[0],
            world_scale[1] / parent.scale[1],
            world_scale[2] / parent.scale[2],
        ];
        if (parent.scale[0] - parent.scale[2]).abs() <= 0.0001
            || node.transform.rotation[1].abs() <= 0.0001
        {
            return Ok((local_position, local_scale));
        }
        Err(format!(
            "scene node {id} cannot resize under a non-uniform rotated parent without shear"
        ))
    }

    fn world_affine<'a>(
        &'a self,
        id: &str,
        nodes: &BTreeMap<&str, &AuthoringNode>,
        cache: &mut BTreeMap<String, Affine3A>,
    ) -> Result<Affine3A, String> {
        if let Some(transform) = cache.get(id) {
            return Ok(*transform);
        }
        let node = nodes
            .get(id)
            .ok_or_else(|| format!("scene node {id} was not found"))?;
        let local = local_affine(&node.transform);
        let world = if let Some(parent_id) = node.parent_id.as_deref() {
            self.world_affine(parent_id, nodes, cache)? * local
        } else {
            local
        };
        cache.insert(id.to_owned(), world);
        Ok(world)
    }

    pub fn node(&self, id: &str) -> Option<&AuthoringNode> {
        self.nodes.iter().find(|node| node.id == id)
    }

    pub fn node_mut(&mut self, id: &str) -> Option<&mut AuthoringNode> {
        self.nodes.iter_mut().find(|node| node.id == id)
    }

    pub fn set_position(&mut self, id: &str, position: [f32; 3]) -> Result<[f32; 3], String> {
        if position.iter().any(|value| !value.is_finite()) {
            return Err(format!(
                "scene node {id} position must contain finite values"
            ));
        }
        let node = self
            .node_mut(id)
            .ok_or_else(|| format!("scene node {id} was not found"))?;
        if node.editor.locked {
            return Err(format!(
                "scene node {} ({}) is locked{}",
                node.id,
                node.name,
                node.editor
                    .lock_reason
                    .as_deref()
                    .map(|reason| format!(": {reason}"))
                    .unwrap_or_default()
            ));
        }
        let previous = node.transform.position;
        node.transform.position = position;
        Ok(previous)
    }

    pub fn set_scale(&mut self, id: &str, scale: [f32; 3]) -> Result<[f32; 3], String> {
        if scale
            .iter()
            .any(|value| !value.is_finite() || *value < MIN_AUTHORING_SCALE)
        {
            return Err(format!(
                "scene node {id} scale must contain finite values of at least {MIN_AUTHORING_SCALE}"
            ));
        }
        let node = self
            .node_mut(id)
            .ok_or_else(|| format!("scene node {id} was not found"))?;
        if node.editor.locked {
            return Err(format!(
                "scene node {} ({}) is locked{}",
                node.id,
                node.name,
                node.editor
                    .lock_reason
                    .as_deref()
                    .map(|reason| format!(": {reason}"))
                    .unwrap_or_default()
            ));
        }
        let previous = node.transform.scale;
        node.transform.scale = scale;
        Ok(previous)
    }

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

fn local_affine(transform: &Transform) -> Affine3A {
    Affine3A::from_scale_rotation_translation(
        Vec3::from_array(transform.scale),
        Quat::from_euler(
            EulerRot::XYZ,
            transform.rotation[0],
            transform.rotation[1],
            transform.rotation[2],
        ),
        Vec3::from_array(transform.position),
    )
}

fn affine_transform(transform: Affine3A) -> AuthoringWorldTransform {
    let (scale, rotation, position) = transform.to_scale_rotation_translation();
    let (x, y, z) = rotation.to_euler(EulerRot::XYZ);
    AuthoringWorldTransform {
        position: position.to_array(),
        rotation: [x, y, z],
        scale: scale.to_array(),
    }
}

fn runtime_mesh_transform_is_lossless(transform: Affine3A) -> bool {
    let decomposed = affine_transform(transform);
    let rebuilt = Affine3A::from_scale_rotation_translation(
        Vec3::from_array(decomposed.scale),
        Quat::from_euler(
            EulerRot::XYZ,
            decomposed.rotation[0],
            decomposed.rotation[1],
            decomposed.rotation[2],
        ),
        Vec3::from_array(decomposed.position),
    );
    transform
        .to_cols_array()
        .into_iter()
        .zip(rebuilt.to_cols_array())
        .all(|(left, right)| (left - right).abs() <= 0.001)
}

fn axis_aligned_runtime_size(
    transform: Affine3A,
    local_size: [f32; 3],
) -> Result<[f32; 3], &'static str> {
    if !runtime_mesh_transform_is_lossless(transform) {
        return Err("contains shear and cannot compile losslessly");
    }
    let axes = [
        transform.transform_vector3(Vec3::X),
        transform.transform_vector3(Vec3::Y),
        transform.transform_vector3(Vec3::Z),
    ];
    let mut world_size = [0.0; 3];
    let mut used_world_axes = [false; 3];
    for (local_axis, vector) in axes.into_iter().enumerate() {
        let absolute = vector.abs().to_array();
        let world_axis = absolute
            .iter()
            .enumerate()
            .max_by(|left, right| left.1.total_cmp(right.1))
            .map(|(axis, _)| axis)
            .expect("a 3D axis always has a largest component");
        let magnitude = absolute[world_axis];
        let tolerance = magnitude.max(1.0) * 0.0001;
        if magnitude <= f32::EPSILON
            || absolute
                .iter()
                .enumerate()
                .any(|(axis, value)| axis != world_axis && *value > tolerance)
            || used_world_axes[world_axis]
        {
            return Err("has non-axis-aligned rotation unsupported by the runtime adapter");
        }
        used_world_axes[world_axis] = true;
        world_size[world_axis] = local_size[local_axis] * magnitude;
    }
    Ok(world_size)
}

fn uniform_runtime_scale(transform: Affine3A) -> Result<f32, &'static str> {
    if !runtime_mesh_transform_is_lossless(transform) {
        return Err("contains shear and cannot compile losslessly");
    }
    let scale = affine_transform(transform).scale.map(f32::abs);
    if scale
        .iter()
        .skip(1)
        .any(|value| (*value - scale[0]).abs() > 0.0001)
    {
        return Err("uses non-uniform scale unsupported by the runtime radius adapter");
    }
    Ok(scale[0])
}

fn runtime_ladder_axis(
    transform: Affine3A,
    local_axis: &str,
) -> Result<&'static str, &'static str> {
    let vector = if local_axis.eq_ignore_ascii_case("x") {
        transform.transform_vector3(Vec3::X)
    } else {
        transform.transform_vector3(Vec3::Z)
    };
    let absolute = vector.abs().to_array();
    let world_axis = absolute
        .iter()
        .enumerate()
        .max_by(|left, right| left.1.total_cmp(right.1))
        .map(|(axis, _)| axis)
        .expect("a 3D axis always has a largest component");
    match world_axis {
        0 => Ok("x"),
        2 => Ok("z"),
        _ => Err(
            "ladder rotation maps its climb axis vertically, which the runtime does not support",
        ),
    }
}

fn validate_transform(node: &AuthoringNode) -> Result<(), String> {
    let values = node
        .transform
        .position
        .into_iter()
        .chain(node.transform.rotation)
        .chain(node.transform.scale);
    if values.clone().any(|value| !value.is_finite()) {
        return Err(format!(
            "scene node {} ({}) has a non-finite transform",
            node.id, node.name
        ));
    }
    if node
        .transform
        .scale
        .into_iter()
        .any(|value| value < MIN_AUTHORING_SCALE)
    {
        return Err(format!(
            "scene node {} ({}) scale must be at least {MIN_AUTHORING_SCALE}",
            node.id, node.name
        ));
    }
    Ok(())
}

fn validate_components(node: &AuthoringNode) -> Result<(), String> {
    for (name, value) in &node.components {
        if !matches!(
            name.as_str(),
            "render"
                | "text"
                | "interaction"
                | "primitive"
                | "collision"
                | "ladder"
                | "checkpoint"
                | "hazard"
                | "safeZone"
        ) {
            return Err(format!(
                "scene node {} ({}) uses unsupported component {:?}",
                node.id, node.name, name
            ));
        }
        if !value.is_object() {
            return Err(component_error(node, &format!("{name} must be an object")));
        }
        if name == "render"
            && value
                .get("mesh")
                .and_then(Value::as_str)
                .is_none_or(|mesh| mesh.trim().is_empty())
        {
            return Err(component_error(
                node,
                "render component requires a non-empty mesh asset",
            ));
        }
        if name == "primitive" {
            let shape = value.get("shape").and_then(Value::as_str).unwrap_or("box");
            if shape != "box" {
                return Err(component_error(
                    node,
                    "primitive shape must currently be `box`",
                ));
            }
            let Some(size) = value.get("size").and_then(vector_value) else {
                return Err(component_error(node, "primitive component requires a size"));
            };
            if size
                .iter()
                .any(|value| !value.is_finite() || *value < MIN_AUTHORING_SCALE)
            {
                return Err(component_error(
                    node,
                    "primitive size must contain finite values above the minimum size",
                ));
            }
        }
        if matches!(name.as_str(), "ladder" | "hazard") {
            let Some(size) = value.get("size").and_then(vector_value) else {
                return Err(component_error(
                    node,
                    &format!("{name} component requires a size"),
                ));
            };
            if size
                .iter()
                .any(|value| !value.is_finite() || *value < MIN_AUTHORING_SCALE)
            {
                return Err(component_error(
                    node,
                    &format!("{name} size must contain finite values above the minimum size"),
                ));
            }
        }
        if matches!(name.as_str(), "checkpoint" | "safeZone")
            && value.get("radius").is_some_and(|radius| {
                radius
                    .as_f64()
                    .is_none_or(|radius| !radius.is_finite() || radius <= 0.0)
            })
        {
            return Err(component_error(
                node,
                &format!("{name} component requires a finite positive radius"),
            ));
        }
        if name == "collision"
            && value
                .get("kind")
                .and_then(Value::as_str)
                .is_some_and(|kind| kind != "box")
        {
            return Err(component_error(
                node,
                "collision kind must currently be `box`",
            ));
        }
    }
    Ok(())
}

fn component_error(node: &AuthoringNode, message: &str) -> String {
    format!("scene node {} ({}) {message}", node.id, node.name)
}

fn copy_component_value(source: &Map<String, Value>, target: &mut Map<String, Value>, key: &str) {
    if let Some(value) = source.get(key) {
        target.insert(key.to_owned(), value.clone());
    }
}

fn vector_value(value: &Value) -> Option<[f32; 3]> {
    let values = value.as_array()?;
    Some([
        values.first()?.as_f64()? as f32,
        values.get(1)?.as_f64()? as f32,
        values.get(2)?.as_f64()? as f32,
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(id: &str, parent_id: Option<&str>) -> AuthoringNode {
        AuthoringNode {
            id: id.to_owned(),
            parent_id: parent_id.map(str::to_owned),
            name: id.to_owned(),
            transform: Transform::default(),
            components: BTreeMap::new(),
            editor: EditorMetadata::default(),
            source: None,
        }
    }

    #[test]
    fn duplicate_ids_are_rejected() {
        let mut scene = AuthoringScene {
            format_version: 1,
            world_id: None,
            nodes: vec![node("same", None), node("same", None)],
        };
        let error = scene.validate().unwrap_err();
        assert!(error.contains("same"));
        scene.nodes[1].id = "other".to_owned();
        assert!(scene.validate().is_ok());
    }

    #[test]
    fn missing_parent_and_cycle_are_rejected() {
        let mut scene = AuthoringScene {
            format_version: 1,
            world_id: None,
            nodes: vec![node("child", Some("missing"))],
        };
        assert!(scene.validate().unwrap_err().contains("missing parent"));
        scene.nodes = vec![node("a", Some("b")), node("b", Some("a"))];
        assert!(scene.validate().unwrap_err().contains("cycle"));
    }

    #[test]
    fn serialization_is_deterministic_and_position_is_undoable_by_caller() {
        let scene = AuthoringScene {
            format_version: 1,
            world_id: None,
            nodes: vec![node("root", None)],
        };
        let first = serialize_authoring_scene(&scene).unwrap();
        let reloaded = parse_authoring_scene(&first).unwrap();
        let second = serialize_authoring_scene(&reloaded).unwrap();
        assert_eq!(first, second);
        let mut edited = reloaded.clone();
        let old = edited.set_position("root", [5.0, 0.0, 0.0]).unwrap();
        assert_eq!(old, [0.0; 3]);
        assert_eq!(
            edited.node("root").unwrap().transform.position,
            [5.0, 0.0, 0.0]
        );
        assert!(
            edited
                .set_scale("root", [MIN_AUTHORING_SCALE - 0.01, 1.0, 1.0])
                .is_err()
        );
    }

    #[test]
    fn serialization_normalizes_authoring_float_noise() {
        let mut root = node("root", None);
        root.transform.position = [1.2000000476837158, 3.812197685241699, -0.0];
        root.components.insert(
            "interaction".to_owned(),
            json!({ "id": "use", "radius": 3.812197685241699 }),
        );
        let source = serialize_authoring_scene(&AuthoringScene {
            format_version: 1,
            world_id: None,
            nodes: vec![root],
        })
        .unwrap();

        assert!(source.contains("1.2"));
        assert!(source.contains("3.812198"));
        assert!(!source.contains("1.2000000476837158"));
        assert!(!source.contains("3.812197685241699"));
    }

    #[test]
    fn world_resize_converts_position_and_scale_back_to_local_parent_space() {
        let mut parent = node("parent", None);
        parent.transform.scale = [2.0, 3.0, 4.0];
        let child = node("child", Some("parent"));
        let scene = AuthoringScene {
            format_version: 1,
            world_id: None,
            nodes: vec![parent, child],
        };
        let (position, scale) = scene
            .local_transform_for_world("child", [8.0, 6.0, 12.0], [4.0, 6.0, 8.0])
            .unwrap();
        assert_eq!(position, [4.0, 2.0, 3.0]);
        assert_eq!(scale, [2.0, 2.0, 2.0]);
    }

    #[test]
    fn mesh_compile_rejects_non_uniform_parent_scale_with_child_rotation() {
        let mut parent = node("parent", None);
        parent.transform.scale = [2.0, 1.0, 1.0];
        let mut child = node("child", Some("parent"));
        child.transform.rotation[1] = 0.5;
        child
            .components
            .insert("render".to_owned(), json!({ "mesh": "chair" }));
        let scene = AuthoringScene {
            format_version: 1,
            world_id: None,
            nodes: vec![parent, child],
        };
        let mut manifest = json!({
            "id": "game",
            "version": "0.1.0",
            "sdkVersion": "0.6.0",
            "startWorld": "world",
            "worlds": { "world": {} }
        });
        let error = scene.compile_into_manifest(&mut manifest).unwrap_err();
        assert!(error.contains("cannot compile losslessly"));
    }

    #[test]
    fn primitive_box_compiles_to_a_runtime_block() {
        let mut block = node("block", None);
        block.transform.position = [2.0, 1.0, -3.0];
        block.components.insert(
            "primitive".to_owned(),
            json!({
                "shape": "box",
                "size": [4.0, 1.0, 4.0],
                "material": "signal"
            }),
        );
        block
            .components
            .insert("collision".to_owned(), json!({ "kind": "box" }));
        let scene = AuthoringScene {
            format_version: 1,
            world_id: Some("world".to_owned()),
            nodes: vec![block],
        };
        let mut manifest = json!({
            "id": "game",
            "version": "0.1.0",
            "sdkVersion": "0.6.0",
            "startWorld": "world",
            "worlds": { "world": {} }
        });
        scene.compile_into_manifest(&mut manifest).unwrap();
        assert_eq!(manifest["worlds"]["world"]["blocks"][0]["id"], "block");
        assert_eq!(
            manifest["worlds"]["world"]["blocks"][0]["position"],
            json!([2.0, 1.0, -3.0])
        );
        assert_eq!(manifest["worlds"]["world"]["blocks"][0]["color"], "signal");
    }

    #[test]
    fn primitive_runtime_id_overrides_the_authoring_node_id() {
        let mut block = node("block-platform", None);
        block.transform.position = [2.0, 1.0, -3.0];
        block.components.insert(
            "primitive".to_owned(),
            json!({
                "shape": "box",
                "size": [4.0, 1.0, 4.0],
                "runtimeId": "platform"
            }),
        );
        let scene = AuthoringScene {
            format_version: 1,
            world_id: Some("world".to_owned()),
            nodes: vec![block],
        };
        let mut manifest = json!({
            "startWorld": "world",
            "worlds": { "world": {} }
        });

        scene.compile_into_manifest(&mut manifest).unwrap();

        assert_eq!(manifest["worlds"]["world"]["blocks"][0]["id"], "platform");
    }

    #[test]
    fn primitive_box_bakes_parent_scale_into_runtime_size() {
        let mut group = node("group", None);
        group.transform.scale = [2.0, 2.0, 2.0];
        let mut block = node("block", Some("group"));
        block.components.insert(
            "primitive".to_owned(),
            json!({ "shape": "box", "size": [4.0, 1.0, 4.0] }),
        );
        let scene = AuthoringScene {
            format_version: 1,
            world_id: Some("world".to_owned()),
            nodes: vec![group, block],
        };
        let mut manifest = json!({ "startWorld": "world", "worlds": { "world": {} } });

        scene.compile_into_manifest(&mut manifest).unwrap();

        assert_eq!(
            manifest["worlds"]["world"]["blocks"][0]["size"],
            json!([8.0, 2.0, 8.0])
        );
    }

    #[test]
    fn primitive_box_rejects_non_axis_aligned_rotation_and_shear() {
        let mut rotated = node("rotated", None);
        rotated.transform.rotation[1] = 0.25;
        rotated.components.insert(
            "primitive".to_owned(),
            json!({ "shape": "box", "size": [4.0, 1.0, 4.0] }),
        );
        let mut manifest = json!({ "startWorld": "world", "worlds": { "world": {} } });
        let error = AuthoringScene {
            format_version: 1,
            world_id: Some("world".to_owned()),
            nodes: vec![rotated],
        }
        .compile_into_manifest(&mut manifest)
        .unwrap_err();
        assert!(error.contains("non-axis-aligned rotation"));

        let mut parent = node("parent", None);
        parent.transform.scale = [2.0, 1.0, 1.0];
        let mut sheared = node("sheared", Some("parent"));
        sheared.transform.rotation[1] = 0.5;
        sheared.components.insert(
            "primitive".to_owned(),
            json!({ "shape": "box", "size": [4.0, 1.0, 4.0] }),
        );
        let error = AuthoringScene {
            format_version: 1,
            world_id: Some("world".to_owned()),
            nodes: vec![parent, sheared],
        }
        .compile_into_manifest(&mut manifest)
        .unwrap_err();
        assert!(error.contains("shear"));
    }

    #[test]
    fn compiling_a_scene_without_blocks_clears_stale_runtime_blocks() {
        let scene = AuthoringScene {
            format_version: 1,
            world_id: Some("world".to_owned()),
            nodes: vec![node("world", None)],
        };
        let mut manifest = json!({
            "startWorld": "world",
            "worlds": {
                "world": {
                    "blocks": [{ "id": "stale" }]
                }
            }
        });

        scene.compile_into_manifest(&mut manifest).unwrap();

        assert_eq!(manifest["worlds"]["world"]["blocks"], json!([]));
    }

    #[test]
    fn serialization_omits_implicit_optional_metadata() {
        let source = r#"{
  "formatVersion": 1,
  "nodes": [{
    "id": "root",
    "name": "Root",
    "transform": {
      "position": [0, 0, 0],
      "rotation": [0, 0, 0],
      "scale": [1, 1, 1]
    },
    "components": {},
    "editor": {
      "visible": true,
      "locked": false
    },
    "source": {
      "format": "roblox"
    }
  }]
}"#;

        let scene = parse_authoring_scene(source).unwrap();
        let rendered = serialize_authoring_scene(&scene).unwrap();

        assert!(!rendered.contains("\"parentId\""));
        assert!(!rendered.contains("\"lockReason\""));
        assert!(!rendered.contains("\"class\""));
        assert!(!rendered.contains("\"path\""));
        assert!(rendered.contains("\"source\": {"));

        let source = source.replace(
            ",\n    \"source\": {\n      \"format\": \"roblox\"\n    }",
            "",
        );
        let scene_without_source = parse_authoring_scene(&source).unwrap();
        let rendered_without_source = serialize_authoring_scene(&scene_without_source).unwrap();
        assert!(!rendered_without_source.contains("\"source\""));
    }

    #[test]
    fn world_transforms_compose_parent_rotation_scale_and_inverse_position() {
        let mut root = node("root", None);
        root.transform.position = [10.0, 0.0, 4.0];
        root.transform.rotation = [0.0, std::f32::consts::FRAC_PI_2, 0.0];
        root.transform.scale = [2.0, 2.0, 2.0];
        let mut child = node("child", Some("root"));
        child.transform.position = [1.0, 0.0, 0.0];
        let scene = AuthoringScene {
            format_version: 1,
            world_id: None,
            nodes: vec![root, child],
        };

        let world = scene.world_transform("child").unwrap();
        assert!((world.position[0] - 10.0).abs() < 0.0001);
        assert!((world.position[2] - 2.0).abs() < 0.0001);
        let local = scene
            .local_position_for_world("child", world.position)
            .unwrap();
        assert!(
            local
                .into_iter()
                .zip([1.0, 0.0, 0.0])
                .all(|(actual, expected)| (actual - expected).abs() < 0.0001)
        );
    }

    #[test]
    fn components_compile_to_the_existing_runtime_collections() {
        let mut mesh = node("mesh", Some("root"));
        mesh.transform.scale = [1.0, 1.5, 0.75];
        mesh.components
            .insert("render".to_owned(), json!({ "mesh": "casino" }));
        let mut sign = node("sign", Some("root"));
        sign.transform.position = [1.0, 2.0, 3.0];
        sign.components.insert(
            "text".to_owned(),
            json!({ "text": "TABLES", "maxWidth": 8, "color": "butter" }),
        );
        let mut interaction = node("interaction", Some("root"));
        interaction.components.insert(
            "interaction".to_owned(),
            json!({ "id": "table", "label": "PLAY", "radius": 7.5 }),
        );
        let mut ladder = node("ladder", Some("root"));
        ladder.components.insert(
            "ladder".to_owned(),
            json!({ "id": "up", "size": [2, 6, 1], "climbAxis": "z" }),
        );
        let mut checkpoint = node("checkpoint", Some("root"));
        checkpoint.components.insert(
            "checkpoint".to_owned(),
            json!({ "id": "save", "radius": 3 }),
        );
        let mut hazard = node("hazard", Some("root"));
        hazard.components.insert(
            "hazard".to_owned(),
            json!({ "id": "hurt", "kind": "damage", "size": [4, 1, 4], "damagePerSecond": 10 }),
        );
        let mut safe_zone = node("safe", Some("root"));
        safe_zone.components.insert(
            "safeZone".to_owned(),
            json!({ "id": "camp", "radius": 5, "healPerSecond": 4 }),
        );
        let scene = AuthoringScene {
            format_version: 1,
            world_id: None,
            nodes: vec![
                node("root", None),
                mesh,
                sign,
                interaction,
                ladder,
                checkpoint,
                hazard,
                safe_zone,
            ],
        };
        let mut manifest = json!({
            "startWorld": "world",
            "worlds": { "world": {} }
        });
        scene.compile_into_manifest(&mut manifest).unwrap();
        let world = &manifest["worlds"]["world"];
        assert_eq!(world["decorations"][0]["asset"], "casino");
        assert_eq!(world["decorations"][0]["scale3"], json!([1.0, 1.5, 0.75]));
        assert_eq!(world["signs"][0]["text"], "TABLES");
        assert_eq!(world["interactions"][0]["id"], "table");
        assert_eq!(world["ladders"][0]["id"], "up");
        assert_eq!(world["checkpoints"][0]["id"], "save");
        assert_eq!(world["hazards"][0]["damagePerSecond"], 10);
        assert_eq!(world["safeZones"][0]["healPerSecond"], 4);
    }
}
