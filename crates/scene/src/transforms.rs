use crate::{
    AuthoringNode, AuthoringScene, AuthoringWorldTransform, MIN_AUTHORING_SCALE, Transform,
};
use glam::{Affine3A, EulerRot, Quat, Vec3};
use std::collections::BTreeMap;

impl AuthoringScene {
    pub(crate) fn index(&self) -> Result<BTreeMap<&str, &AuthoringNode>, String> {
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

    pub(crate) fn world_affine<'a>(
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

    /// Moves a node below a different parent without changing its world-space
    /// placement. Reparenting is rejected when the new hierarchy would cycle
    /// or when the resulting local transform would contain shear.
    pub fn reparent_preserving_world_transform(
        &mut self,
        id: &str,
        parent_id: &str,
    ) -> Result<Option<String>, String> {
        if id == parent_id {
            return Err(format!("scene node {id} cannot be its own parent"));
        }
        let node = self
            .node(id)
            .ok_or_else(|| format!("scene node {id} was not found"))?;
        if node.editor.locked {
            return Err(format!("scene node {id} ({}) is locked", node.name));
        }
        if node.parent_id.is_none() {
            return Err(format!("scene root node {id} cannot be reparented"));
        }
        if self.node(parent_id).is_none() {
            return Err(format!("scene parent node {parent_id} was not found"));
        }
        let mut ancestor = Some(parent_id);
        while let Some(candidate) = ancestor {
            if candidate == id {
                return Err(format!(
                    "scene node {id} cannot be parented below one of its descendants"
                ));
            }
            ancestor = self
                .node(candidate)
                .and_then(|node| node.parent_id.as_deref());
        }

        let nodes = self.index()?;
        let mut cache = BTreeMap::new();
        let world = self.world_affine(id, &nodes, &mut cache)?;
        let parent_world = self.world_affine(parent_id, &nodes, &mut cache)?;
        let local = parent_world.inverse() * world;
        if !runtime_mesh_transform_is_lossless(local) {
            return Err(format!(
                "moving scene node {id} there would distort its shape; reset non-uniform scale or rotation on the object or destination group and try again"
            ));
        }
        let local = affine_transform(local);
        if local
            .scale
            .iter()
            .any(|value| !value.is_finite() || *value < MIN_AUTHORING_SCALE)
        {
            return Err(format!(
                "scene node {id} reparenting produced an unsupported local scale"
            ));
        }
        let previous = self
            .node(id)
            .expect("scene node existed before reparenting")
            .clone();
        let replacement = Transform {
            position: local.position,
            rotation: local.rotation,
            scale: local.scale,
        };
        {
            let node = self
                .node_mut(id)
                .expect("scene node existed before reparenting");
            node.parent_id = Some(parent_id.to_owned());
            node.transform = replacement;
        }
        if let Err(error) = self.validate() {
            *self
                .node_mut(id)
                .expect("scene node existed before failed reparenting") = previous.clone();
            return Err(error);
        }
        Ok(previous.parent_id)
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

pub(crate) fn affine_transform(transform: Affine3A) -> AuthoringWorldTransform {
    let (scale, rotation, position) = transform.to_scale_rotation_translation();

    // A transform containing only Y rotation is a common authoring case for
    // imported furniture. glam's general XYZ Euler decomposition can choose
    // an equivalent representation with non-zero X/Z angles for some Y
    // angles. The runtime mesh adapter intentionally supports Y rotation only,
    // so preserve that canonical representation when the affine basis proves
    // that the transform is actually Y-only.
    let x_axis = transform.transform_vector3(Vec3::X);
    let y_axis = transform.transform_vector3(Vec3::Y);
    let z_axis = transform.transform_vector3(Vec3::Z);
    let tolerance = 0.0001;
    if x_axis.y.abs() <= tolerance
        && z_axis.y.abs() <= tolerance
        && y_axis.x.abs() <= tolerance
        && y_axis.z.abs() <= tolerance
        && x_axis.length_squared() > f32::EPSILON
    {
        let yaw = (-x_axis.z).atan2(x_axis.x);
        return AuthoringWorldTransform {
            position: position.to_array(),
            rotation: [0.0, yaw, 0.0],
            scale: scale.to_array(),
        };
    }

    let (x, y, z) = rotation.to_euler(EulerRot::XYZ);
    AuthoringWorldTransform {
        position: position.to_array(),
        rotation: [x, y, z],
        scale: scale.to_array(),
    }
}

pub(crate) fn runtime_mesh_transform_is_lossless(transform: Affine3A) -> bool {
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

pub(crate) fn axis_aligned_runtime_size(
    transform: Affine3A,
    local_size: [f32; 3],
) -> Result<[f32; 3], &'static str> {
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
            return Err(if runtime_mesh_transform_is_lossless(transform) {
                "has non-axis-aligned rotation unsupported by the runtime adapter"
            } else {
                "contains shear and cannot compile losslessly"
            });
        }
        used_world_axes[world_axis] = true;
        world_size[world_axis] = local_size[local_axis] * magnitude;
    }
    Ok(world_size)
}

pub(crate) fn uniform_runtime_scale(transform: Affine3A) -> Result<f32, &'static str> {
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

pub(crate) fn runtime_ladder_axis(
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
