//! The project-owned authoring scene format.
//!
//! This is deliberately an entity/component document rather than a copy of
//! Roblox's class hierarchy.  The builder is the adapter between this
//! editable source and the compact runtime manifest.

use glam::{Affine3A, EulerRot, Quat, Vec3};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, BTreeSet};

pub const AUTHORING_SCENE_FORMAT_VERSION: u32 = 1;

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
    serde_json::to_string_pretty(scene)
        .map(|source| format!("{source}\n"))
        .map_err(|error| format!("could not serialize scene.json: {error}"))
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
            .any(|value| !value.is_finite() || *value <= 0.0)
        {
            return Err(format!(
                "scene node {id} scale must contain positive finite values"
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

        let mut decorations = Vec::new();
        let mut signs = Vec::new();
        let mut interactions = Vec::new();
        let nodes = self.index()?;
        let mut world_cache = BTreeMap::new();
        for node in &self.nodes {
            if !node.editor.visible {
                continue;
            }
            let world = self.world_affine(&node.id, &nodes, &mut world_cache)?;
            let world_transform = affine_transform(world);
            if let Some(render) = node.components.get("render") {
                let render = render
                    .as_object()
                    .ok_or_else(|| component_error(node, "render must be an object"))?;
                if let Some(mesh) = render.get("mesh").and_then(Value::as_str) {
                    let scale = world_transform.scale;
                    if world_transform.rotation[0].abs() > 0.0001
                        || world_transform.rotation[2].abs() > 0.0001
                    {
                        return Err(component_error(
                            node,
                            "render rotation is limited to the runtime mesh adapter's Y axis",
                        ));
                    }
                    decorations.push(json!({
                        "kind": "mesh",
                        "asset": mesh,
                        "position": world_transform.position,
                        "scale": scale[0],
                        "scale3": scale,
                        "yaw": world_transform.rotation[1],
                        "color": render.get("color").cloned().unwrap_or_else(|| json!("#FFFFFF")),
                    }));
                }
            }
            if let Some(text) = node.components.get("text") {
                let text = text
                    .as_object()
                    .ok_or_else(|| component_error(node, "text must be an object"))?;
                let mut sign = Map::new();
                sign.insert(
                    "text".to_owned(),
                    text.get("text")
                        .cloned()
                        .unwrap_or_else(|| json!(node.name)),
                );
                sign.insert("position".to_owned(), json!(world_transform.position));
                copy_component_value(text, &mut sign, "maxWidth");
                copy_component_value(text, &mut sign, "color");
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
                for key in ["radius", "color", "visual"] {
                    copy_component_value(interaction, &mut output, key);
                }
                interactions.push(Value::Object(output));
            }
        }
        world.insert("decorations".to_owned(), Value::Array(decorations));
        world.insert("signs".to_owned(), Value::Array(signs));
        world.insert("interactions".to_owned(), Value::Array(interactions));
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
    Ok(())
}

fn validate_components(node: &AuthoringNode) -> Result<(), String> {
    for (name, value) in &node.components {
        if !matches!(name.as_str(), "render" | "text" | "interaction") {
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
        let scene = AuthoringScene {
            format_version: 1,
            world_id: None,
            nodes: vec![node("root", None), mesh, sign, interaction],
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
    }
}
