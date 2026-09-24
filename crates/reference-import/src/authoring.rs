use crate::{GeometryInstance, ImportOptions, ReferenceScene, load_reference};
use cubacadabra_scene::{AuthoringNode, AuthoringScene, EditorMetadata, SourceMetadata, Transform};
use glam::{EulerRot, Mat3, Quat, Vec3};
use rbx_dom_weak::types::{CFrame, Color3, Enum, Matrix3, Vector3};
use rbx_dom_weak::{Instance, InstanceBuilder, WeakDom, types::Ref, ustr};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    fs::{self, File},
    io::{BufReader, BufWriter, Write},
    path::{Path, PathBuf},
};

const GENERATED_BY: &str = "cubacadabra-roblox-interchange";
const WORKSPACE_ROOT: &str = "Workspace:Workspace[1]";

#[derive(Clone, Debug, PartialEq)]
pub struct RobloxAuthoringImport {
    pub scene: AuthoringScene,
    pub import_root_id: String,
    pub source_sha256: String,
    pub source_instances: usize,
    pub editable_parts: usize,
    pub preserved_instances: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RobloxExportReport {
    pub updated_parts: usize,
    pub added_parts: usize,
    pub preserved_instances: usize,
    pub omitted_nodes: usize,
    pub warnings: Vec<String>,
}

/// Merge the editable subset of a Roblox XML place into a native authoring
/// scene. The original XML remains the preservation layer and is referenced by
/// a project-relative path recorded on every generated node.
pub fn import_roblox_authoring_scene(
    place_path: impl AsRef<Path>,
    base_scene: &AuthoringScene,
    source_file: &str,
) -> Result<RobloxAuthoringImport, String> {
    base_scene.validate()?;
    let place_path = place_path.as_ref();
    let reference = load_reference(&ImportOptions {
        place_path: place_path.to_path_buf(),
        terrain_path: None,
        project_path: None,
        output_path: PathBuf::new(),
    })?;
    merge_reference(reference, base_scene, source_file)
}

fn merge_reference(
    reference: ReferenceScene,
    base_scene: &AuthoringScene,
    source_file: &str,
) -> Result<RobloxAuthoringImport, String> {
    let mut scene = base_scene.clone();
    let parent_id = scene
        .nodes
        .iter()
        .find(|node| node.parent_id.is_none())
        .map(|node| node.id.clone())
        .ok_or_else(|| "authoring scene has no root node for the Roblox import".to_owned())?;
    let import_id = short_hash(&reference.source.place.sha256);
    scene.nodes.retain(|node| {
        node.source
            .as_ref()
            .and_then(|source| source.properties.get("importId"))
            .and_then(Value::as_str)
            != Some(import_id.as_str())
    });

    let import_root_id = unique_id(&scene, &format!("roblox-import-{import_id}"));
    let source_root_id = unique_id(&scene, &format!("roblox-source-{import_id}"));
    let display_name = Path::new(&reference.source.place.name)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .filter(|stem| !stem.is_empty())
        .unwrap_or("Roblox Place");
    let promoted = reference
        .geometry
        .iter()
        .filter(|geometry| promotable_part(geometry))
        .collect::<Vec<_>>();
    let promoted_paths = promoted
        .iter()
        .map(|geometry| geometry.path.as_str())
        .collect::<BTreeSet<_>>();
    let preserved_instances = reference.instances.len().saturating_sub(promoted.len());

    scene.nodes.push(AuthoringNode {
        id: import_root_id.clone(),
        parent_id: Some(parent_id.clone()),
        name: display_name.to_owned(),
        transform: Transform::default(),
        components: BTreeMap::new(),
        editor: EditorMetadata::default(),
        source: Some(source_metadata(
            "ImportRoot",
            None,
            source_file,
            &import_id,
            BTreeMap::from([
                ("compatibility".to_owned(), json!("native-and-preserved")),
                ("editableParts".to_owned(), json!(promoted.len())),
                ("preservedInstances".to_owned(), json!(preserved_instances)),
            ]),
        )),
    });
    scene.nodes.push(AuthoringNode {
        id: source_root_id,
        parent_id: Some(import_root_id.clone()),
        name: "Preserved Roblox Source".to_owned(),
        transform: Transform::default(),
        components: BTreeMap::new(),
        editor: EditorMetadata {
            visible: true,
            locked: true,
            lock_reason: Some(
                "Unsupported Roblox objects are preserved in the source XML and exported unchanged"
                    .to_owned(),
            ),
        },
        source: Some(source_metadata(
            "SourceArchive",
            None,
            source_file,
            &import_id,
            BTreeMap::from([
                (
                    "sourceSha256".to_owned(),
                    json!(reference.source.place.sha256),
                ),
                ("instanceCount".to_owned(), json!(reference.instances.len())),
                ("preservedInstances".to_owned(), json!(preserved_instances)),
            ]),
        )),
    });

    let instances = reference
        .instances
        .iter()
        .map(|instance| (instance.path.as_str(), instance))
        .collect::<BTreeMap<_, _>>();
    let mut group_paths = BTreeSet::new();
    for geometry in &promoted {
        let mut ancestor = geometry.parent_path.as_str();
        while let Some(instance) = instances.get(ancestor) {
            if matches!(instance.class.as_str(), "Model" | "Folder") {
                group_paths.insert(instance.path.clone());
            }
            if instance.path == WORKSPACE_ROOT || instance.parent_path.is_empty() {
                break;
            }
            ancestor = &instance.parent_path;
        }
    }
    let mut group_paths = group_paths.into_iter().collect::<Vec<_>>();
    group_paths.sort_by_key(|path| (path.split('/').count(), path.clone()));
    let mut group_ids = BTreeMap::new();
    for path in group_paths {
        let Some(instance) = instances.get(path.as_str()) else {
            continue;
        };
        let id = unique_id(&scene, &format!("roblox-group-{}", short_hash(&path)));
        let parent = group_ids
            .get(instance.parent_path.as_str())
            .cloned()
            .unwrap_or_else(|| import_root_id.clone());
        group_ids.insert(path.clone(), id.clone());
        scene.nodes.push(AuthoringNode {
            id,
            parent_id: Some(parent),
            name: instance.name.clone(),
            transform: Transform::default(),
            components: BTreeMap::new(),
            editor: EditorMetadata::default(),
            source: Some(source_metadata(
                &instance.class,
                Some(&instance.path),
                source_file,
                &import_id,
                BTreeMap::from([("representation".to_owned(), json!("native-group"))]),
            )),
        });
    }

    for geometry in promoted {
        if !promoted_paths.contains(geometry.path.as_str()) {
            continue;
        }
        let id = unique_id(
            &scene,
            &format!("roblox-part-{}", short_hash(&geometry.path)),
        );
        let parent = group_ids
            .get(&geometry.parent_path)
            .cloned()
            .unwrap_or_else(|| import_root_id.clone());
        let mut components = BTreeMap::from([(
            "primitive".to_owned(),
            json!({
                "shape": "box",
                "size": geometry.size,
                "color": source_color(geometry.color),
                "collidable": geometry.can_collide,
                "castShadow": geometry.cast_shadow,
            }),
        )]);
        if geometry.can_collide {
            components.insert("collision".to_owned(), json!({"kind": "box"}));
        }
        scene.nodes.push(AuthoringNode {
            id,
            parent_id: Some(parent),
            name: geometry.name.clone(),
            transform: Transform {
                position: geometry.transform.position,
                rotation: source_rotation_to_euler(geometry.transform.rotation),
                scale: [1.0; 3],
            },
            components,
            editor: EditorMetadata::default(),
            source: Some(source_metadata(
                &geometry.class,
                Some(&geometry.path),
                source_file,
                &import_id,
                BTreeMap::from([
                    ("representation".to_owned(), json!("native-primitive")),
                    ("sourceAnchored".to_owned(), json!(geometry.anchored)),
                    ("sourceMaterial".to_owned(), json!(geometry.material.name)),
                    (
                        "sourceMaterialValue".to_owned(),
                        json!(geometry.material.value),
                    ),
                ]),
            )),
        });
    }
    scene.validate()?;
    Ok(RobloxAuthoringImport {
        scene,
        import_root_id,
        source_sha256: reference.source.place.sha256,
        source_instances: reference.instances.len(),
        editable_parts: promoted_paths.len(),
        preserved_instances,
    })
}

fn source_metadata(
    class: &str,
    path: Option<&str>,
    source_file: &str,
    import_id: &str,
    mut properties: BTreeMap<String, Value>,
) -> SourceMetadata {
    properties.insert("generatedBy".to_owned(), json!(GENERATED_BY));
    properties.insert("sourceFile".to_owned(), json!(source_file));
    properties.insert("importId".to_owned(), json!(import_id));
    SourceMetadata {
        format: "roblox".to_owned(),
        class: Some(class.to_owned()),
        path: path.map(str::to_owned),
        properties,
    }
}

fn promotable_part(geometry: &GeometryInstance) -> bool {
    geometry.class == "Part"
        && geometry.shape.is_none_or(|shape| shape == 1)
        && geometry.mesh.is_none()
        && geometry.transparency.abs() <= 0.0001
        && geometry.reflectance.abs() <= 0.0001
        && geometry
            .size
            .iter()
            .all(|value| value.is_finite() && *value >= 0.05)
        && (geometry.path == WORKSPACE_ROOT
            || geometry.path.starts_with(&format!("{WORKSPACE_ROOT}/")))
}

pub fn roblox_source_file(scene: &AuthoringScene) -> Option<&str> {
    scene.nodes.iter().find_map(|node| {
        node.source
            .as_ref()
            .filter(|source| source.format == "roblox")?
            .properties
            .get("sourceFile")?
            .as_str()
    })
}

/// Export a Roblox XML place. When `preserved_source` is present, the original
/// DOM is the base: supported Parts are updated and every unknown instance and
/// property remains untouched. Native Cubacadabra primitives are added under a
/// clearly named model in Workspace.
pub fn write_roblox_place(
    scene: &AuthoringScene,
    preserved_source: Option<&Path>,
    output_path: impl AsRef<Path>,
) -> Result<RobloxExportReport, String> {
    scene.validate()?;
    let mut dom = match preserved_source {
        Some(path) => decode_xml(path)?,
        None => WeakDom::new(InstanceBuilder::new("DataModel")),
    };
    let original_instance_count = dom.descendants().count();
    let paths = instance_refs_by_path(&dom)?;
    let world_transforms = scene.world_transforms()?;
    let mut report = RobloxExportReport::default();
    let mut source_linked = BTreeSet::new();

    for node in &scene.nodes {
        let Some(source) = node
            .source
            .as_ref()
            .filter(|source| source.format == "roblox")
        else {
            continue;
        };
        let Some(path) = source.path.as_deref() else {
            continue;
        };
        let Some(referent) = paths.get(path).copied() else {
            report.warnings.push(format!(
                "{}: preserved Roblox source path is missing; the native edit was not applied",
                node.name
            ));
            continue;
        };
        source_linked.insert(node.id.as_str());
        let Some(instance) = dom.get_by_ref_mut(referent) else {
            continue;
        };
        instance.name = node.name.clone();
        if node.components.contains_key("primitive") {
            let Some(world) = world_transforms.get(&node.id) else {
                continue;
            };
            update_part(instance, node, world, &mut report)?;
            report.updated_parts += 1;
        }
    }

    let native_parts = scene
        .nodes
        .iter()
        .filter(|node| node.components.contains_key("primitive"))
        .filter(|node| !source_linked.contains(node.id.as_str()))
        .collect::<Vec<_>>();
    if !native_parts.is_empty() {
        let workspace = find_or_create_workspace(&mut dom);
        let mut export_model = InstanceBuilder::new("Model").with_name("Cubacadabra Export");
        for node in native_parts {
            let Some(world) = world_transforms.get(&node.id) else {
                continue;
            };
            match part_builder(node, world, &mut report) {
                Some(part) => {
                    export_model.add_child(part);
                    report.added_parts += 1;
                }
                None => report.omitted_nodes += 1,
            }
        }
        if report.added_parts > 0 {
            dom.insert(workspace, export_model);
        }
    }

    report.preserved_instances = original_instance_count.saturating_sub(report.updated_parts);
    report.omitted_nodes += scene
        .nodes
        .iter()
        .filter(|node| {
            !node.components.is_empty()
                && !node.components.contains_key("primitive")
                && node
                    .source
                    .as_ref()
                    .and_then(|source| source.path.as_ref())
                    .is_none()
        })
        .count();
    if report.omitted_nodes > 0 {
        report.warnings.push(format!(
            "{} native node(s) use components this Roblox exporter does not represent yet",
            report.omitted_nodes
        ));
    }

    let output_path = output_path.as_ref();
    let parent = output_path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).map_err(|error| {
        format!(
            "could not create export directory {}: {error}",
            parent.display()
        )
    })?;
    let temp_name = format!(
        ".{}.{}.tmp",
        output_path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("place.rbxlx"),
        std::process::id()
    );
    let temp_path = parent.join(temp_name);
    let write_result = (|| {
        let mut writer = BufWriter::new(File::create(&temp_path).map_err(|error| {
            format!(
                "could not create Roblox export {}: {error}",
                temp_path.display()
            )
        })?);
        rbx_xml::to_writer_default(&mut writer, &dom, dom.root().children()).map_err(|error| {
            format!(
                "could not encode Roblox XML {}: {error}",
                output_path.display()
            )
        })?;
        writer.flush().map_err(|error| {
            format!(
                "could not finish Roblox export {}: {error}",
                output_path.display()
            )
        })?;
        #[cfg(target_os = "windows")]
        if output_path.exists() {
            fs::remove_file(output_path).map_err(|error| {
                format!(
                    "could not replace Roblox export {}: {error}",
                    output_path.display()
                )
            })?;
        }
        fs::rename(&temp_path, output_path).map_err(|error| {
            format!(
                "could not replace Roblox export {}: {error}",
                output_path.display()
            )
        })
    })();
    if write_result.is_err() {
        let _ = fs::remove_file(&temp_path);
    }
    write_result?;
    Ok(report)
}

