use super::export_helpers::*;
use super::*;

/// DOM is the base: supported Parts are updated and every unknown instance and
/// property remains untouched. Native Cubacadabra primitives are added under a
/// clearly named model in Workspace.
pub fn write_roblox_place(
    scene: &AuthoringScene,
    preserved_source: Option<&Path>,
    output_path: impl AsRef<Path>,
) -> Result<RobloxExportReport, String> {
    write_roblox_place_inner(scene, None, preserved_source, output_path)
}

/// Export a scene with its native world settings. For a new place, the
/// implicit Cubacadabra ground becomes a physical Roblox Part. An imported
/// place continues to use its preserved XML as the sole source of ground.
pub fn write_roblox_place_with_manifest(
    scene: &AuthoringScene,
    manifest: &Value,
    preserved_source: Option<&Path>,
    output_path: impl AsRef<Path>,
) -> Result<RobloxExportReport, String> {
    write_roblox_place_inner(scene, Some(manifest), preserved_source, output_path)
}

fn write_roblox_place_inner(
    scene: &AuthoringScene,
    manifest: Option<&Value>,
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

    if preserved_source.is_none()
        && let Some(manifest) = manifest
    {
        let (ground, terrain_omitted) = native_ground(scene, manifest)?;
        if terrain_omitted {
            report
                .warnings
                .push("Native terrain operations are not exported to Roblox yet".to_owned());
        }
        if let Some(ground) = ground {
            let workspace = find_or_create_workspace(&mut dom);
            dom.insert(workspace, ground);
            report.added_parts += 1;
        }
    }

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
    let mut exported_interactions = BTreeSet::new();
    let mut scripted_interactions = BTreeSet::new();
    let mut scripted_interaction_parts = 0;
    if !native_parts.is_empty() || manifest.is_some() {
        let workspace = find_or_create_workspace(&mut dom);
        let mut export_model = InstanceBuilder::new("Model").with_name("Cubacadabra Export");
        let mut model_parts = 0;
        for node in native_parts {
            let Some(world) = world_transforms.get(&node.id) else {
                continue;
            };
            match part_builder(node, world, &mut report) {
                Some(part) => {
                    export_model.add_child(part);
                    report.added_parts += 1;
                    model_parts += 1;
                }
                None => report.omitted_nodes += 1,
            }
        }
        if let Some(manifest) = manifest {
            for node in scene.nodes.iter().filter(|node| {
                node.components.contains_key("interaction")
                    && !source_linked.contains(node.id.as_str())
            }) {
                let Some(world) = world_transforms.get(&node.id) else {
                    continue;
                };
                let exported = interaction_visual_parts(scene, node, world, manifest, &mut report);
                if !exported.parts.is_empty() {
                    exported_interactions.insert(node.id.as_str());
                }
                if exported.fully_scripted {
                    scripted_interactions.insert(node.id.as_str());
                }
                scripted_interaction_parts += exported.scripted_parts;
                for part in exported.parts {
                    export_model.add_child(part);
                    report.added_parts += 1;
                    model_parts += 1;
                }
            }
        }
        if scripted_interaction_parts > 0 {
            export_model.add_child(
                InstanceBuilder::new("Script")
                    .with_name("Cubacadabra Interaction Runtime")
                    .with_property("Source", INTERACTION_SCRIPT),
            );
        }
        if model_parts > 0 {
            dom.insert(workspace, export_model);
        }
    }

    let static_interactions = exported_interactions.len() - scripted_interactions.len();
    if static_interactions > 0 {
        report.warnings.push(format!(
            "{} interaction visual(s) were exported as static Parts; interaction behavior and state changes require Roblox scripting",
            static_interactions
        ));
    }

    report.preserved_instances = original_instance_count.saturating_sub(report.updated_parts);
    report.omitted_nodes += scene
        .nodes
        .iter()
        .filter(|node| {
            !node.components.is_empty()
                && !node.components.contains_key("primitive")
                && !exported_interactions.contains(node.id.as_str())
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

fn interaction_visual_parts(
    scene: &AuthoringScene,
    node: &AuthoringNode,
    world: &cubacadabra_scene::AuthoringWorldTransform,
    manifest: &Value,
    report: &mut RobloxExportReport,
) -> InteractionVisualExport {
    let empty = || InteractionVisualExport {
        parts: Vec::new(),
        fully_scripted: false,
        scripted_parts: 0,
    };
    let Some(interaction) = node
        .components
        .get("interaction")
        .and_then(Value::as_object)
    else {
        return empty();
    };
    let Some(visual) = interaction.get("visual").and_then(Value::as_str) else {
        return empty();
    };
    if visual == "none" {
        return empty();
    }
    let Some(template) = manifest
        .get("effects")
        .and_then(|effects| effects.get("templates"))
        .and_then(|templates| templates.get(visual))
    else {
        report.warnings.push(format!(
            "{}: interaction visual {visual:?} was not found in the manifest",
            node.name
        ));
        return empty();
    };
    let Some(effect_nodes) = template.get("nodes").and_then(Value::as_array) else {
        report.warnings.push(format!(
            "{}: interaction visual {visual:?} has no effect nodes",
            node.name
        ));
        return empty();
    };
    let duration = template
        .get("duration")
        .and_then(Value::as_f64)
        .map(|value| value as f32)
        .filter(|value| value.is_finite() && *value > 0.0);
    let state = if effect_nodes.iter().any(|effect| {
        effect
            .get("visibleStates")
            .and_then(Value::as_array)
            .is_some_and(|states| {
                states
                    .iter()
                    .any(|state| state.as_str() == Some("available"))
            })
    }) {
        "available"
    } else {
        "default"
    };
    let interaction_color = interaction.get("color").and_then(Value::as_str);
    let selected_world = manifest_scene_world(scene, manifest).ok();
    let palette_color = |name: &str| {
        selected_world
            .and_then(|world| world.get("palette"))
            .and_then(|palette| palette.get(name))
            .or_else(|| {
                manifest
                    .get("palette")
                    .and_then(|palette| palette.get(name))
            })
            .and_then(Value::as_str)
            .and_then(parse_color)
    };
    let mut parts = Vec::new();
    let mut scripted_parts = 0;
    for (index, effect) in effect_nodes.iter().enumerate() {
        if effect
            .get("variants")
            .and_then(Value::as_array)
            .is_some_and(|variants| !variants.is_empty())
        {
            report.warnings.push(format!(
                "{}: interaction visual node {} uses variants and was omitted",
                node.name,
                index + 1
            ));
            continue;
        }
        let states = effect.get("visibleStates").and_then(Value::as_array);
        if states.is_some_and(|states| {
            !states.is_empty()
                && !states
                    .iter()
                    .any(|candidate| candidate.as_str() == Some(state))
        }) {
            continue;
        }
        if effect.get("shape").and_then(Value::as_str) != Some("box") {
            report.warnings.push(format!(
                "{}: interaction visual node {} is not a box and was omitted",
                node.name,
                index + 1
            ));
            continue;
        }
        let count = effect.get("count").and_then(Value::as_u64).unwrap_or(1);
        if count != 1 {
            report.warnings.push(format!(
                "{}: interaction visual node {} has {count} copies and was omitted",
                node.name,
                index + 1
            ));
            continue;
        }
        let position = json_vector3(effect.get("position")).unwrap_or([0.0; 3]);
        let Some(size) = json_vector3(effect.get("size")) else {
            report.warnings.push(format!(
                "{}: interaction visual node {} has no valid size and was omitted",
                node.name,
                index + 1
            ));
            continue;
        };
        let rotation = json_vector3(effect.get("rotation")).unwrap_or([0.0; 3]);
        if position
            .iter()
            .chain(size.iter())
            .chain(rotation.iter())
            .any(|value| !value.is_finite())
            || size.iter().any(|value| *value < 0.05)
        {
            report.warnings.push(format!(
                "{}: interaction visual node {} has invalid geometry and was omitted",
                node.name,
                index + 1
            ));
            continue;
        }
        let color_name = effect.get("color").and_then(Value::as_str).unwrap_or("");
        let color = if color_name == "$interaction" {
            interaction_color
                .and_then(parse_color)
                .or_else(|| interaction_color.and_then(palette_color))
        } else {
            parse_color(color_name).or_else(|| palette_color(color_name))
        }
        .unwrap_or([0.64; 3]);
        let opacity = effect
            .get("opacity")
            .and_then(Value::as_f64)
            .unwrap_or(1.0)
            .clamp(0.0, 1.0) as f32;
        let position = [
            world.position[0] + position[0],
            world.position[1] + position[1],
            world.position[2] + position[2],
        ];
        let name = if effect_nodes.len() == 1 {
            node.name.clone()
        } else {
            format!("{} Visual {}", node.name, index + 1)
        };
        let mut part = InstanceBuilder::new("Part")
            .with_name(name)
            .with_property("CFrame", cframe_values(position, rotation))
            .with_property("Size", Vector3::new(size[0], size[1], size[2]))
            .with_property("Color", Color3::new(color[0], color[1], color[2]))
            .with_property("Transparency", 1.0_f32 - opacity)
            .with_property("Anchored", true)
            .with_property("CanCollide", false)
            .with_property("CastShadow", false)
            .with_property("Material", Enum::from_u32(272));
        if let Some(tween) = interaction_tween(effect_nodes, effect, duration, world.position) {
            let attributes = Attributes::new()
                .with(
                    INTERACTION_TARGET_CFRAME,
                    cframe_values(tween.target_position, tween.target_rotation),
                )
                .with(
                    INTERACTION_TARGET_SIZE,
                    Vector3::new(
                        tween.target_size[0],
                        tween.target_size[1],
                        tween.target_size[2],
                    ),
                )
                .with(INTERACTION_TWEEN_DURATION, tween.duration as f64);
            part = part.with_property("Attributes", attributes);
            scripted_parts += 1;
        }
        parts.push(part);
    }
    InteractionVisualExport {
        fully_scripted: !parts.is_empty() && scripted_parts == parts.len(),
        parts,
        scripted_parts,
    }
}

fn interaction_tween(
    effect_nodes: &[Value],
    visible_effect: &Value,
    duration: Option<f32>,
    origin: [f32; 3],
) -> Option<InteractionTween> {
    let duration = duration?;
    let visible_position = json_vector3(visible_effect.get("position")).unwrap_or([0.0; 3]);
    let visible_size = json_vector3(visible_effect.get("size"))?;
    let visible_rotation = json_vector3(visible_effect.get("rotation")).unwrap_or([0.0; 3]);
    let mut matches = effect_nodes.iter().filter_map(|effect| {
        if effect
            .get("variants")
            .and_then(Value::as_array)
            .is_some_and(|value| !value.is_empty())
            || effect.get("shape").and_then(Value::as_str) != Some("box")
            || effect.get("count").and_then(Value::as_u64).unwrap_or(1) != 1
        {
            return None;
        }
        let position = json_vector3(effect.get("position")).unwrap_or([0.0; 3]);
        let size = json_vector3(effect.get("size"))?;
        let rotation = json_vector3(effect.get("rotation")).unwrap_or([0.0; 3]);
        if !vectors_close(position, visible_position)
            || !vectors_close(size, visible_size)
            || !vectors_close(rotation, visible_rotation)
        {
            return None;
        }
        let animation = effect.get("animation")?.as_object()?;
        let target = json_vector3(animation.get("travelTo"))?;
        let target_size = json_vector3(animation.get("travelSize")).unwrap_or(size);
        let target_rotation = json_vector3(animation.get("travelRotation")).unwrap_or(rotation);
        if target
            .iter()
            .chain(target_size.iter())
            .chain(target_rotation.iter())
            .any(|value| !value.is_finite())
            || target_size.iter().any(|value| *value < 0.05)
        {
            return None;
        }
        Some(InteractionTween {
            target_position: [
                origin[0] + target[0],
                origin[1] + target[1],
                origin[2] + target[2],
            ],
            target_size,
            target_rotation,
            duration,
        })
    });
    let tween = matches.next()?;
    matches.next().is_none().then_some(tween)
}

fn vectors_close(left: [f32; 3], right: [f32; 3]) -> bool {
    left.into_iter()
        .zip(right)
        .all(|(left, right)| (left - right).abs() <= 0.00001)
}
