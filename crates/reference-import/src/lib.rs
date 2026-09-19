//! Narrow Roblox XML importer for visual-reference reconstruction.
//!
//! This crate intentionally emits a tool-owned intermediate scene instead of a
//! Cubacadabra package manifest. It preserves source facts first; package and
//! renderer adaptation can then be measured against that stable artifact.

use rbx_dom_weak::types::{CFrame, Color3, ContentType, Matrix3, Variant, Vector3};
use rbx_dom_weak::{Instance, WeakDom, types::Ref, ustr};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, HashMap},
    fs::{self, File},
    io::{BufReader, Write},
    path::{Path, PathBuf},
};

mod collision;

mod mesh_overrides;

#[derive(Debug, Clone)]
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

#[derive(Default)]
struct SceneCollector {
    class_counts: BTreeMap<String, usize>,
    instances: Vec<ReferenceInstance>,
    geometry: Vec<GeometryInstance>,
    cameras: Vec<CameraInstance>,
    lights: Vec<LightInstance>,
    textures: Vec<SurfaceTexture>,
    texts: Vec<TextInstance>,
    spawns: Vec<SpawnInstance>,
    bounds: Option<Bounds>,
    instance_count: usize,
    visible_geometry_count: usize,
}

pub fn import_reference(options: &ImportOptions) -> Result<ImportResult, String> {
    let place_path = existing_file(&options.place_path, "place")?;
    let terrain_path = options
        .terrain_path
        .as_ref()
        .map(|path| existing_file(path, "terrain"))
        .transpose()?;
    let project_path = options
        .project_path
        .as_ref()
        .map(|path| existing_file(path, "project"))
        .transpose()?;

    let place_source = source_file(&place_path)?;
    let terrain_source_file = terrain_path.as_deref().map(source_file).transpose()?;
    let project_source = project_path.as_deref().map(source_file).transpose()?;

    let place = decode_xml(&place_path)?;
    let mut collector = SceneCollector::default();
    walk_children(&place, place.root_ref(), "", &mut collector)?;

    collector.geometry.sort_by(|a, b| a.path.cmp(&b.path));
    collector.instances.sort_by(|a, b| a.path.cmp(&b.path));
    collector.cameras.sort_by(|a, b| a.path.cmp(&b.path));
    collector.lights.sort_by(|a, b| a.path.cmp(&b.path));
    collector.textures.sort_by(|a, b| a.path.cmp(&b.path));
    collector.texts.sort_by(|a, b| a.path.cmp(&b.path));
    collector.spawns.sort_by(|a, b| a.path.cmp(&b.path));

    let terrain = terrain_path.as_deref().map(import_terrain).transpose()?;
    let project_lighting = project_path
        .as_deref()
        .map(import_project_lighting)
        .transpose()?
        .flatten();

    let summary = SceneSummary {
        instance_count: collector.instance_count,
        geometry_count: collector.geometry.len(),
        visible_geometry_count: collector.visible_geometry_count,
        camera_count: collector.cameras.len(),
        light_count: collector.lights.len(),
        texture_count: collector.textures.len(),
        text_count: collector.texts.len(),
        spawn_count: collector.spawns.len(),
    };
    let result = ImportResult {
        output: options.output_path.clone(),
        geometry_count: summary.geometry_count,
        visible_geometry_count: summary.visible_geometry_count,
        camera_count: summary.camera_count,
        light_count: summary.light_count,
        texture_count: summary.texture_count,
        text_count: summary.text_count,
        has_terrain_payload: terrain.is_some(),
    };
    let scene = ReferenceScene {
        format_version: 1,
        kind: "roblox-static-reference-scene".to_owned(),
        coordinate_system:
            "Roblox source coordinates (X right, Y up, CFrame matrix rows preserved)".to_owned(),
        source: SourceSet {
            place: place_source,
            terrain: terrain_source_file,
            project: project_source,
        },
        summary,
        bounds: collector.bounds,
        class_counts: collector.class_counts,
        instances: collector.instances,
        geometry: collector.geometry,
        cameras: collector.cameras,
        lights: collector.lights,
        textures: collector.textures,
        texts: collector.texts,
        spawns: collector.spawns,
        project_lighting,
        terrain,
    };
    write_scene(&options.output_path, &scene)?;
    Ok(result)
}

fn existing_file(path: &Path, label: &str) -> Result<PathBuf, String> {
    path.canonicalize()
        .map_err(|error| format!("could not resolve {label} file {}: {error}", path.display()))
        .and_then(|path| {
            if path.is_file() {
                Ok(path)
            } else {
                Err(format!("{label} path is not a file: {}", path.display()))
            }
        })
}

fn source_file(path: &Path) -> Result<SourceFile, String> {
    let bytes = fs::metadata(path)
        .map_err(|error| format!("could not inspect {}: {error}", path.display()))?
        .len();
    let data =
        fs::read(path).map_err(|error| format!("could not read {}: {error}", path.display()))?;
    Ok(SourceFile {
        name: path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("source")
            .to_owned(),
        bytes,
        sha256: sha256(&data),
    })
}

fn decode_xml(path: &Path) -> Result<WeakDom, String> {
    let reader = BufReader::new(
        File::open(path).map_err(|error| format!("could not open {}: {error}", path.display()))?,
    );
    rbx_xml::from_reader_default(reader)
        .map_err(|error| format!("could not decode Roblox XML {}: {error}", path.display()))
}

fn walk_children(
    dom: &WeakDom,
    parent_ref: Ref,
    parent_path: &str,
    collector: &mut SceneCollector,
) -> Result<(), String> {
    let parent = dom
        .get_by_ref(parent_ref)
        .ok_or_else(|| "Roblox DOM contains a missing parent reference".to_owned())?;
    let mut occurrences: HashMap<(String, String), usize> = HashMap::new();
    for child_ref in parent.children() {
        let child = dom
            .get_by_ref(*child_ref)
            .ok_or_else(|| "Roblox DOM contains a missing child reference".to_owned())?;
        let class = child.class.to_string();
        let key = (class.clone(), child.name.clone());
        let occurrence = occurrences.entry(key).or_insert(0);
        *occurrence += 1;
        let segment = format!(
            "{}:{}[{}]",
            class,
            escape_path_component(&child.name),
            occurrence
        );
        let path = if parent_path.is_empty() {
            segment
        } else {
            format!("{parent_path}/{segment}")
        };
        collect_instance(dom, child, &path, parent_path, collector);
        walk_children(dom, *child_ref, &path, collector)?;
    }
    Ok(())
}

