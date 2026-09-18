//! Narrow Roblox XML importer for visual-reference reconstruction.
//!
//! This crate intentionally emits a tool-owned intermediate scene instead of a
//! Cubacadabra package manifest. It preserves source facts first; package and
//! renderer adaptation can then be measured against that stable artifact.

use rbx_dom_weak::types::{CFrame, Color3, ContentType, Matrix3, Variant, Vector3};
use rbx_dom_weak::{Instance, WeakDom, types::Ref, ustr};
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, HashMap},
    fs::{self, File},
    io::{BufReader, Write},
    path::{Path, PathBuf},
};

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

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ReferenceScene {
    format_version: u32,
    kind: &'static str,
    coordinate_system: &'static str,
    source: SourceSet,
    summary: SceneSummary,
    bounds: Option<Bounds>,
    class_counts: BTreeMap<String, usize>,
    geometry: Vec<GeometryInstance>,
    cameras: Vec<CameraInstance>,
    lights: Vec<LightInstance>,
    textures: Vec<SurfaceTexture>,
    texts: Vec<TextInstance>,
    spawns: Vec<SpawnInstance>,
    project_lighting: Option<ProjectLighting>,
    terrain: Option<TerrainSource>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SourceSet {
    place: SourceFile,
    #[serde(skip_serializing_if = "Option::is_none")]
    terrain: Option<SourceFile>,
    #[serde(skip_serializing_if = "Option::is_none")]
    project: Option<SourceFile>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SourceFile {
    name: String,
    bytes: u64,
    sha256: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SceneSummary {
    instance_count: usize,
    geometry_count: usize,
    visible_geometry_count: usize,
    camera_count: usize,
    light_count: usize,
    texture_count: usize,
    text_count: usize,
    spawn_count: usize,
}

#[derive(Debug, Clone, Serialize)]
struct Bounds {
    minimum: [f32; 3],
    maximum: [f32; 3],
}

#[derive(Debug, Clone, Serialize)]
struct Transform {
    position: [f32; 3],
    /// Roblox CFrame basis vectors, stored as columns in source coordinates.
    rotation: [[f32; 3]; 3],
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct GeometryInstance {
    path: String,
    parent_path: String,
    class: String,
    name: String,
    transform: Transform,
    size: [f32; 3],
    color: [f32; 3],
    material: Material,
    transparency: f32,
    reflectance: f32,
    anchored: bool,
    can_collide: bool,
    cast_shadow: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    shape: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    mesh: Option<MeshReference>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    opaque_properties: Vec<OpaqueProperty>,
}

#[derive(Debug, Serialize)]
struct Material {
    value: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<&'static str>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct MeshReference {
    kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    mesh_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    texture_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    mesh_type: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    scale: Option<[f32; 3]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    offset: Option<[f32; 3]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    vertex_color: Option<[f32; 3]>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct OpaqueProperty {
    name: String,
    kind: &'static str,
    bytes: usize,
    sha256: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CameraInstance {
    path: String,
    name: String,
    transform: Transform,
    #[serde(skip_serializing_if = "Option::is_none")]
    focus: Option<Transform>,
    field_of_view: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    camera_type: Option<u32>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct LightInstance {
    path: String,
    parent_path: String,
    class: String,
    name: String,
    color: [f32; 3],
    brightness: f32,
    range: f32,
    enabled: bool,
    shadows: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    face: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    angle: Option<f32>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SurfaceTexture {
    path: String,
    parent_path: String,
    class: String,
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    asset: Option<String>,
    color: [f32; 3],
    transparency: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    face: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    studs_per_tile_u: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    studs_per_tile_v: Option<f32>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct TextInstance {
    path: String,
    parent_path: String,
    class: String,
    name: String,
    text: String,
    color: [f32; 3],
    transparency: f32,
    text_scaled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    font: Option<u32>,
}

#[derive(Debug, Serialize)]
struct SpawnInstance {
    path: String,
    name: String,
    transform: Transform,
    size: [f32; 3],
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProjectLighting {
    properties: BTreeMap<String, Value>,
    effects: Vec<ProjectEffect>,
}

#[derive(Debug, Serialize)]
struct ProjectEffect {
    name: String,
    class: String,
    properties: BTreeMap<String, Value>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct TerrainSource {
    properties: BTreeMap<String, Value>,
    opaque_properties: Vec<OpaqueProperty>,
    requires_voxel_decoder: bool,
}

#[derive(Default)]
struct SceneCollector {
    class_counts: BTreeMap<String, usize>,
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
        kind: "roblox-static-reference-scene",
        coordinate_system: "Roblox source coordinates (X right, Y up, CFrame basis preserved)",
        source: SourceSet {
            place: place_source,
            terrain: terrain_source_file,
            project: project_source,
        },
        summary,
        bounds: collector.bounds,
        class_counts: collector.class_counts,
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
        name: material_name(value),
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
        kind,
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

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(fs::read(first).unwrap(), fs::read(second).unwrap());
    }
}