fn update_part(
    instance: &mut Instance,
    node: &AuthoringNode,
    world: &cubacadabra_scene::AuthoringWorldTransform,
    report: &mut RobloxExportReport,
) -> Result<(), String> {
    let primitive = node.components["primitive"]
        .as_object()
        .ok_or_else(|| format!("scene node {} primitive must be an object", node.id))?;
    let size = json_vector3(primitive.get("size"))
        .ok_or_else(|| format!("scene node {} primitive has no valid size", node.id))?;
    let size = multiply(size, world.scale);
    instance
        .properties
        .insert(ustr("CFrame"), cframe(world).into());
    instance
        .properties
        .insert(ustr("Size"), Vector3::new(size[0], size[1], size[2]).into());
    instance.properties.insert(
        ustr("CanCollide"),
        primitive
            .get("collidable")
            .and_then(Value::as_bool)
            .unwrap_or(true)
            .into(),
    );
    instance.properties.insert(
        ustr("CastShadow"),
        primitive
            .get("castShadow")
            .and_then(Value::as_bool)
            .unwrap_or(true)
            .into(),
    );
    if let Some(color) = primitive
        .get("color")
        .and_then(Value::as_str)
        .and_then(parse_color)
    {
        instance.properties.insert(
            ustr("Color"),
            Color3::new(color[0], color[1], color[2]).into(),
        );
    } else if primitive.get("color").is_some() {
        report.warnings.push(format!(
            "{}: named color could not be resolved for Roblox; preserved the source color",
            node.name
        ));
    }
    if let Some(material) = primitive
        .get("material")
        .and_then(Value::as_str)
        .and_then(roblox_material_value)
    {
        instance
            .properties
            .insert(ustr("Material"), Enum::from_u32(material).into());
    }
    Ok(())
}