fn collect_instance(
    dom: &WeakDom,
    instance: &Instance,
    path: &str,
    parent_path: &str,
    collector: &mut SceneCollector,
) {
    let class = instance.class.to_string();
    *collector.class_counts.entry(class.clone()).or_insert(0) += 1;
    collector.instance_count += 1;
    collector.instances.push(ReferenceInstance {
        path: path.to_owned(),
        parent_path: parent_path.to_owned(),
        class: class.clone(),
        name: instance.name.clone(),
        transform: cframe_property(instance, &["CFrame"]).map(transform),
    });

    if let (Some(cframe), Some(size)) = (
        cframe_property(instance, &["CFrame"]),
        vector3_property(instance, &["Size", "size"]),
    ) {
        let transparency = number_property(instance, &["Transparency"]).unwrap_or(0.0);
        let geometry = GeometryInstance {
            path: path.to_owned(),
            parent_path: parent_path.to_owned(),
            class: class.clone(),
            name: instance.name.clone(),
            transform: transform(cframe),
            size: vector(size),
            color: color_property(instance, &["Color", "Color3uint8"])
                .unwrap_or([0.64, 0.64, 0.64]),
            material: material(instance),
            transparency,
            reflectance: number_property(instance, &["Reflectance"]).unwrap_or(0.0),
            anchored: bool_property(instance, &["Anchored"]).unwrap_or(false),
            can_collide: bool_property(instance, &["CanCollide"]).unwrap_or(true),
            cast_shadow: bool_property(instance, &["CastShadow"]).unwrap_or(true),
            shape: enum_property(instance, &["Shape", "shape"]),
            mesh: mesh_reference(dom, instance),
            opaque_properties: opaque_properties(instance),
        };
        if transparency < 1.0 && size.x > 0.0 && size.y > 0.0 && size.z > 0.0 {
            extend_bounds(&mut collector.bounds, cframe, size);
            collector.visible_geometry_count += 1;
        }
        if class == "SpawnLocation" {
            collector.spawns.push(SpawnInstance {
                path: path.to_owned(),
                name: instance.name.clone(),
                transform: transform(cframe),
                size: vector(size),
            });
        }
        collector.geometry.push(geometry);
    }

    if class == "Camera"
        && let Some(cframe) = cframe_property(instance, &["CFrame"])
    {
        collector.cameras.push(CameraInstance {
            path: path.to_owned(),
            name: instance.name.clone(),
            transform: transform(cframe),
            focus: cframe_property(instance, &["Focus"]).map(transform),
            field_of_view: number_property(instance, &["FieldOfView"]).unwrap_or(70.0),
            camera_type: enum_property(instance, &["CameraType"]),
        });
    }

    if matches!(class.as_str(), "PointLight" | "SpotLight" | "SurfaceLight") {
        collector.lights.push(LightInstance {
            path: path.to_owned(),
            parent_path: parent_path.to_owned(),
            class: class.clone(),
            name: instance.name.clone(),
            color: color_property(instance, &["Color"]).unwrap_or([1.0, 1.0, 1.0]),
            brightness: number_property(instance, &["Brightness"]).unwrap_or(1.0),
            range: number_property(instance, &["Range"]).unwrap_or(8.0),
            enabled: bool_property(instance, &["Enabled"]).unwrap_or(true),
            shadows: bool_property(instance, &["Shadows"]).unwrap_or(false),
            face: enum_property(instance, &["Face"]),
            angle: number_property(instance, &["Angle"]),
        });
    }

    if matches!(class.as_str(), "Texture" | "Decal") {
        collector.textures.push(SurfaceTexture {
            path: path.to_owned(),
            parent_path: parent_path.to_owned(),
            class: class.clone(),
            name: instance.name.clone(),
            asset: content_property(instance, &["Texture", "TextureContent"]),
            color: color_property(instance, &["Color3", "Color"]).unwrap_or([1.0, 1.0, 1.0]),
            transparency: number_property(instance, &["Transparency"]).unwrap_or(0.0),
            face: enum_property(instance, &["Face"]),
            studs_per_tile_u: number_property(instance, &["StudsPerTileU"]),
            studs_per_tile_v: number_property(instance, &["StudsPerTileV"]),
        });
    }

    if matches!(class.as_str(), "TextLabel" | "TextButton" | "TextBox")
        && let Some(text) = string_property(instance, &["Text"])
    {
        collector.texts.push(TextInstance {
            path: path.to_owned(),
            parent_path: parent_path.to_owned(),
            class,
            name: instance.name.clone(),
            text,
            color: color_property(instance, &["TextColor3"]).unwrap_or([1.0, 1.0, 1.0]),
            transparency: number_property(instance, &["TextTransparency"]).unwrap_or(0.0),
            text_scaled: bool_property(instance, &["TextScaled"]).unwrap_or(false),
            font: enum_property(instance, &["Font"]),
        });
    }
}

fn mesh_reference(dom: &WeakDom, instance: &Instance) -> Option<MeshReference> {
    if instance.class.as_str() == "MeshPart" {
        return Some(mesh_from_instance(instance));
    }
    for child_ref in instance.children() {
        let child = dom.get_by_ref(*child_ref)?;
        if matches!(
            child.class.as_str(),
            "SpecialMesh" | "BlockMesh" | "CylinderMesh"
        ) {
            return Some(mesh_from_instance(child));
        }
    }
    None
}

fn mesh_from_instance(instance: &Instance) -> MeshReference {
    MeshReference {
        kind: instance.class.to_string(),
        mesh_id: content_property(instance, &["MeshId", "MeshID", "MeshContent"]),
        texture_id: content_property(instance, &["TextureId", "TextureID", "TextureContent"]),
        mesh_type: enum_property(instance, &["MeshType"]),
        scale: vector3_property(instance, &["Scale"]).map(vector),
        offset: vector3_property(instance, &["Offset"]).map(vector),
        vertex_color: vector3_property(instance, &["VertexColor"]).map(vector),
    }
}

fn material(instance: &Instance) -> Material {
    let value = enum_property(instance, &["Material"]).unwrap_or(256);
    Material {
        value,
        name: material_name(value).map(str::to_owned),
    }
}

