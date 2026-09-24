use crate::{GeometryInstance, ImportOptions, ReferenceScene, load_reference};
use cubacadabra_scene::{AuthoringNode, AuthoringScene, EditorMetadata, SourceMetadata, Transform};
use glam::{EulerRot, Mat3, Quat};
use rbx_dom_weak::types::{CFrame, Color3, Enum, Matrix3, Variant, Vector3};
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
    merge_reference(reference, base_scene, source_file, place_path)
}

fn merge_reference(
    reference: ReferenceScene,
    base_scene: &AuthoringScene,
    source_file: &str,
    place_path: &Path,
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
    let dom = decode_xml(place_path)?;
    let dom_paths = instance_refs_by_path(&dom)?;
    let promoted = reference
        .geometry
        .iter()
        .filter(|geometry| {
            promotable_part(geometry) && crate::is_roblox_workspace_path(&reference, &geometry.path)
        })
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
            if instance.class == "Workspace" || instance.parent_path.is_empty() {
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
        let has_unrepresented_descendant = reference.instances.iter().any(|candidate| {
            candidate.path.starts_with(&format!("{}/", instance.path))
                && !promoted_paths.contains(candidate.path.as_str())
                && !matches!(candidate.class.as_str(), "Model" | "Folder")
        });
        scene.nodes.push(AuthoringNode {
            id,
            parent_id: Some(parent),
            name: instance.name.clone(),
            transform: Transform::default(),
            components: BTreeMap::new(),
            editor: EditorMetadata {
                visible: true,
                locked: has_unrepresented_descendant,
                lock_reason: has_unrepresented_descendant.then_some(
                    "This Roblox group contains preserved objects that Studio cannot transform safely yet"
                        .to_owned(),
                ),
            },
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
        let source_property = |name: &str| {
            dom_paths
                .get(geometry.path.as_str())
                .and_then(|reference| dom.get_by_ref(*reference))
                .is_some_and(|instance| instance.properties.contains_key(&ustr(name)))
        };
        scene.nodes.push(AuthoringNode {
            id,
            parent_id: Some(parent),
            name: geometry.name.clone(),
            transform: Transform {
                position: geometry.transform.position,
                rotation: crate::roblox_source_rotation_to_euler(geometry.transform.rotation),
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
                    (
                        "sourceHasColor".to_owned(),
                        json!(source_property("Color") || source_property("Color3uint8")),
                    ),
                    (
                        "sourceHasCanCollide".to_owned(),
                        json!(source_property("CanCollide")),
                    ),
                    (
                        "sourceHasCastShadow".to_owned(),
                        json!(source_property("CastShadow")),
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
    crate::validate_roblox_native_part(geometry).is_ok()
}

pub fn roblox_source_file(scene: &AuthoringScene) -> Option<&str> {
    roblox_source_files(scene).into_iter().next()
}

pub fn roblox_source_files(scene: &AuthoringScene) -> Vec<&str> {
    let mut seen = BTreeSet::new();
    scene
        .nodes
        .iter()
        .filter_map(|node| {
            node.source
                .as_ref()
                .filter(|source| source.format == "roblox")?
                .properties
                .get("sourceFile")?
                .as_str()
        })
        .filter(|source_file| seen.insert(*source_file))
        .collect()
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
    let selected_import_id = preserved_source.map(source_import_id).transpose()?;
    if let Some(selected_import_id) = selected_import_id.as_deref()
        && scene.nodes.iter().any(|node| {
            node.source
                .as_ref()
                .is_some_and(|source| source.format == "roblox")
        })
    {
        let known_import = scene.nodes.iter().any(|node| {
            node.source.as_ref().is_some_and(|source| {
                source.format == "roblox"
                    && source.properties.get("importId").and_then(Value::as_str)
                        == Some(selected_import_id)
            })
        });
        if !known_import {
            let expected_sha = scene.nodes.iter().find_map(|node| {
                node.source
                    .as_ref()?
                    .properties
                    .get("sourceSha256")
                    .and_then(Value::as_str)
            });
            return Err(format!(
                "the preserved Roblox source has changed since import (expected SHA {}, found hash {selected_import_id}); re-import or restore the preserved source",
                expected_sha.unwrap_or("recorded import identity")
            ));
        }
    }
    let mut dom = match preserved_source {
        Some(path) => decode_xml(path)?,
        None => WeakDom::new(InstanceBuilder::new("DataModel")),
    };
    let original_instance_count = dom.descendants().count();
    let paths = instance_refs_by_path(&dom)?;
    let world_transforms = scene.world_transforms()?;
    let mut report = RobloxExportReport::default();
    let mut source_linked = BTreeSet::new();
    let mut skipped_imports = BTreeSet::new();

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
        source_linked.insert(node.id.as_str());
        if let Some(selected) = selected_import_id.as_deref() {
            let Some(node_import) = source.properties.get("importId").and_then(Value::as_str)
            else {
                report.warnings.push(format!(
                    "{}: preserved Roblox source link has no import identity; the native edit was not applied",
                    node.name
                ));
                continue;
            };
            if node_import != selected {
                skipped_imports.insert(node_import.to_owned());
                continue;
            }
        }
        let Some(referent) = paths.get(path).copied() else {
            report.warnings.push(format!(
                "{}: preserved Roblox source path is missing; the native edit was not applied",
                node.name
            ));
            continue;
        };
        let Some(instance) = dom.get_by_ref_mut(referent) else {
            continue;
        };
        if let Some(expected_class) = source.class.as_deref()
            && instance.class.as_str() != expected_class
        {
            report.warnings.push(format!(
                "{}: preserved Roblox source class is {}, not {}; the native edit was not applied",
                node.name, instance.class, expected_class
            ));
            continue;
        }
        let name_changed = instance.name != node.name;
        if node.components.contains_key("primitive") {
            if instance.class.as_str() != "Part" {
                report.warnings.push(format!(
                    "{}: only Roblox Part instances can receive primitive edits; the source was preserved",
                    node.name
                ));
                continue;
            }
            let Some(world) = world_transforms.get(&node.id) else {
                continue;
            };
            let properties_changed = update_part(instance, node, world, &mut report)?;
            if name_changed {
                instance.name = node.name.clone();
            }
            if name_changed || properties_changed {
                report.updated_parts += 1;
            }
        } else if name_changed {
            instance.name = node.name.clone();
        }
    }

    if !skipped_imports.is_empty() {
        report.warnings.push(format!(
            "{} other Roblox import(s) were not merged into the selected preserved source",
            skipped_imports.len()
        ));
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
        rbx_xml::to_writer(
            &mut writer,
            &dom,
            dom.root().children(),
            rbx_xml::EncodeOptions::new()
                .property_behavior(rbx_xml::EncodePropertyBehavior::WriteUnknown),
        )
        .map_err(|error| {
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
) -> Result<bool, String> {
    let primitive = node.components["primitive"]
        .as_object()
        .ok_or_else(|| format!("scene node {} primitive must be an object", node.id))?;
    let size = json_vector3(primitive.get("size"))
        .ok_or_else(|| format!("scene node {} primitive has no valid size", node.id))?;
    let size = multiply(size, world.scale);
    let mut changed = false;
    let desired_cframe = cframe(world);
    if !cframe_matches(instance.properties.get(&ustr("CFrame")), &desired_cframe) {
        instance
            .properties
            .insert(ustr("CFrame"), desired_cframe.into());
        changed = true;
    }
    let desired_size = Vector3::new(size[0], size[1], size[2]);
    if !vector3_matches(
        instance.properties.get(&ustr("Size")),
        desired_size,
        0.00001,
    ) {
        instance
            .properties
            .insert(ustr("Size"), desired_size.into());
        changed = true;
    }
    let can_collide = primitive
        .get("collidable")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let source_had_can_collide = source_property_was_present(node, "sourceHasCanCollide");
    if (source_had_can_collide || !can_collide)
        && !bool_matches(instance.properties.get(&ustr("CanCollide")), can_collide)
    {
        instance
            .properties
            .insert(ustr("CanCollide"), can_collide.into());
        changed = true;
    }
    let cast_shadow = primitive
        .get("castShadow")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let source_had_cast_shadow = source_property_was_present(node, "sourceHasCastShadow");
    if (source_had_cast_shadow || !cast_shadow)
        && !bool_matches(instance.properties.get(&ustr("CastShadow")), cast_shadow)
    {
        instance
            .properties
            .insert(ustr("CastShadow"), cast_shadow.into());
        changed = true;
    }
    if let Some(color) = primitive
        .get("color")
        .and_then(Value::as_str)
        .and_then(parse_color)
    {
        let color = Color3::new(color[0], color[1], color[2]);
        let source_had_color = source_property_was_present(node, "sourceHasColor");
        let default_color = Color3::new(0.64, 0.64, 0.64);
        if (source_had_color || !color_matches(Some(&Variant::Color3(color)), default_color))
            && !color_matches(instance.properties.get(&ustr("Color")), color)
        {
            instance.properties.insert(ustr("Color"), color.into());
            changed = true;
        }
    } else if primitive.get("color").is_some() {
        report.warnings.push(format!(
            "{}: named color could not be resolved for Roblox; preserved the source color",
            node.name
        ));
    }
    if let Some(material_name) = primitive.get("material").and_then(Value::as_str) {
        if let Some(material) = roblox_material_value(material_name) {
            if !enum_matches(instance.properties.get(&ustr("Material")), material) {
                instance
                    .properties
                    .insert(ustr("Material"), Enum::from_u32(material).into());
                changed = true;
            }
        } else {
            report.warnings.push(format!(
                "{}: material {material_name:?} has no Roblox equivalent; preserved the source material",
                node.name
            ));
        }
    }
    Ok(changed)
}

fn source_property_was_present(node: &AuthoringNode, key: &str) -> bool {
    node.source
        .as_ref()
        .and_then(|source| source.properties.get(key))
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

fn cframe_matches(value: Option<&rbx_dom_weak::types::Variant>, expected: &CFrame) -> bool {
    let actual = match value {
        Some(rbx_dom_weak::types::Variant::CFrame(value)) => value,
        Some(rbx_dom_weak::types::Variant::OptionalCFrame(Some(value))) => value,
        _ => return false,
    };
    vector3_close(actual.position, expected.position, 0.00001)
        && vector3_close(actual.orientation.x, expected.orientation.x, 0.00001)
        && vector3_close(actual.orientation.y, expected.orientation.y, 0.00001)
        && vector3_close(actual.orientation.z, expected.orientation.z, 0.00001)
}

fn vector3_matches(
    value: Option<&rbx_dom_weak::types::Variant>,
    expected: Vector3,
    tolerance: f32,
) -> bool {
    matches!(value, Some(rbx_dom_weak::types::Variant::Vector3(actual)) if vector3_close(*actual, expected, tolerance))
}

fn vector3_close(left: Vector3, right: Vector3, tolerance: f32) -> bool {
    (left.x - right.x).abs() <= tolerance
        && (left.y - right.y).abs() <= tolerance
        && (left.z - right.z).abs() <= tolerance
}

fn bool_matches(value: Option<&rbx_dom_weak::types::Variant>, expected: bool) -> bool {
    matches!(value, Some(rbx_dom_weak::types::Variant::Bool(actual)) if *actual == expected)
}

fn enum_matches(value: Option<&rbx_dom_weak::types::Variant>, expected: u32) -> bool {
    matches!(value, Some(rbx_dom_weak::types::Variant::Enum(actual)) if actual.to_u32() == expected)
        || matches!(value, Some(rbx_dom_weak::types::Variant::EnumItem(actual)) if actual.value == expected)
}

fn color_matches(value: Option<&rbx_dom_weak::types::Variant>, expected: Color3) -> bool {
    let actual = match value {
        Some(rbx_dom_weak::types::Variant::Color3(value)) => *value,
        Some(rbx_dom_weak::types::Variant::Color3uint8(value)) => (*value).into(),
        _ => return false,
    };
    let tolerance = 0.5 / 255.0 + f32::EPSILON;
    (actual.r - expected.r).abs() <= tolerance
        && (actual.g - expected.g).abs() <= tolerance
        && (actual.b - expected.b).abs() <= tolerance
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
    let material = match primitive.get("material").and_then(Value::as_str) {
        Some(material_name) => match roblox_material_value(material_name) {
            Some(material) => material,
            None => {
                report.warnings.push(format!(
                    "{}: material {material_name:?} has no Roblox equivalent; exported as Plastic",
                    node.name
                ));
                256
            }
        },
        None => 256,
    };
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
    rbx_xml::from_reader(
        reader,
        rbx_xml::DecodeOptions::new()
            .property_behavior(rbx_xml::DecodePropertyBehavior::ReadUnknown),
    )
    .map_err(|error| format!("could not decode Roblox XML {}: {error}", path.display()))
}

fn source_import_id(path: &Path) -> Result<String, String> {
    let bytes = fs::read(path)
        .map_err(|error| format!("could not read Roblox XML {}: {error}", path.display()))?;
    let source_sha256 = format!("{:x}", Sha256::digest(bytes));
    Ok(short_hash(&source_sha256))
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
    use rbx_dom_weak::types::Variant;
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

    const KITCHEN_SINK_PLACE: &str =
        include_str!("../tests/fixtures/roblox-roundtrip/kitchen-sink.rbxlx");
    const GENERICITY_AUDIT_PLACE: &str =
        include_str!("../tests/fixtures/roblox-roundtrip/services-references-and-types.rbxlx");

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
        let yard = imported
            .scene
            .nodes
            .iter()
            .find(|node| node.name == "Yard")
            .expect("source model should be represented as a native group");
        assert!(yard.editor.locked);
        assert_eq!(
            roblox_source_file(&imported.scene),
            Some("imports/roblox/yard/source.rbxlx")
        );
        let encoded = serialize_authoring_scene(&imported.scene).unwrap();
        assert!(encoded.contains("Preserved Roblox Source"));
        assert!(encoded.contains("roblox-part-"));
    }

    #[test]
    fn workspace_class_not_service_name_controls_native_promotion() {
        let temp = tempdir().unwrap();
        let source = temp.path().join("renamed-workspace.rbxlx");
        fs::write(
            &source,
            PLACE.replace(
                "<string name=\"Name\">Workspace</string>",
                "<string name=\"Name\">World Root</string>",
            ),
        )
        .unwrap();

        let imported = import_roblox_authoring_scene(
            &source,
            &base_scene(),
            "imports/roblox/renamed-workspace/source.rbxlx",
        )
        .unwrap();

        assert_eq!(imported.editable_parts, 1);
        assert!(
            imported
                .scene
                .nodes
                .iter()
                .any(|node| node.name == "Block" && node.components.contains_key("primitive"))
        );
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

    #[test]
    fn export_preserves_absent_default_part_properties() {
        let temp = tempdir().unwrap();
        let source = temp.path().join("defaults.rbxlx");
        let output = temp.path().join("defaults-export.rbxlx");
        let source_text = PLACE
            .replace(
                "<Color3 name=\"Color\"><R>1</R><G>0</G><B>0</B></Color3>",
                "",
            )
            .replace("<bool name=\"CanCollide\">true</bool>", "")
            .replace("<bool name=\"CastShadow\">true</bool>", "");
        fs::write(&source, source_text).unwrap();
        let mut imported = import_roblox_authoring_scene(
            &source,
            &base_scene(),
            "imports/roblox/defaults/source.rbxlx",
        )
        .unwrap();
        let block = imported
            .scene
            .nodes
            .iter_mut()
            .find(|node| node.components.contains_key("primitive"))
            .unwrap();
        block.transform.position = [9.0, 0.0, 0.0];
        write_roblox_place(&imported.scene, Some(&source), &output).unwrap();
        let exported = fs::read_to_string(output).unwrap();
        assert!(!exported.contains("name=\"Color\""));
        assert!(!exported.contains("name=\"CanCollide\""));
        assert!(!exported.contains("name=\"CastShadow\""));
    }

    #[test]
    fn duplicated_source_part_exports_as_a_new_part() {
        let temp = tempdir().unwrap();
        let source = temp.path().join("duplicate.rbxlx");
        let output = temp.path().join("duplicate-export.rbxlx");
        fs::write(&source, PLACE).unwrap();
        let mut imported = import_roblox_authoring_scene(
            &source,
            &base_scene(),
            "imports/roblox/duplicate/source.rbxlx",
        )
        .unwrap();
        let original = imported
            .scene
            .nodes
            .iter()
            .find(|node| node.components.contains_key("primitive"))
            .cloned()
            .unwrap();
        let mut copy = original.clone();
        copy.id = "block-copy".to_owned();
        copy.name = "Block Copy".to_owned();
        copy.source = None;
        copy.transform.position[0] = 8.0;
        imported.scene.nodes.push(copy);
        let report = write_roblox_place(&imported.scene, Some(&source), &output).unwrap();
        assert_eq!(report.added_parts, 1);
        let exported = load_reference(&ImportOptions {
            place_path: output,
            terrain_path: None,
            project_path: None,
            output_path: PathBuf::new(),
        })
        .unwrap();
        assert!(exported.geometry.iter().any(|geometry| {
            geometry.name == "Block Copy" && geometry.transform.position == [8.0, 2.0, 3.0]
        }));
    }

    #[test]
    fn unsupported_native_material_reports_warning_instead_of_silent_plastic() {
        let temp = tempdir().unwrap();
        let output = temp.path().join("material-export.rbxlx");
        let mut scene = base_scene();
        scene.nodes.push(AuthoringNode {
            id: "ground".to_owned(),
            parent_id: Some("world".to_owned()),
            name: "Ground".to_owned(),
            transform: Transform::default(),
            components: BTreeMap::from([(
                "primitive".to_owned(),
                json!({"shape": "box", "size": [2, 1, 2], "material": "builtin:ground"}),
            )]),
            editor: EditorMetadata::default(),
            source: None,
        });
        let report = write_roblox_place(&scene, None, &output).unwrap();
        assert!(
            report
                .warnings
                .iter()
                .any(|warning| warning.contains("builtin:ground") && warning.contains("Plastic"))
        );
    }

    #[test]
    fn changed_preserved_source_is_rejected_before_merge() {
        let temp = tempdir().unwrap();
        let source = temp.path().join("changed.rbxlx");
        let output = temp.path().join("changed-export.rbxlx");
        fs::write(&source, PLACE).unwrap();
        let imported = import_roblox_authoring_scene(
            &source,
            &base_scene(),
            "imports/roblox/changed/source.rbxlx",
        )
        .unwrap();
        fs::write(&source, PLACE.replace("keep-me", "changed")).unwrap();
        let error = write_roblox_place(&imported.scene, Some(&source), &output).unwrap_err();
        assert!(error.contains("preserved Roblox source has changed"));
    }

    #[test]
    fn missing_source_import_identity_is_not_applied() {
        let temp = tempdir().unwrap();
        let source = temp.path().join("identity.rbxlx");
        let output = temp.path().join("identity-export.rbxlx");
        fs::write(&source, PLACE).unwrap();
        let mut imported = import_roblox_authoring_scene(
            &source,
            &base_scene(),
            "imports/roblox/identity/source.rbxlx",
        )
        .unwrap();
        let block = imported
            .scene
            .nodes
            .iter_mut()
            .find(|node| node.components.contains_key("primitive"))
            .unwrap();
        block.source.as_mut().unwrap().properties.remove("importId");
        let report = write_roblox_place(&imported.scene, Some(&source), &output).unwrap();
        assert!(
            report
                .warnings
                .iter()
                .any(|warning| warning.contains("no import identity"))
        );
        assert_eq!(report.updated_parts, 0);
    }

    #[test]
    fn export_preserves_nested_unsupported_content_and_references_while_editing_a_part() {
        let temp = tempdir().unwrap();
        let source = temp.path().join("kitchen-sink.rbxlx");
        let output = temp.path().join("kitchen-sink-export.rbxlx");
        fs::write(&source, KITCHEN_SINK_PLACE).unwrap();
        let source_dom = decode_xml(&source).unwrap();
        let mut imported = import_roblox_authoring_scene(
            &source,
            &base_scene(),
            "imports/roblox/kitchen-sink/source.rbxlx",
        )
        .unwrap();
        let block = imported
            .scene
            .nodes
            .iter_mut()
            .find(|node| node.components.contains_key("primitive"))
            .unwrap();
        block.transform.position = [8.0, 9.0, 10.0];
        block.name = "Edited Kitchen Sink Part".to_owned();
        block.components.get_mut("primitive").unwrap()["size"] = json!([8, 4, 12]);

        let report = write_roblox_place(&imported.scene, Some(&source), &output).unwrap();
        assert_eq!(report.updated_parts, 1);
        assert_eq!(report.added_parts, 0);
        assert_eq!(report.omitted_nodes, 0);

        let exported_dom = decode_xml(&output).unwrap();
        let exported_part = named_instance(&exported_dom, "Edited Kitchen Sink Part");
        assert_eq!(exported_part.class.as_str(), "Part");
        let Some(Variant::CFrame(cframe)) = exported_part.properties.get(&ustr("CFrame")) else {
            panic!("edited Part has no CFrame");
        };
        assert_eq!(cframe.position, Vector3::new(8.0, 9.0, 10.0));
        assert_eq!(
            exported_part.properties.get(&ustr("Size")),
            Some(&Variant::Vector3(Vector3::new(8.0, 4.0, 12.0)))
        );
        assert_eq!(
            exported_part.properties.get(&ustr("Material")),
            Some(&Variant::Enum(Enum::from_u32(512)))
        );
        assert_eq!(
            encoded_attributes(exported_part),
            encoded_attributes(named_instance(&source_dom, "Kitchen Sink Part"))
        );

        for name in [
            "Effect Socket",
            "Sparks",
            "Glow",
            "Marker",
            "Hum",
            "Controller",
            "Mode",
        ] {
            assert_preserved_instance(&source_dom, &exported_dom, name);
        }
        assert_eq!(
            parent_name(
                &exported_dom,
                named_instance(&exported_dom, "Effect Socket")
            ),
            "Edited Kitchen Sink Part"
        );
        for name in [
            "Marker",
            "Hum",
            "Controller",
            "Mode",
            "EffectSocketReference",
        ] {
            assert_eq!(
                parent_name(&exported_dom, named_instance(&exported_dom, name)),
                "Edited Kitchen Sink Part"
            );
        }
        for name in ["Sparks", "Glow"] {
            assert_eq!(
                parent_name(&exported_dom, named_instance(&exported_dom, name)),
                "Effect Socket"
            );
        }
        assert_eq!(
            child_names(
                named_instance(&source_dom, "Kitchen Sink Part"),
                &source_dom
            ),
            child_names(exported_part, &exported_dom)
        );
        assert_eq!(
            child_names(named_instance(&source_dom, "Effect Socket"), &source_dom),
            child_names(
                named_instance(&exported_dom, "Effect Socket"),
                &exported_dom
            )
        );

        let object_value = named_instance(&exported_dom, "EffectSocketReference");
        assert_eq!(object_value.class.as_str(), "ObjectValue");
        let Some(Variant::Ref(target)) = object_value.properties.get(&ustr("Value")) else {
            panic!("ObjectValue.Value was not preserved as an instance reference");
        };
        assert_eq!(
            exported_dom.get_by_ref(*target).unwrap().name,
            "Effect Socket"
        );
    }

    #[test]
    fn export_preserves_services_references_unknown_classes_and_property_types() {
        let temp = tempdir().unwrap();
        let source = temp.path().join("genericity-audit.rbxlx");
        let output = temp.path().join("genericity-audit-export.rbxlx");
        fs::write(&source, GENERICITY_AUDIT_PLACE).unwrap();
        let source_dom = decode_xml(&source).unwrap();
        let mut imported = import_roblox_authoring_scene(
            &source,
            &base_scene(),
            "imports/roblox/genericity-audit/source.rbxlx",
        )
        .unwrap();
        assert_eq!(imported.editable_parts, 2);
        let block = imported
            .scene
            .nodes
            .iter_mut()
            .find(|node| node.name == "Audit Part")
            .unwrap();
        block.transform.position = [20.0, 30.0, 40.0];
        block.name = "Edited Audit Part".to_owned();
        block.components.get_mut("primitive").unwrap()["size"] = json!([8, 10, 12]);

        let report = write_roblox_place(&imported.scene, Some(&source), &output).unwrap();
        assert_eq!(report.updated_parts, 1);
        assert_eq!(report.added_parts, 0);
        assert_eq!(report.omitted_nodes, 0);
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);

        let exported_dom = decode_xml(&output).unwrap();
        assert_dom_semantics(
            &source_dom,
            &exported_dom,
            &BTreeMap::from([("Audit Part", "Edited Audit Part")]),
            Some("Audit Part"),
        );
        let edited = named_instance(&exported_dom, "Edited Audit Part");
        let Some(Variant::CFrame(cframe)) = edited.properties.get(&ustr("CFrame")) else {
            panic!("edited Part has no CFrame");
        };
        assert_eq!(cframe.position, Vector3::new(20.0, 30.0, 40.0));
        assert_eq!(
            edited.properties.get(&ustr("Size")),
            Some(&Variant::Vector3(Vector3::new(8.0, 10.0, 12.0)))
        );
    }

    #[test]
    fn export_scopes_overlapping_paths_to_the_selected_preserved_source() {
        let temp = tempdir().unwrap();
        let source_a = temp.path().join("source-a.rbxlx");
        let source_b = temp.path().join("source-b.rbxlx");
        let output = temp.path().join("source-a-export.rbxlx");
        fs::write(&source_a, PLACE.replace("keep-me", "source-a")).unwrap();
        fs::write(&source_b, PLACE.replace("keep-me", "source-b")).unwrap();
        let imported_a = import_roblox_authoring_scene(
            &source_a,
            &base_scene(),
            "imports/roblox/source-a/source.rbxlx",
        )
        .unwrap();
        let mut imported_b = import_roblox_authoring_scene(
            &source_b,
            &imported_a.scene,
            "imports/roblox/source-b/source.rbxlx",
        )
        .unwrap();
        assert_eq!(roblox_source_files(&imported_b.scene).len(), 2);
        for node in &mut imported_b.scene.nodes {
            let source_file = node
                .source
                .as_ref()
                .and_then(|source| source.properties.get("sourceFile"))
                .and_then(Value::as_str);
            if node.components.contains_key("primitive") {
                match source_file {
                    Some("imports/roblox/source-a/source.rbxlx") => {
                        node.name = "Edited Source A".to_owned()
                    }
                    Some("imports/roblox/source-b/source.rbxlx") => {
                        node.name = "Edited Source B".to_owned()
                    }
                    _ => {}
                }
            }
        }

        let report = write_roblox_place(&imported_b.scene, Some(&source_a), &output).unwrap();
        assert_eq!(report.updated_parts, 1);
        assert_eq!(report.added_parts, 0);
        assert_eq!(report.warnings.len(), 1);
        let exported = decode_xml(&output).unwrap();
        assert_eq!(named_instance(&exported, "Edited Source A").class, "Part");
        assert!(
            exported
                .descendants()
                .all(|instance| instance.name != "Edited Source B")
        );
        assert_eq!(
            named_instance(&exported, "Yard")
                .properties
                .get(&ustr("CustomState")),
            Some(&Variant::String("source-a".to_owned()))
        );
    }

    #[test]
    fn missing_source_paths_warn_without_exporting_duplicate_native_parts() {
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
        block.name = "Must Not Become Native".to_owned();
        block.source.as_mut().unwrap().path =
            Some("Workspace:Workspace[1]/Part:Missing[1]".to_owned());

        let report = write_roblox_place(&imported.scene, Some(&source), &output).unwrap();
        assert_eq!(report.updated_parts, 0);
        assert_eq!(report.added_parts, 0);
        assert_eq!(report.warnings.len(), 1);
        let exported = decode_xml(&output).unwrap();
        assert_eq!(named_instance(&exported, "Block").class, "Part");
        assert!(
            exported
                .descendants()
                .all(|instance| instance.name != "Must Not Become Native")
        );
    }

    fn named_instance<'a>(dom: &'a WeakDom, name: &str) -> &'a Instance {
        dom.descendants()
            .find(|instance| instance.name == name)
            .unwrap_or_else(|| panic!("missing Roblox instance named {name}"))
    }

    fn parent_name<'a>(dom: &'a WeakDom, instance: &Instance) -> &'a str {
        &dom.get_by_ref(instance.parent()).unwrap().name
    }

    fn child_names(instance: &Instance, dom: &WeakDom) -> Vec<String> {
        instance
            .children()
            .iter()
            .map(|reference| dom.get_by_ref(*reference).unwrap().name.clone())
            .collect()
    }

    fn assert_preserved_instance(source: &WeakDom, exported: &WeakDom, name: &str) {
        let source = named_instance(source, name);
        let exported = named_instance(exported, name);
        assert_eq!(exported.class, source.class, "class changed for {name}");
        assert_eq!(
            exported.properties, source.properties,
            "properties changed for {name}"
        );
    }

    fn encoded_attributes(instance: &Instance) -> Vec<u8> {
        let Some(Variant::Attributes(attributes)) = instance.properties.get(&ustr("Attributes"))
        else {
            panic!("{} has no decoded Roblox attributes", instance.name);
        };
        let mut encoded = Vec::new();
        attributes.to_writer(&mut encoded).unwrap();
        encoded
    }

    fn assert_dom_semantics(
        source: &WeakDom,
        exported: &WeakDom,
        renamed: &BTreeMap<&str, &str>,
        edited_part: Option<&str>,
    ) {
        let source_paths = ordinal_paths(source);
        let exported_paths = ordinal_paths(exported);
        assert_instance_semantics(
            source,
            source.root_ref(),
            exported,
            exported.root_ref(),
            renamed,
            edited_part,
            &source_paths,
            &exported_paths,
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn assert_instance_semantics(
        source_dom: &WeakDom,
        source_ref: Ref,
        exported_dom: &WeakDom,
        exported_ref: Ref,
        renamed: &BTreeMap<&str, &str>,
        edited_part: Option<&str>,
        source_paths: &HashMap<Ref, Vec<usize>>,
        exported_paths: &HashMap<Ref, Vec<usize>>,
    ) {
        let source = source_dom.get_by_ref(source_ref).unwrap();
        let exported = exported_dom.get_by_ref(exported_ref).unwrap();
        assert_eq!(
            exported.class, source.class,
            "class changed for {}",
            source.name
        );
        assert_eq!(
            exported.name,
            renamed
                .get(source.name.as_str())
                .copied()
                .unwrap_or(&source.name),
            "name changed unexpectedly for {}",
            source.name
        );
        let excluded = if edited_part == Some(source.name.as_str()) {
            &["CFrame", "Size"][..]
        } else {
            &[][..]
        };
        assert_property_semantics(source, exported, excluded, source_paths, exported_paths);
        assert_eq!(
            exported.children().len(),
            source.children().len(),
            "child count changed for {}",
            source.name
        );
        for (source_child, exported_child) in
            source.children().iter().zip(exported.children().iter())
        {
            assert_instance_semantics(
                source_dom,
                *source_child,
                exported_dom,
                *exported_child,
                renamed,
                edited_part,
                source_paths,
                exported_paths,
            );
        }
    }

    fn assert_property_semantics(
        source: &Instance,
        exported: &Instance,
        excluded: &[&str],
        source_paths: &HashMap<Ref, Vec<usize>>,
        exported_paths: &HashMap<Ref, Vec<usize>>,
    ) {
        let source_names = source
            .properties
            .keys()
            .filter(|name| !excluded.contains(&name.as_str()))
            .map(ToString::to_string)
            .collect::<BTreeSet<_>>();
        let exported_names = exported
            .properties
            .keys()
            .filter(|name| !excluded.contains(&name.as_str()))
            .map(ToString::to_string)
            .collect::<BTreeSet<_>>();
        assert_eq!(
            exported_names, source_names,
            "property names changed for {}",
            source.name
        );
        for name in source_names {
            let source_value = source.properties.get(&ustr(&name)).unwrap();
            let exported_value = exported.properties.get(&ustr(&name)).unwrap();
            match (source_value, exported_value) {
                (Variant::Ref(source_ref), Variant::Ref(exported_ref)) => assert_eq!(
                    exported_paths.get(exported_ref),
                    source_paths.get(source_ref),
                    "reference target changed for {}.{name}",
                    source.name
                ),
                _ => assert_eq!(
                    format!("{exported_value:?}"),
                    format!("{source_value:?}"),
                    "property changed for {}.{name}",
                    source.name
                ),
            }
        }
    }

    fn ordinal_paths(dom: &WeakDom) -> HashMap<Ref, Vec<usize>> {
        fn collect(
            dom: &WeakDom,
            reference: Ref,
            path: &mut Vec<usize>,
            output: &mut HashMap<Ref, Vec<usize>>,
        ) {
            output.insert(reference, path.clone());
            let instance = dom.get_by_ref(reference).unwrap();
            for (index, child) in instance.children().iter().enumerate() {
                path.push(index);
                collect(dom, *child, path, output);
                path.pop();
            }
        }

        let mut output = HashMap::new();
        collect(dom, dom.root_ref(), &mut Vec::new(), &mut output);
        output
    }
}
