use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::PathBuf;

pub struct ImportOptions {
    pub place_path: PathBuf,
    pub terrain_path: Option<PathBuf>,
    pub project_path: Option<PathBuf>,
    pub output_path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportResult {
    pub output: PathBuf,
    pub geometry_count: usize,
    pub visible_geometry_count: usize,
    pub camera_count: usize,
    pub light_count: usize,
    pub texture_count: usize,
    pub text_count: usize,
    pub has_terrain_payload: bool,
}

#[derive(Debug, Clone)]
pub struct MeshExportOptions {
    pub scene_path: PathBuf,
    pub output_path: PathBuf,
    pub path_prefixes: Vec<String>,
    pub exclude_paths: Vec<String>,
    pub exclude_exact_paths: Vec<String>,
    pub instance_root: Option<String>,
    pub local_space: bool,
    pub scale: f32,
    pub origin: [f32; 3],
    pub collision_output: Option<PathBuf>,
    pub bounds_output: Option<PathBuf>,
    pub mesh_overrides: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct MeshExportResult {
    pub output: PathBuf,
    pub geometry_count: usize,
    pub vertex_count: usize,
    pub triangle_count: usize,
    pub bounds: Bounds,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReferenceScene {
    pub format_version: u32,
    pub kind: String,
    pub coordinate_system: String,
    pub source: SourceSet,
    pub summary: SceneSummary,
    pub bounds: Option<Bounds>,
    pub class_counts: BTreeMap<String, usize>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub instances: Vec<ReferenceInstance>,
    pub geometry: Vec<GeometryInstance>,
    pub cameras: Vec<CameraInstance>,
    pub lights: Vec<LightInstance>,
    pub textures: Vec<SurfaceTexture>,
    pub texts: Vec<TextInstance>,
    pub spawns: Vec<SpawnInstance>,
    pub project_lighting: Option<ProjectLighting>,
    pub terrain: Option<TerrainSource>,
}

/// The normalized source hierarchy record.  Geometry and presentation facts
/// stay in their specialized arrays; this record preserves the source tree so
/// native authoring tools do not have to infer parentage from mesh batches.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReferenceInstance {
    pub path: String,
    pub parent_path: String,
    pub class: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transform: Option<Transform>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceSet {
    pub place: SourceFile,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub terrain: Option<SourceFile>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project: Option<SourceFile>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceFile {
    pub name: String,
    pub bytes: u64,
    pub sha256: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SceneSummary {
    pub instance_count: usize,
    pub geometry_count: usize,
    pub visible_geometry_count: usize,
    pub camera_count: usize,
    pub light_count: usize,
    pub texture_count: usize,
    pub text_count: usize,
    pub spawn_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bounds {
    pub minimum: [f32; 3],
    pub maximum: [f32; 3],
}

/// The inferred authoring frame for a source model whose Roblox XML does not
/// carry a model CFrame. Reference geometry stores world-space transforms, so
/// the first descendant geometry provides a deterministic pivot and basis for
/// extracting a reusable local-space asset.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ReferenceInstanceFrame {
    pub position: [f32; 3],
    pub rotation: [[f32; 3]; 3],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transform {
    pub position: [f32; 3],
    /// Roblox CFrame rotation matrix, stored as three source rows.
    pub rotation: [[f32; 3]; 3],
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeometryInstance {
    pub path: String,
    pub parent_path: String,
    pub class: String,
    pub name: String,
    pub transform: Transform,
    pub size: [f32; 3],
    pub color: [f32; 3],
    pub material: Material,
    pub transparency: f32,
    pub reflectance: f32,
    pub anchored: bool,
    pub can_collide: bool,
    pub cast_shadow: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shape: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mesh: Option<MeshReference>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub opaque_properties: Vec<OpaqueProperty>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Material {
    pub value: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MeshReference {
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mesh_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub texture_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mesh_type: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scale: Option<[f32; 3]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offset: Option<[f32; 3]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vertex_color: Option<[f32; 3]>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpaqueProperty {
    pub name: String,
    pub kind: String,
    pub bytes: usize,
    pub sha256: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CameraInstance {
    pub path: String,
    pub name: String,
    pub transform: Transform,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub focus: Option<Transform>,
    pub field_of_view: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub camera_type: Option<u32>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LightInstance {
    pub path: String,
    pub parent_path: String,
    pub class: String,
    pub name: String,
    pub color: [f32; 3],
    pub brightness: f32,
    pub range: f32,
    pub enabled: bool,
    pub shadows: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub face: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub angle: Option<f32>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SurfaceTexture {
    pub path: String,
    pub parent_path: String,
    pub class: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub asset: Option<String>,
    pub color: [f32; 3],
    pub transparency: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub face: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub studs_per_tile_u: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub studs_per_tile_v: Option<f32>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextInstance {
    pub path: String,
    pub parent_path: String,
    pub class: String,
    pub name: String,
    pub text: String,
    pub color: [f32; 3],
    pub transparency: f32,
    pub text_scaled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub font: Option<u32>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SpawnInstance {
    pub path: String,
    pub name: String,
    pub transform: Transform,
    pub size: [f32; 3],
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectLighting {
    pub properties: BTreeMap<String, Value>,
    pub effects: Vec<ProjectEffect>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ProjectEffect {
    pub name: String,
    pub class: String,
    pub properties: BTreeMap<String, Value>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerrainSource {
    pub properties: BTreeMap<String, Value>,
    pub opaque_properties: Vec<OpaqueProperty>,
    pub requires_voxel_decoder: bool,
}