fn part_builder(
    node: &AuthoringNode,
    world: &cubacadabra_scene::AuthoringWorldTransform,
    report: &mut RobloxExportReport,
) -> Option<InstanceBuilder> {
    let primitive = node.components.get("primitive")?.as_object()?;
    let local_size = json_vector3(primitive.get("size"))?;
    let size = multiply(local_size, world.scale);
    let color = primitive
        .get("color")
        .and_then(Value::as_str)
        .and_then(parse_color)
        .unwrap_or_else(|| {
            report.warnings.push(format!(
                "{}: named color was exported with the neutral fallback color",
                node.name
            ));
            [0.64; 3]
        });
    let material = primitive
        .get("material")
        .and_then(Value::as_str)
        .and_then(roblox_material_value)
        .unwrap_or(256);
    Some(
        InstanceBuilder::new("Part")
            .with_name(&node.name)
            .with_property("CFrame", cframe(world))
            .with_property("Size", Vector3::new(size[0], size[1], size[2]))
            .with_property("Color", Color3::new(color[0], color[1], color[2]))
            .with_property("Anchored", true)
            .with_property(
                "CanCollide",
                primitive
                    .get("collidable")
                    .and_then(Value::as_bool)
                    .unwrap_or(true),
            )
            .with_property(
                "CastShadow",
                primitive
                    .get("castShadow")
                    .and_then(Value::as_bool)
                    .unwrap_or(true),
            )
            .with_property("Material", Enum::from_u32(material)),
    )
}