fn material_name(value: u32) -> Option<&'static str> {
    Some(match value {
        256 => "Plastic",
        272 => "SmoothPlastic",
        288 => "Neon",
        512 => "Wood",
        528 => "WoodPlanks",
        784 => "Marble",
        800 => "Slate",
        816 => "Concrete",
        832 => "Granite",
        848 => "Brick",
        864 => "Pebble",
        880 => "Cobblestone",
        896 => "Rock",
        1040 => "CorrodedMetal",
        1056 => "DiamondPlate",
        1072 => "Foil",
        1088 => "Metal",
        1280 => "Grass",
        1296 => "Sand",
        1312 => "Fabric",
        1536 => "Ice",
        1568 => "Snow",
        _ => return None,
    })
}

fn opaque_properties(instance: &Instance) -> Vec<OpaqueProperty> {
    const GEOMETRY_BLOBS: &[&str] = &[
        "AssetId",
        "ChildData",
        "MeshData",
        "PhysicsData",
        "PhysicalConfigData",
    ];
    let mut output = Vec::new();
    for name in GEOMETRY_BLOBS {
        let Some(value) = property(instance, &[*name]) else {
            continue;
        };
        if let Some(property) = opaque_property(name, value) {
            output.push(property);
        }
    }
    output.sort_by(|a, b| a.name.cmp(&b.name));
    output
}

fn opaque_property(name: &str, value: &Variant) -> Option<OpaqueProperty> {
    let (kind, bytes) = match value {
        Variant::BinaryString(value) => ("BinaryString", value.as_ref()),
        Variant::SharedString(value) => ("SharedString", value.data()),
        _ => return None,
    };
    if bytes.is_empty() {
        return None;
    }
    Some(OpaqueProperty {
        name: name.to_owned(),
        kind: kind.to_owned(),
        bytes: bytes.len(),
        sha256: sha256(bytes),
    })
}

fn import_terrain(path: &Path) -> Result<TerrainSource, String> {
    let dom = decode_xml(path)?;
    let terrain = find_first_class(&dom, dom.root_ref(), "Terrain")
        .ok_or_else(|| format!("no Terrain instance found in {}", path.display()))?;
    let mut properties = BTreeMap::new();
    let mut opaque = Vec::new();
    for (name, value) in &terrain.properties {
        if let Some(property) = opaque_property(name.as_str(), value) {
            opaque.push(property);
        } else if let Some(value) = variant_to_json(value) {
            properties.insert(name.to_string(), value);
        }
    }
    opaque.sort_by(|a, b| a.name.cmp(&b.name));
    let requires_voxel_decoder = opaque.iter().any(|property| property.name == "SmoothGrid");
    Ok(TerrainSource {
        properties,
        opaque_properties: opaque,
        requires_voxel_decoder,
    })
}

fn find_first_class<'a>(dom: &'a WeakDom, root: Ref, class: &str) -> Option<&'a Instance> {
    let instance = dom.get_by_ref(root)?;
    if instance.class.as_str() == class {
        return Some(instance);
    }
    for child in instance.children() {
        if let Some(found) = find_first_class(dom, *child, class) {
            return Some(found);
        }
    }
    None
}

fn import_project_lighting(path: &Path) -> Result<Option<ProjectLighting>, String> {
    let data = fs::read(path)
        .map_err(|error| format!("could not read project {}: {error}", path.display()))?;
    let project: Value = serde_json::from_slice(&data)
        .map_err(|error| format!("could not parse project {}: {error}", path.display()))?;
    let Some(lighting) = project
        .get("tree")
        .and_then(|tree| tree.get("Lighting"))
        .and_then(Value::as_object)
    else {
        return Ok(None);
    };
    let properties = lighting
        .get("$properties")
        .and_then(Value::as_object)
        .map(sorted_object)
        .unwrap_or_default();
    let mut effects = Vec::new();
    for (name, value) in lighting {
        if name.starts_with('$') {
            continue;
        }
        let Some(effect) = value.as_object() else {
            continue;
        };
        let Some(class) = effect.get("$className").and_then(Value::as_str) else {
            continue;
        };
        effects.push(ProjectEffect {
            name: name.clone(),
            class: class.to_owned(),
            properties: effect
                .get("$properties")
                .and_then(Value::as_object)
                .map(sorted_object)
                .unwrap_or_default(),
        });
    }
    effects.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(Some(ProjectLighting {
        properties,
        effects,
    }))
}

fn sorted_object(object: &serde_json::Map<String, Value>) -> BTreeMap<String, Value> {
    object
        .iter()
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect()
}

fn variant_to_json(value: &Variant) -> Option<Value> {
    match value {
        Variant::Bool(value) => Some(Value::Bool(*value)),
        Variant::Float32(value) => serde_json::Number::from_f64(*value as f64).map(Value::Number),
        Variant::Float64(value) => serde_json::Number::from_f64(*value).map(Value::Number),
        Variant::Int32(value) => Some(Value::Number((*value).into())),
        Variant::Int64(value) => Some(Value::Number((*value).into())),
        Variant::String(value) => Some(Value::String(value.clone())),
        Variant::Enum(value) => Some(Value::Number(value.to_u32().into())),
        Variant::EnumItem(value) => Some(serde_json::json!({
            "type": value.ty,
            "value": value.value,
        })),
        Variant::Color3(value) => Some(serde_json::json!([value.r, value.g, value.b])),
        Variant::Color3uint8(value) => Some(serde_json::json!([value.r, value.g, value.b])),
        Variant::Vector3(value) => Some(serde_json::json!([value.x, value.y, value.z])),
        Variant::ContentId(value) => Some(Value::String(value.as_str().to_owned())),
        Variant::Content(value) => value.as_uri().map(|value| Value::String(value.to_owned())),
        Variant::MaterialColors(value) => serde_json::to_value(value).ok(),
        _ => None,
    }
}

fn property<'a>(instance: &'a Instance, names: &[&str]) -> Option<&'a Variant> {
    names
        .iter()
        .find_map(|name| instance.properties.get(&ustr(name)))
}

fn cframe_property<'a>(instance: &'a Instance, names: &[&str]) -> Option<&'a CFrame> {
    match property(instance, names)? {
        Variant::CFrame(value) => Some(value),
        Variant::OptionalCFrame(Some(value)) => Some(value),
        _ => None,
    }
}

fn vector3_property<'a>(instance: &'a Instance, names: &[&str]) -> Option<&'a Vector3> {
    match property(instance, names)? {
        Variant::Vector3(value) => Some(value),
        _ => None,
    }
}

