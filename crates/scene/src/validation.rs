use crate::{AUTHORING_SCENE_FORMAT_VERSION, AuthoringNode, AuthoringScene, MIN_AUTHORING_SCALE};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};

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
            if value
                .get("collidable")
                .is_some_and(|collidable| !collidable.is_boolean())
            {
                return Err(component_error(
                    node,
                    "primitive collidable must be a boolean",
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
        if name == "collision" {
            let kind = value.get("kind").and_then(Value::as_str).unwrap_or("box");
            match kind {
                "box" => {}
                "mesh" => {
                    if value
                        .get("asset")
                        .and_then(Value::as_str)
                        .is_none_or(|asset| asset.trim().is_empty())
                    {
                        return Err(component_error(
                            node,
                            "mesh collision requires a non-empty asset",
                        ));
                    }
                }
                _ => {
                    return Err(component_error(
                        node,
                        "collision kind must be `box` or `mesh`",
                    ));
                }
            }
        }
    }
    Ok(())
}

pub(crate) fn component_error(node: &AuthoringNode, message: &str) -> String {
    format!("scene node {} ({}) {message}", node.id, node.name)
}

pub(crate) fn copy_component_value(
    source: &Map<String, Value>,
    target: &mut Map<String, Value>,
    key: &str,
) {
    if let Some(value) = source.get(key) {
        target.insert(key.to_owned(), value.clone());
    }
}

pub(crate) fn vector_value(value: &Value) -> Option<[f32; 3]> {
    let values = value.as_array()?;
    Some([
        values.first()?.as_f64()? as f32,
        values.get(1)?.as_f64()? as f32,
        values.get(2)?.as_f64()? as f32,
    ])
}