fn cframe(world: &cubacadabra_scene::AuthoringWorldTransform) -> CFrame {
    let matrix = Mat3::from_quat(Quat::from_euler(
        EulerRot::XYZ,
        world.rotation[0],
        world.rotation[1],
        world.rotation[2],
    ));
    let rows = matrix.transpose().to_cols_array_2d();
    CFrame::new(
        Vector3::new(world.position[0], world.position[1], world.position[2]),
        Matrix3::new(
            Vector3::new(rows[0][0], rows[0][1], rows[0][2]),
            Vector3::new(rows[1][0], rows[1][1], rows[1][2]),
            Vector3::new(rows[2][0], rows[2][1], rows[2][2]),
        ),
    )
}

fn find_or_create_workspace(dom: &mut WeakDom) -> Ref {
    if let Some(reference) = dom.root().children().iter().copied().find(|reference| {
        dom.get_by_ref(*reference)
            .is_some_and(|instance| instance.class.as_str() == "Workspace")
    }) {
        reference
    } else {
        dom.insert(
            dom.root_ref(),
            InstanceBuilder::new("Workspace").with_name("Workspace"),
        )
    }
}

fn instance_refs_by_path(dom: &WeakDom) -> Result<BTreeMap<String, Ref>, String> {
    fn walk(
        dom: &WeakDom,
        parent_ref: Ref,
        parent_path: &str,
        output: &mut BTreeMap<String, Ref>,
    ) -> Result<(), String> {
        let parent = dom
            .get_by_ref(parent_ref)
            .ok_or_else(|| "Roblox DOM contains a missing parent reference".to_owned())?;
        let children = parent.children().to_vec();
        let mut occurrences: HashMap<(String, String), usize> = HashMap::new();
        for child_ref in children {
            let child = dom
                .get_by_ref(child_ref)
                .ok_or_else(|| "Roblox DOM contains a missing child reference".to_owned())?;
            let class = child.class.to_string();
            let key = (class.clone(), child.name.clone());
            let occurrence = occurrences.entry(key).or_default();
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
            output.insert(path.clone(), child_ref);
            walk(dom, child_ref, &path, output)?;
        }
        Ok(())
    }

    let mut output = BTreeMap::new();
    walk(dom, dom.root_ref(), "", &mut output)?;
    Ok(output)
}

