use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

pub const AUTHORING_SCENE_FORMAT_VERSION: u32 = 1;
pub const MIN_AUTHORING_SCALE: f32 = 0.05;

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