fn number_property(instance: &Instance, names: &[&str]) -> Option<f32> {
    match property(instance, names)? {
        Variant::Float32(value) => Some(*value),
        Variant::Float64(value) => Some(*value as f32),
        Variant::Int32(value) => Some(*value as f32),
        Variant::Int64(value) => Some(*value as f32),
        _ => None,
    }
}

fn bool_property(instance: &Instance, names: &[&str]) -> Option<bool> {
    match property(instance, names)? {
        Variant::Bool(value) => Some(*value),
        _ => None,
    }
}

fn enum_property(instance: &Instance, names: &[&str]) -> Option<u32> {
    match property(instance, names)? {
        Variant::Enum(value) => Some(value.to_u32()),
        Variant::EnumItem(value) => Some(value.value),
        Variant::Int32(value) => (*value).try_into().ok(),
        Variant::Int64(value) => (*value).try_into().ok(),
        _ => None,
    }
}

fn string_property(instance: &Instance, names: &[&str]) -> Option<String> {
    match property(instance, names)? {
        Variant::String(value) => Some(value.clone()),
        _ => None,
    }
}

fn color_property(instance: &Instance, names: &[&str]) -> Option<[f32; 3]> {
    match property(instance, names)? {
        Variant::Color3(value) => Some(color(*value)),
        Variant::Color3uint8(value) => Some(color((*value).into())),
        _ => None,
    }
}

fn content_property(instance: &Instance, names: &[&str]) -> Option<String> {
    match property(instance, names)? {
        Variant::ContentId(value) => non_empty(value.as_str()),
        Variant::Content(value) => match value.value() {
            ContentType::Uri(value) => non_empty(value),
            _ => None,
        },
        Variant::String(value) => non_empty(value),
        _ => None,
    }
}

fn non_empty(value: &str) -> Option<String> {
    (!value.trim().is_empty()).then(|| value.to_owned())
}

fn transform(value: &CFrame) -> Transform {
    Transform {
        position: vector(&value.position),
        rotation: matrix(&value.orientation),
    }
}

fn vector(value: &Vector3) -> [f32; 3] {
    [value.x, value.y, value.z]
}

fn matrix(value: &Matrix3) -> [[f32; 3]; 3] {
    [vector(&value.x), vector(&value.y), vector(&value.z)]
}

fn color(value: Color3) -> [f32; 3] {
    [value.r, value.g, value.b]
}

fn extend_bounds(bounds: &mut Option<Bounds>, cframe: &CFrame, size: &Vector3) {
    let half = Vector3::new(size.x * 0.5, size.y * 0.5, size.z * 0.5);
    let orientation = &cframe.orientation;
    let extent = Vector3::new(
        orientation.x.x.abs() * half.x
            + orientation.y.x.abs() * half.y
            + orientation.z.x.abs() * half.z,
        orientation.x.y.abs() * half.x
            + orientation.y.y.abs() * half.y
            + orientation.z.y.abs() * half.z,
        orientation.x.z.abs() * half.x
            + orientation.y.z.abs() * half.y
            + orientation.z.z.abs() * half.z,
    );
    let minimum = [
        cframe.position.x - extent.x,
        cframe.position.y - extent.y,
        cframe.position.z - extent.z,
    ];
    let maximum = [
        cframe.position.x + extent.x,
        cframe.position.y + extent.y,
        cframe.position.z + extent.z,
    ];
    match bounds {
        Some(bounds) => {
            for axis in 0..3 {
                bounds.minimum[axis] = bounds.minimum[axis].min(minimum[axis]);
                bounds.maximum[axis] = bounds.maximum[axis].max(maximum[axis]);
            }
        }
        None => *bounds = Some(Bounds { minimum, maximum }),
    }
}

fn escape_path_component(value: &str) -> String {
    value.replace('%', "%25").replace('/', "%2F")
}

fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn write_scene(path: &Path, scene: &ReferenceScene) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            format!(
                "could not create output directory {}: {error}",
                parent.display()
            )
        })?;
    }
    let mut file = File::create(path)
        .map_err(|error| format!("could not create {}: {error}", path.display()))?;
    serde_json::to_writer_pretty(&mut file, scene)
        .map_err(|error| format!("could not encode {}: {error}", path.display()))?;
    file.write_all(b"\n")
        .map_err(|error| format!("could not finish {}: {error}", path.display()))
}

pub fn read_reference_scene(path: impl AsRef<Path>) -> Result<ReferenceScene, String> {
    let path = path.as_ref();
    let data =
        fs::read(path).map_err(|error| format!("could not read {}: {error}", path.display()))?;
    let scene: ReferenceScene = serde_json::from_slice(&data)
        .map_err(|error| format!("could not decode {}: {error}", path.display()))?;
    if scene.kind != "roblox-static-reference-scene" || scene.format_version != 1 {
        return Err(format!(
            "{} is not a supported Roblox reference scene (kind {:?}, version {})",
            path.display(),
            scene.kind,
            scene.format_version
        ));
    }
    Ok(scene)
}

#[derive(Clone, Copy)]
struct StaticMeshVertex {
    position: [f32; 3],
    normal: [f32; 3],
    color: [u8; 4],
}

struct StaticMeshGroup {
    material: String,
    vertices: Vec<StaticMeshVertex>,
}