fn decode_xml(path: &Path) -> Result<WeakDom, String> {
    let reader = BufReader::new(
        File::open(path).map_err(|error| format!("could not open {}: {error}", path.display()))?,
    );
    rbx_xml::from_reader_default(reader)
        .map_err(|error| format!("could not decode Roblox XML {}: {error}", path.display()))
}

fn unique_id(scene: &AuthoringScene, base: &str) -> String {
    if scene.nodes.iter().all(|node| node.id != base) {
        return base.to_owned();
    }
    (2..)
        .map(|suffix| format!("{base}-{suffix}"))
        .find(|candidate| scene.nodes.iter().all(|node| node.id != *candidate))
        .expect("scene IDs have a finite practical range")
}

fn source_rotation_to_euler(rotation: [[f32; 3]; 3]) -> [f32; 3] {
    let matrix = Mat3::from_cols(
        Vec3::new(rotation[0][0], rotation[1][0], rotation[2][0]),
        Vec3::new(rotation[0][1], rotation[1][1], rotation[2][1]),
        Vec3::new(rotation[0][2], rotation[1][2], rotation[2][2]),
    );
    let (x, y, z) = Quat::from_mat3(&matrix).to_euler(EulerRot::XYZ);
    [x, y, z]
}

fn source_color(color: [f32; 3]) -> String {
    let channel = |value: f32| (value.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!(
        "#{:02X}{:02X}{:02X}",
        channel(color[0]),
        channel(color[1]),
        channel(color[2])
    )
}

fn parse_color(value: &str) -> Option<[f32; 3]> {
    let hex = value.strip_prefix('#')?;
    if hex.len() != 6 {
        return None;
    }
    let channel = |offset| u8::from_str_radix(&hex[offset..offset + 2], 16).ok();
    Some([
        channel(0)? as f32 / 255.0,
        channel(2)? as f32 / 255.0,
        channel(4)? as f32 / 255.0,
    ])
}

fn roblox_material_value(value: &str) -> Option<u32> {
    Some(match value {
        "builtin:wood" | "Wood" => 512,
        "builtin:brick" | "Brick" => 848,
        "builtin:rock" | "Rock" => 896,
        "builtin:metal" | "Metal" => 1088,
        "builtin:grass" | "Grass" => 1280,
        "builtin:sand" | "Sand" => 1296,
        "builtin:ice" | "Ice" => 1536,
        "builtin:snow" | "Snow" => 1568,
        "Plastic" => 256,
        "SmoothPlastic" => 272,
        _ => return None,
    })
}

fn json_vector3(value: Option<&Value>) -> Option<[f32; 3]> {
    let values = value?.as_array()?;
    Some([
        values.first()?.as_f64()? as f32,
        values.get(1)?.as_f64()? as f32,
        values.get(2)?.as_f64()? as f32,
    ])
}

fn multiply(left: [f32; 3], right: [f32; 3]) -> [f32; 3] {
    [left[0] * right[0], left[1] * right[1], left[2] * right[2]]
}

fn short_hash(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))[..12].to_owned()
}

fn escape_path_component(value: &str) -> String {
    value.replace('%', "%25").replace('/', "%2F")
}

#[cfg(test)]
mod tests {
    use super::*;
    use cubacadabra_scene::{AUTHORING_SCENE_FORMAT_VERSION, serialize_authoring_scene};
    use tempfile::tempdir;

    const PLACE: &str = r#"<roblox version="4">
  <Item class="Workspace" referent="RBX0">
    <Properties><string name="Name">Workspace</string></Properties>
    <Item class="Model" referent="RBX1">
      <Properties><string name="Name">Yard</string><string name="CustomState">keep-me</string></Properties>
      <Item class="Part" referent="RBX2">
        <Properties>
          <string name="Name">Block</string>
          <CoordinateFrame name="CFrame"><X>1</X><Y>2</Y><Z>3</Z><R00>1</R00><R01>0</R01><R02>0</R02><R10>0</R10><R11>1</R11><R12>0</R12><R20>0</R20><R21>0</R21><R22>1</R22></CoordinateFrame>
          <Vector3 name="Size"><X>4</X><Y>2</Y><Z>6</Z></Vector3>
          <Color3 name="Color"><R>1</R><G>0</G><B>0</B></Color3>
          <bool name="Anchored">true</bool><bool name="CanCollide">true</bool><bool name="CastShadow">true</bool>
          <token name="Material">256</token><token name="Shape">1</token>
        </Properties>
      </Item>
      <Item class="ParticleEmitter" referent="RBX3">
        <Properties><string name="Name">PreserveMe</string><float name="Rate">9</float></Properties>
      </Item>
    </Item>
  </Item>
</roblox>"#;