pub fn export_reference_mesh(options: &MeshExportOptions) -> Result<MeshExportResult, String> {
    if !options.scale.is_finite() || options.scale <= 0.0 {
        return Err("export scale must be finite and positive".to_owned());
    }
    let scene = read_reference_scene(&options.scene_path)?;
    let overrides = mesh_overrides::load(options.mesh_overrides.as_deref())?;
    let selected = scene
        .geometry
        .iter()
        .filter(|geometry| {
            (options.path_prefixes.is_empty()
                || options
                    .path_prefixes
                    .iter()
                    .any(|prefix| geometry.path.starts_with(prefix)))
                && !options
                    .exclude_paths
                    .iter()
                    .any(|path| geometry.path.contains(path))
                && geometry
                    .size
                    .iter()
                    .all(|value| value.is_finite() && *value > 0.0)
        })
        .collect::<Vec<_>>();
    if selected.is_empty() {
        return Err("no visible reference geometry matched the requested path prefix".to_owned());
    }

    let mut groups = BTreeMap::<String, Vec<StaticMeshVertex>>::new();
    let mut collision_triangles = Vec::<[[f32; 3]; 3]>::new();
    for geometry in &selected {
        let mut vertices = Vec::new();
        if let Some(mesh) = geometry
            .mesh
            .as_ref()
            .and_then(|mesh| mesh.mesh_id.as_ref())
            .and_then(|id| overrides.get(id))
        {
            mesh.append(&mut vertices, geometry);
        } else {
            append_static_geometry(&mut vertices, geometry);
        }
        for vertex in &mut vertices {
            vertex.position = scale3(sub3(vertex.position, options.origin), options.scale);
        }
        if options.collision_output.is_some() && geometry.can_collide {
            collision_triangles.extend(vertices.chunks_exact(3).map(|triangle| {
                [
                    triangle[0].position,
                    triangle[1].position,
                    triangle[2].position,
                ]
            }));
        }
        if geometry.transparency >= 0.99 {
            continue;
        }
        let material = geometry
            .material
            .name
            .clone()
            .unwrap_or_else(|| format!("Material({})", geometry.material.value));
        groups.entry(material).or_default().extend(vertices);
    }
    let groups = groups
        .into_iter()
        .filter(|(_, vertices)| !vertices.is_empty())
        .map(|(material, vertices)| StaticMeshGroup { material, vertices })
        .collect::<Vec<_>>();
    if groups.is_empty() {
        return Err("the selected reference geometry produced no drawable triangles".to_owned());
    }
    let vertex_count = groups
        .iter()
        .map(|group| group.vertices.len())
        .sum::<usize>();
    let bounds = mesh_bounds(&groups)?;
    write_static_glb(&options.output_path, &groups)?;
    if let Some(path) = &options.collision_output {
        collision::write_file(path, &collision_triangles)?;
    }
    if let Some(path) = &options.bounds_output {
        write_mesh_bounds(path, &bounds)?;
    }
    Ok(MeshExportResult {
        output: options.output_path.clone(),
        geometry_count: selected.len(),
        vertex_count,
        triangle_count: vertex_count / 3,
        bounds,
    })
}

fn mesh_bounds(groups: &[StaticMeshGroup]) -> Result<Bounds, String> {
    let mut bounds: Option<Bounds> = None;
    for vertex in groups.iter().flat_map(|group| &group.vertices) {
        for axis in 0..3 {
            if !vertex.position[axis].is_finite() {
                return Err("exported mesh contains a non-finite vertex".to_owned());
            }
        }
        match &mut bounds {
            Some(bounds) => {
                for axis in 0..3 {
                    bounds.minimum[axis] = bounds.minimum[axis].min(vertex.position[axis]);
                    bounds.maximum[axis] = bounds.maximum[axis].max(vertex.position[axis]);
                }
            }
            None => {
                bounds = Some(Bounds {
                    minimum: vertex.position,
                    maximum: vertex.position,
                });
            }
        }
    }
    bounds.ok_or_else(|| "exported mesh has no vertices".to_owned())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct MeshBoundsFile {
    format_version: u32,
    minimum: [f32; 3],
    maximum: [f32; 3],
    size: [f32; 3],
}

fn write_mesh_bounds(path: &Path, bounds: &Bounds) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            format!(
                "could not create bounds directory {}: {error}",
                parent.display()
            )
        })?;
    }
    let size = [
        bounds.maximum[0] - bounds.minimum[0],
        bounds.maximum[1] - bounds.minimum[1],
        bounds.maximum[2] - bounds.minimum[2],
    ];
    let file = MeshBoundsFile {
        format_version: 1,
        minimum: bounds.minimum,
        maximum: bounds.maximum,
        size,
    };
    let output = serde_json::to_string_pretty(&file)
        .map_err(|error| format!("could not encode mesh bounds: {error}"))?;
    fs::write(path, format!("{output}\n"))
        .map_err(|error| format!("could not write mesh bounds {}: {error}", path.display()))
}

fn append_static_geometry(vertices: &mut Vec<StaticMeshVertex>, geometry: &GeometryInstance) {
    let mut half = scale3(geometry.size, 0.5);
    let mut center = geometry.transform.position;
    if let Some(mesh) = &geometry.mesh
        && mesh.kind == "SpecialMesh"
        && mesh.mesh_type == Some(2)
    {
        half = multiply3(half, mesh.scale.unwrap_or([1.0, 1.0, 1.0]));
        center = add3(
            center,
            rotate_vector(
                geometry.transform.rotation,
                mesh.offset.unwrap_or([0.0, 0.0, 0.0]),
            ),
        );
    }
    let world = |local| add3(center, rotate_vector(geometry.transform.rotation, local));
    let color = [
        channel(geometry.color[0]),
        channel(geometry.color[1]),
        channel(geometry.color[2]),
        channel(1.0 - geometry.transparency),
    ];
    if geometry.class == "WedgePart" {
        // Roblox wedges rise toward local +Z. Mirroring this puts the source
        // island's adjoining triangular sheets on opposite sides of each seam.
        let points = [
            world([-half[0], -half[1], half[2]]),
            world([half[0], -half[1], half[2]]),
            world([-half[0], half[1], half[2]]),
            world([half[0], half[1], half[2]]),
            world([-half[0], -half[1], -half[2]]),
            world([half[0], -half[1], -half[2]]),
        ];
        for indices in [
            [0, 1, 3],
            [0, 3, 2],
            [0, 4, 5],
            [0, 5, 1],
            [2, 3, 5],
            [2, 5, 4],
            [0, 2, 4],
            [1, 5, 3],
        ] {
            append_static_triangle(vertices, &points, indices, color);
        }
        return;
    }
    let points = [
        world([-half[0], -half[1], -half[2]]),
        world([half[0], -half[1], -half[2]]),
        world([half[0], half[1], -half[2]]),
        world([-half[0], half[1], -half[2]]),
        world([-half[0], -half[1], half[2]]),
        world([half[0], -half[1], half[2]]),
        world([half[0], half[1], half[2]]),
        world([-half[0], half[1], half[2]]),
    ];
    for indices in [
        [0, 3, 2],
        [0, 2, 1],
        [4, 5, 6],
        [4, 6, 7],
        [0, 4, 7],
        [0, 7, 3],
        [1, 2, 6],
        [1, 6, 5],
        [0, 1, 5],
        [0, 5, 4],
        [3, 7, 6],
        [3, 6, 2],
    ] {
        append_static_triangle(vertices, &points, indices, color);
    }
}