    fn base_scene() -> AuthoringScene {
        AuthoringScene {
            format_version: AUTHORING_SCENE_FORMAT_VERSION,
            world_id: Some("world".to_owned()),
            nodes: vec![AuthoringNode {
                id: "world".to_owned(),
                parent_id: None,
                name: "World".to_owned(),
                transform: Transform::default(),
                components: BTreeMap::new(),
                editor: EditorMetadata::default(),
                source: None,
            }],
        }
    }

    #[test]
    fn imports_editable_parts_and_preserves_the_source_contract() {
        let temp = tempdir().unwrap();
        let source = temp.path().join("yard.rbxlx");
        fs::write(&source, PLACE).unwrap();
        let imported = import_roblox_authoring_scene(
            &source,
            &base_scene(),
            "imports/roblox/yard/source.rbxlx",
        )
        .unwrap();

        assert_eq!(imported.editable_parts, 1);
        assert_eq!(imported.preserved_instances, 3);
        assert_eq!(
            roblox_source_file(&imported.scene),
            Some("imports/roblox/yard/source.rbxlx")
        );
        let encoded = serialize_authoring_scene(&imported.scene).unwrap();
        assert!(encoded.contains("Preserved Roblox Source"));
        assert!(encoded.contains("roblox-part-"));
    }

    #[test]
    fn export_updates_supported_parts_without_dropping_unknown_instances() {
        let temp = tempdir().unwrap();
        let source = temp.path().join("yard.rbxlx");
        let output = temp.path().join("yard-export.rbxlx");
        fs::write(&source, PLACE).unwrap();
        let mut imported = import_roblox_authoring_scene(
            &source,
            &base_scene(),
            "imports/roblox/yard/source.rbxlx",
        )
        .unwrap();
        let block = imported
            .scene
            .nodes
            .iter_mut()
            .find(|node| node.components.contains_key("primitive"))
            .unwrap();
        block.transform.position = [8.0, 9.0, 10.0];
        block.name = "Edited Block".to_owned();

        let report = write_roblox_place(&imported.scene, Some(&source), &output).unwrap();
        assert_eq!(report.updated_parts, 1);
        let exported = fs::read_to_string(&output).unwrap();
        assert!(exported.contains("PreserveMe"));
        assert!(exported.contains("Edited Block"));
        assert!(exported.contains("name=\"Rate\""));

        let normalized = load_reference(&ImportOptions {
            place_path: output,
            terrain_path: None,
            project_path: None,
            output_path: PathBuf::new(),
        })
        .unwrap();
        let block = normalized
            .geometry
            .iter()
            .find(|geometry| geometry.name == "Edited Block")
            .unwrap();
        assert_eq!(block.transform.position, [8.0, 9.0, 10.0]);
    }
}