fn append_static_triangle<const N: usize>(
    vertices: &mut Vec<StaticMeshVertex>,
    points: &[[f32; 3]; N],
    indices: [usize; 3],
    color: [u8; 4],
) {
    let first = points[indices[0]];
    let second = points[indices[1]];
    let third = points[indices[2]];
    let edge_a = subtract3(second, first);
    let edge_b = subtract3(third, first);
    let cross = cross3(edge_a, edge_b);
    let length = dot3(cross, cross).sqrt();
    if !length.is_finite() || length <= 0.000001 {
        return;
    }
    let normal = scale3(cross, length.recip());
    vertices.extend(indices.into_iter().map(|index| StaticMeshVertex {
        position: points[index],
        normal,
        color,
    }));
}

fn rotate_vector(rows: [[f32; 3]; 3], vector: [f32; 3]) -> [f32; 3] {
    [
        dot3(rows[0], vector),
        dot3(rows[1], vector),
        dot3(rows[2], vector),
    ]
}

fn add3(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn subtract3(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn scale3(value: [f32; 3], scale: f32) -> [f32; 3] {
    [value[0] * scale, value[1] * scale, value[2] * scale]
}

fn sub3(left: [f32; 3], right: [f32; 3]) -> [f32; 3] {
    [left[0] - right[0], left[1] - right[1], left[2] - right[2]]
}

fn multiply3(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] * b[0], a[1] * b[1], a[2] * b[2]]
}

fn dot3(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross3(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn channel(value: f32) -> u8 {
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}

// This is an import approximation, not a renderer rule for arbitrary glTF names.
// Keep the original identity in material extras; source Color3 stays in COLOR_0.
fn export_material_name(name: &str) -> &str {
    match name.to_ascii_lowercase().as_str() {
        "grass" => "builtin:grass",
        "leafygrass" => "builtin:leafygrass",
        "ground" | "brick" => "builtin:ground",
        "rock" | "slate" | "concrete" | "granite" | "marble" | "pebble" | "cobblestone"
        | "corrodedmetal" | "diamondplate" | "foil" | "metal" => "builtin:rock",
        "sand" => "builtin:sand",
        "mud" | "wood" | "woodplanks" => "builtin:mud",
        "snow" | "ice" => "builtin:snow",
        _ => name,
    }
}

fn write_static_glb(path: &Path, groups: &[StaticMeshGroup]) -> Result<(), String> {
    const STRIDE: usize = 28;
    let mut binary = Vec::new();
    let mut buffer_views = Vec::with_capacity(groups.len());
    let mut accessors = Vec::with_capacity(groups.len() * 3);
    let mut primitives = Vec::with_capacity(groups.len());
    let mut materials = Vec::with_capacity(groups.len());
    for (material_index, group) in groups.iter().enumerate() {
        let byte_offset = binary.len();
        let mut minimum = [f32::INFINITY; 3];
        let mut maximum = [f32::NEG_INFINITY; 3];
        for vertex in &group.vertices {
            for axis in 0..3 {
                minimum[axis] = minimum[axis].min(vertex.position[axis]);
                maximum[axis] = maximum[axis].max(vertex.position[axis]);
                binary.extend_from_slice(&vertex.position[axis].to_le_bytes());
            }
            for value in vertex.normal {
                binary.extend_from_slice(&value.to_le_bytes());
            }
            binary.extend_from_slice(&vertex.color);
        }
        while binary.len() % 4 != 0 {
            binary.push(0);
        }
        let view_index = buffer_views.len();
        buffer_views.push(serde_json::json!({
            "buffer": 0,
            "byteOffset": byte_offset,
            "byteLength": binary.len() - byte_offset,
            "byteStride": STRIDE,
            "target": 34962
        }));
        let position_accessor = accessors.len();
        accessors.push(serde_json::json!({
            "bufferView": view_index,
            "componentType": 5126,
            "count": group.vertices.len(),
            "type": "VEC3",
            "min": minimum,
            "max": maximum
        }));
        let normal_accessor = accessors.len();
        accessors.push(serde_json::json!({
            "bufferView": view_index,
            "byteOffset": 12,
            "componentType": 5126,
            "count": group.vertices.len(),
            "type": "VEC3"
        }));
        let color_accessor = accessors.len();
        accessors.push(serde_json::json!({
            "bufferView": view_index,
            "byteOffset": 24,
            "componentType": 5121,
            "normalized": true,
            "count": group.vertices.len(),
            "type": "VEC4"
        }));
        primitives.push(serde_json::json!({
            "attributes": {
                "POSITION": position_accessor,
                "NORMAL": normal_accessor,
                "COLOR_0": color_accessor
            },
            "material": material_index,
            "mode": 4
        }));
        materials.push(serde_json::json!({
            "name": export_material_name(&group.material),
            "extras": { "robloxMaterial": group.material },
            "pbrMetallicRoughness": {
                "baseColorFactor": [1.0, 1.0, 1.0, 1.0],
                "metallicFactor": 0.0,
                "roughnessFactor": 0.88
            }
        }));
    }
    let document = serde_json::json!({
        "asset": { "version": "2.0", "generator": "cubacadabra-reference-import" },
        "scene": 0,
        "scenes": [{ "nodes": [0] }],
        "nodes": [{ "mesh": 0, "name": "Roblox reference geometry" }],
        "meshes": [{
            "name": "Roblox reference geometry",
            "primitives": primitives
        }],
        "materials": materials,
        "buffers": [{ "byteLength": binary.len() }],
        "bufferViews": buffer_views,
        "accessors": accessors
    });
    let mut json = serde_json::to_vec(&document)
        .map_err(|error| format!("could not encode GLB document: {error}"))?;
    while json.len() % 4 != 0 {
        json.push(b' ');
    }
    let total_length = 12usize
        .checked_add(8 + json.len())
        .and_then(|length| length.checked_add(8 + binary.len()))
        .and_then(|length| u32::try_from(length).ok())
        .ok_or_else(|| "reference GLB is too large".to_owned())?;
    let mut output = Vec::with_capacity(total_length as usize);
    output.extend_from_slice(&0x4654_6c67_u32.to_le_bytes());
    output.extend_from_slice(&2_u32.to_le_bytes());
    output.extend_from_slice(&total_length.to_le_bytes());
    output.extend_from_slice(&(json.len() as u32).to_le_bytes());
    output.extend_from_slice(&0x4e4f_534a_u32.to_le_bytes());
    output.extend_from_slice(&json);
    output.extend_from_slice(&(binary.len() as u32).to_le_bytes());
    output.extend_from_slice(&0x004e_4942_u32.to_le_bytes());
    output.extend_from_slice(&binary);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("could not create {}: {error}", parent.display()))?;
    }
    fs::write(path, output).map_err(|error| format!("could not write {}: {error}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const PLACE: &str = r#"<roblox version="4">
  <Item class="Folder" referent="0">
    <Properties><string name="Name">Place</string></Properties>
    <Item class="Part" referent="1">
      <Properties>
        <string name="Name">Island</string>
        <bool name="Anchored">true</bool>
        <CoordinateFrame name="CFrame">
          <X>10</X><Y>4</Y><Z>-2</Z>
          <R00>1</R00><R01>0</R01><R02>0</R02>
          <R10>0</R10><R11>1</R11><R12>0</R12>
          <R20>0</R20><R21>0</R21><R22>1</R22>
        </CoordinateFrame>
        <bool name="CanCollide">true</bool>
        <bool name="CastShadow">true</bool>
        <Color3uint8 name="Color3uint8">65280</Color3uint8>
        <token name="Material">1280</token>
        <Vector3 name="size"><X>8</X><Y>2</Y><Z>6</Z></Vector3>
        <float name="Transparency">0</float>
      </Properties>
      <Item class="SpecialMesh" referent="2">
        <Properties>
          <string name="Name">Mesh</string>
          <Content name="MeshId"><url>rbxassetid://123</url></Content>
          <token name="MeshType">5</token>
          <Vector3 name="Scale"><X>1</X><Y>2</Y><Z>3</Z></Vector3>
        </Properties>
      </Item>
      <Item class="PointLight" referent="3">
        <Properties>
          <string name="Name">Sun Lamp</string>
          <float name="Brightness">2</float>
          <Color3 name="Color"><R>1</R><G>0.5</G><B>0.25</B></Color3>
          <bool name="Enabled">true</bool>
          <float name="Range">16</float>
          <bool name="Shadows">true</bool>
        </Properties>
      </Item>
    </Item>
    <Item class="Camera" referent="4">
      <Properties>
        <string name="Name">Golden Camera</string>
        <CoordinateFrame name="CFrame">
          <X>0</X><Y>8</Y><Z>20</Z>
          <R00>1</R00><R01>0</R01><R02>0</R02>
          <R10>0</R10><R11>1</R11><R12>0</R12>
          <R20>0</R20><R21>0</R21><R22>1</R22>
        </CoordinateFrame>
        <float name="FieldOfView">55</float>
      </Properties>
    </Item>
  </Item>
</roblox>"#;

    const TERRAIN: &str = r#"<roblox version="4">
  <Item class="Terrain" referent="0">
    <Properties>
      <string name="Name">Terrain</string>
      <BinaryString name="SmoothGrid">AQIDBA==</BinaryString>
      <Color3 name="WaterColor"><R>0</R><G>0.5</G><B>1</B></Color3>
    </Properties>
  </Item>
</roblox>"#;

    #[test]
    fn imports_deterministic_static_reference_scene() {
        let temp = tempfile::tempdir().unwrap();
        let place = temp.path().join("Place.rbxmx");
        let terrain = temp.path().join("PlaceTerrain.rbxmx");
        let project = temp.path().join("default.project.json");
        let first = temp.path().join("first.json");
        let second = temp.path().join("second.json");
        let first_mesh = temp.path().join("first.glb");
        let second_mesh = temp.path().join("second.glb");
        fs::write(&place, PLACE).unwrap();
        fs::write(&terrain, TERRAIN).unwrap();
        fs::write(
            &project,
            r#"{"tree":{"Lighting":{"$className":"Lighting","$properties":{"Brightness":2},"ColorCorrection":{"$className":"ColorCorrectionEffect","$properties":{"Enabled":true,"Saturation":0.6}}}}}"#,
        )
        .unwrap();

        let options = |output| ImportOptions {
            place_path: place.clone(),
            terrain_path: Some(terrain.clone()),
            project_path: Some(project.clone()),
            output_path: output,
        };
        let result = import_reference(&options(first.clone())).unwrap();
        import_reference(&options(second.clone())).unwrap();

        assert_eq!(result.geometry_count, 1);
        assert_eq!(result.visible_geometry_count, 1);
        assert_eq!(result.camera_count, 1);
        assert_eq!(result.light_count, 1);
        assert!(result.has_terrain_payload);
        assert_eq!(fs::read(&first).unwrap(), fs::read(second).unwrap());

        let mesh_options = |output: PathBuf, collision_output: PathBuf| MeshExportOptions {
            scene_path: first.clone(),
            output_path: output.clone(),
            path_prefixes: vec!["Folder:Place[1]".to_owned()],
            exclude_paths: Vec::new(),
            scale: 1.0,
            origin: [0.0; 3],
            collision_output: Some(collision_output),
            bounds_output: Some(output.with_extension("bounds.json")),
            mesh_overrides: None,
        };
        let first_collision = temp.path().join("first-collision.json");
        let second_collision = temp.path().join("second-collision.json");
        let mesh =
            export_reference_mesh(&mesh_options(first_mesh.clone(), first_collision.clone()))
                .unwrap();
        export_reference_mesh(&mesh_options(second_mesh.clone(), second_collision.clone()))
            .unwrap();
        assert_eq!(mesh.geometry_count, 1);
        assert_eq!(mesh.triangle_count, 12);
        assert_eq!(mesh.vertex_count, 36);
        assert_eq!(mesh.bounds.minimum, [6.0, 3.0, -5.0]);
        assert_eq!(mesh.bounds.maximum, [14.0, 5.0, 1.0]);
        let bounds: serde_json::Value =
            serde_json::from_slice(&fs::read(first_mesh.with_extension("bounds.json")).unwrap())
                .unwrap();
        assert_eq!(bounds["size"], serde_json::json!([8.0, 2.0, 6.0]));
        let bytes = fs::read(&first_mesh).unwrap();
        let json_length = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
        let document: serde_json::Value =
            serde_json::from_slice(&bytes[20..20 + json_length]).unwrap();
        let material = &document["materials"][0];
        assert_eq!(material["name"], "builtin:grass");
        assert_eq!(material["extras"]["robloxMaterial"], "Grass");
        assert_eq!(
            material["pbrMetallicRoughness"]["baseColorFactor"],
            serde_json::json!([1.0, 1.0, 1.0, 1.0])
        );
        let binary_start = 28 + json_length;
        for vertex in bytes[binary_start..].chunks_exact(28) {
            assert_eq!(
                &vertex[24..28],
                &[0, 255, 0, 255],
                "source Color3 must survive export"
            );
        }
        assert_eq!(
            fs::read(first_mesh).unwrap(),
            fs::read(second_mesh).unwrap()
        );
        assert_eq!(
            fs::read(&first_collision).unwrap(),
            fs::read(&second_collision).unwrap()
        );
        let collision: serde_json::Value =
            serde_json::from_slice(&fs::read(first_collision).unwrap()).unwrap();
        assert_eq!(collision["formatVersion"], 1);
        assert_eq!(collision["triangles"].as_array().unwrap().len(), 12);
    }

    #[test]
    fn export_selection_scale_and_overrides_share_visual_and_collision_geometry() {
        let temp = tempfile::tempdir().unwrap();
        let place = temp.path().join("Place.rbxmx");
        let scene_path = temp.path().join("scene.json");
        fs::write(&place, PLACE).unwrap();
        import_reference(&ImportOptions {
            place_path: place,
            terrain_path: None,
            project_path: None,
            output_path: scene_path.clone(),
        })
        .unwrap();
        let mut scene: Value = serde_json::from_slice(&fs::read(&scene_path).unwrap()).unwrap();
        let mut visible = scene["geometry"][0].clone();
        visible["path"] = json!("Main/visible");
        visible["size"] = json!([4, 2, 6]);
        visible["mesh"] = json!({"kind":"MeshPart", "meshId":"local-test"});
        let mut hidden = visible.clone();
        hidden["path"] = json!("Rooms/hidden-floor");
        hidden["transparency"] = json!(1);
        let mut excluded = visible.clone();
        excluded["path"] = json!("Rooms/exclude-this");
        let mut outside = visible.clone();
        outside["path"] = json!("Other/not-selected");
        scene["geometry"] = json!([visible, hidden, excluded, outside]);
        fs::write(&scene_path, serde_json::to_vec(&scene).unwrap()).unwrap();
        let overrides = temp.path().join("overrides.json");
        fs::write(&overrides, r#"{"formatVersion":1,"meshes":{"local-test":{"vertices":[[-0.5,0,-0.5],[0.5,0,-0.5],[0,0,0.5]],"triangles":[[0,1,2]]}}}"#).unwrap();
        let mesh_path = temp.path().join("mesh.glb");
        let collision_path = temp.path().join("collision.json");
        let result = export_reference_mesh(&MeshExportOptions {
            scene_path,
            output_path: mesh_path.clone(),
            path_prefixes: vec!["Main/".into(), "Rooms/".into()],
            exclude_paths: vec!["exclude-this".into()],
            scale: 0.5,
            origin: [0.0; 3],
            collision_output: Some(collision_path.clone()),
            bounds_output: None,
            mesh_overrides: Some(overrides),
        })
        .unwrap();
        assert_eq!(result.geometry_count, 2);
        assert_eq!(result.triangle_count, 1, "hidden collider must not render");
        let collision: Value = serde_json::from_slice(&fs::read(collision_path).unwrap()).unwrap();
        let expected = json!([[4.0, 2.0, -2.5], [6.0, 2.0, -2.5], [5.0, 2.0, 0.5]]);
        assert_eq!(collision["triangles"], json!([expected, expected]));
        let bytes = fs::read(mesh_path).unwrap();
        let json_length = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
        let vertices: Vec<_> = bytes[28 + json_length..]
            .chunks_exact(28)
            .map(|v| {
                (0..3)
                    .map(|axis| f32::from_le_bytes(v[axis * 4..axis * 4 + 4].try_into().unwrap()))
                    .collect::<Vec<_>>()
            })
            .collect();
        assert_eq!(
            json!(vertices),
            expected,
            "visual and collision transforms must match"
        );
    }

    #[test]
    fn exported_materials_opt_in_without_inventing_a_color_factor() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("materials.glb");
        let names = ["Grass", "Slate", "WoodPlanks", "Metal", "Plastic"];
        let groups = names
            .iter()
            .map(|name| StaticMeshGroup {
                material: name.to_string(),
                vertices: vec![
                    StaticMeshVertex {
                        position: [0.0; 3],
                        normal: [0.0, 1.0, 0.0],
                        color: [86, 66, 54, 255]
                    };
                    3
                ],
            })
            .collect::<Vec<_>>();
        write_static_glb(&path, &groups).unwrap();
        let bytes = fs::read(path).unwrap();
        let json_length = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
        let doc: serde_json::Value = serde_json::from_slice(&bytes[20..20 + json_length]).unwrap();
        for (i, expected) in [
            "builtin:grass",
            "builtin:rock",
            "builtin:mud",
            "builtin:rock",
            "Plastic",
        ]
        .iter()
        .enumerate()
        {
            assert_eq!(doc["materials"][i]["name"], *expected);
            assert_eq!(doc["materials"][i]["extras"]["robloxMaterial"], names[i]);
            assert_eq!(
                doc["materials"][i]["pbrMetallicRoughness"]["baseColorFactor"],
                serde_json::json!([1.0, 1.0, 1.0, 1.0])
            );
        }
    }

    #[test]
    fn source_wedge_rises_toward_positive_z_with_outward_normals() {
        let temp = tempfile::tempdir().unwrap();
        let place = temp.path().join("wedge.rbxlx");
        let output = temp.path().join("scene.json");
        fs::write(
            &place,
            PLACE.replace("class=\"Part\"", "class=\"WedgePart\""),
        )
        .unwrap();
        import_reference(&ImportOptions {
            place_path: place,
            terrain_path: None,
            project_path: None,
            output_path: output.clone(),
        })
        .unwrap();
        let scene = read_reference_scene(output).unwrap();
        let mut vertices = Vec::new();
        append_static_geometry(&mut vertices, &scene.geometry[0]);
        assert_eq!(vertices.len(), 24);
        for vertex in vertices.iter().filter(|v| v.position[1] > 4.0) {
            assert_eq!(
                vertex.position[2], 1.0,
                "high edge must be on the back (+Z) face"
            );
        }
        let slope = &vertices[12..18];
        assert!(slope.iter().all(|v| v.normal[1] > 0.0 && v.normal[2] < 0.0));
    }
}
