use super::*;

pub(super) fn generated_scene_nodes(
    reference: &ReferenceScene,
    root_id: &str,
    parent_id: Option<&str>,
    tree_depth: usize,
    focus_paths: &[String],
    source_index_reference: &str,
    promoted_instances: &[PromotedInstance],
    promoted_primitives: &[PromotedPrimitive],
    editable_parent_id: Option<&str>,
    editable_display_prefix: Option<&str>,
    editable_id_prefix: &str,
    promotion_statuses: &BTreeMap<String, PromotionStatus>,
) -> Vec<AuthoringNode> {
    let mut nodes = vec![AuthoringNode {
        id: root_id.to_owned(),
        parent_id: parent_id.map(str::to_owned),
        name: "Roblox Source".to_owned(),
        transform: Transform::default(),
        components: BTreeMap::new(),
        editor: EditorMetadata {
            visible: true,
            locked: true,
            lock_reason: Some(
                "Source hierarchy is read-only; extracted render assets are authored separately"
                    .to_owned(),
            ),
        },
        source: Some(SourceMetadata {
            format: "roblox".to_owned(),
            class: Some("SourceHierarchy".to_owned()),
            path: None,
            properties: BTreeMap::from([
                ("generatedBy".to_owned(), json!("import-roblox-scene")),
                ("sourceIndex".to_owned(), json!(source_index_reference)),
                ("instanceCount".to_owned(), json!(reference.instances.len())),
                ("geometryCount".to_owned(), json!(reference.geometry.len())),
            ]),
        }),
    }];
    let included = reference
        .instances
        .iter()
        .filter(|instance| {
            instance.path.split('/').count() <= tree_depth
                || focus_paths.iter().any(|focus| {
                    instance.path == *focus
                        || instance.path.starts_with(&format!("{focus}/"))
                        || focus.starts_with(&format!("{}/", instance.path))
                })
        })
        .map(|instance| instance.path.as_str())
        .collect::<BTreeSet<_>>();
    let promoted_paths = promoted_instances
        .iter()
        .map(|instance| instance.source_path.as_str())
        .chain(
            promoted_primitives
                .iter()
                .map(|primitive| primitive.source_path.as_str()),
        )
        .collect::<Vec<_>>();
    let (native_groups, native_group_ids) =
        native_group_hierarchy(reference, &promoted_paths, editable_parent_id);
    for instance in &reference.instances {
        if !included.contains(instance.path.as_str()) {
            continue;
        }
        let id = source_node_id(&instance.path);
        let parent = if included.contains(instance.parent_path.as_str()) {
            source_node_id(&instance.parent_path)
        } else {
            root_id.to_owned()
        };
        let mut properties =
            BTreeMap::from([("generatedBy".to_owned(), json!("import-roblox-scene"))]);
        let geometry_count = reference
            .geometry
            .iter()
            .filter(|geometry| geometry.path == instance.path)
            .count();
        if geometry_count > 0 {
            properties.insert("geometryCount".to_owned(), json!(geometry_count));
        }
        if let Some(status) = promotion_statuses.get(&instance.path) {
            match status {
                PromotionStatus::Promoted { node_id } => {
                    properties.insert("promotionStatus".to_owned(), json!("promoted"));
                    properties.insert("nativeRepresentationId".to_owned(), json!(node_id));
                }
                PromotionStatus::Fallback { reason } => {
                    properties.insert("promotionStatus".to_owned(), json!("fallback"));
                    properties.insert("promotionReason".to_owned(), json!(reason));
                }
            }
        }
        if let Some(native_id) = native_group_ids.get(&instance.path) {
            properties.insert("representation".to_owned(), json!("native-group"));
            properties.insert("nativeRepresentationId".to_owned(), json!(native_id));
        }
        nodes.push(AuthoringNode {
            id,
            parent_id: Some(parent),
            name: instance.name.clone(),
            transform: Transform {
                position: instance
                    .transform
                    .as_ref()
                    .map(|transform| transform.position)
                    .unwrap_or([0.0; 3]),
                ..Transform::default()
            },
            components: BTreeMap::new(),
            editor: EditorMetadata {
                visible: true,
                locked: true,
                lock_reason: Some(
                    "Imported source node has no editable native representation yet".to_owned(),
                ),
            },
            source: Some(SourceMetadata {
                format: "roblox".to_owned(),
                class: Some(instance.class.clone()),
                path: Some(instance.path.clone()),
                properties,
            }),
        });
    }
    nodes.extend(native_groups);
    let native_parent = |source_path: &str| {
        reference
            .instances
            .iter()
            .find(|instance| instance.path == source_path)
            .and_then(|instance| native_group_ids.get(&instance.parent_path))
            .cloned()
            .or_else(|| editable_parent_id.map(str::to_owned))
    };
    let mut editable_number = 0;
    for promoted in promoted_instances {
        let Some(instance) = reference
            .instances
            .iter()
            .find(|instance| instance.path == promoted.source_path)
        else {
            continue;
        };
        editable_number += 1;
        let id = format!("{}-{}", editable_id_prefix, short_hash(&instance.path));
        let display_name = editable_display_prefix
            .map(|prefix| format!("{prefix} {editable_number}"))
            .unwrap_or_else(|| format!("{} — {}", instance.name, editable_number));
        let mut properties = BTreeMap::from([
            ("generatedBy".to_owned(), json!("import-roblox-scene")),
            (
                "representation".to_owned(),
                json!("editable-imported-instance"),
            ),
            (
                "sourceFrame".to_owned(),
                json!("inferred-from-first-descendant-geometry"),
            ),
            ("regenerationPolicy".to_owned(), json!("source-generated")),
        ]);
        properties.insert("asset".to_owned(), json!(promoted.mesh));
        nodes.push(AuthoringNode {
            id,
            parent_id: native_parent(&instance.path),
            name: display_name,
            transform: Transform {
                position: promoted.position,
                rotation: promoted.rotation,
                scale: promoted.scale,
            },
            components: {
                let mut components =
                    BTreeMap::from([("render".to_owned(), json!({"mesh": promoted.mesh}))]);
                if let Some(asset) = &promoted.collision_asset {
                    components.insert(
                        "collision".to_owned(),
                        json!({"kind": "mesh", "asset": asset}),
                    );
                }
                components
            },
            editor: EditorMetadata::default(),
            source: Some(SourceMetadata {
                format: "roblox".to_owned(),
                class: Some(instance.class.clone()),
                path: Some(instance.path.clone()),
                properties,
            }),
        });
    }
    let mut primitive_number = 0;
    for promoted in promoted_primitives {
        let Some(instance) = reference
            .instances
            .iter()
            .find(|instance| instance.path == promoted.source_path)
        else {
            continue;
        };
        primitive_number += 1;
        let id = primitive_node_id(&instance.path);
        let mut properties = BTreeMap::from([
            ("generatedBy".to_owned(), json!("import-roblox-scene")),
            ("representation".to_owned(), json!("primitive")),
            ("sourceColor".to_owned(), json!(promoted.material.clone())),
            ("sourceCanCollide".to_owned(), json!(promoted.can_collide)),
            ("sourceCastShadow".to_owned(), json!(promoted.cast_shadow)),
            (
                "sourceFrame".to_owned(),
                json!("geometry-transform-and-bounds"),
            ),
            ("regenerationPolicy".to_owned(), json!("source-generated")),
        ]);
        if let Some(material) = reference
            .geometry
            .iter()
            .find(|geometry| geometry.path == promoted.source_path)
            .and_then(|geometry| geometry.material.name.as_deref())
        {
            properties.insert("sourceMaterial".to_owned(), json!(material));
        }
        let mut components = BTreeMap::from([("primitive".to_owned(), {
            let mut primitive = json!({
                "shape": "box",
                "size": promoted.size,
                "color": promoted.material.clone(),
                "collidable": promoted.can_collide,
                "castShadow": promoted.cast_shadow,
            });
            if let Some(runtime_material) = promoted.runtime_material {
                primitive["material"] = json!(runtime_material);
                properties.insert("material".to_owned(), json!(runtime_material));
            }
            primitive
        })]);
        if promoted.can_collide {
            components.insert("collision".to_owned(), json!({"kind": "box"}));
        }
        nodes.push(AuthoringNode {
            id,
            parent_id: native_parent(&instance.path),
            name: format!("{} — {}", instance.name, primitive_number),
            transform: Transform {
                position: promoted.position,
                rotation: promoted.rotation,
                scale: [1.0; 3],
            },
            components,
            editor: EditorMetadata::default(),
            source: Some(SourceMetadata {
                format: "roblox".to_owned(),
                class: Some(instance.class.clone()),
                path: Some(instance.path.clone()),
                properties,
            }),
        });
    }
    nodes
}

pub(super) fn native_group_hierarchy(
    reference: &ReferenceScene,
    promoted_paths: &[&str],
    fallback_parent_id: Option<&str>,
) -> (Vec<AuthoringNode>, BTreeMap<String, String>) {
    let instances = reference
        .instances
        .iter()
        .map(|instance| (instance.path.as_str(), instance))
        .collect::<BTreeMap<_, _>>();
    let mut paths = BTreeSet::new();
    for promoted_path in promoted_paths {
        let Some(instance) = instances.get(promoted_path) else {
            continue;
        };
        let mut ancestor_path = instance.parent_path.as_str();
        while !ancestor_path.is_empty() {
            let Some(ancestor) = instances.get(ancestor_path) else {
                break;
            };
            if matches!(ancestor.class.as_str(), "Model" | "Folder") {
                paths.insert(ancestor.path.clone());
            }
            ancestor_path = ancestor.parent_path.as_str();
        }
    }
    let mut ordered_paths = paths.into_iter().collect::<Vec<_>>();
    ordered_paths.sort_by_key(|path| (path.split('/').count(), path.clone()));
    let mut ids = BTreeMap::new();
    let mut nodes = Vec::new();
    for path in ordered_paths {
        let Some(instance) = instances.get(path.as_str()) else {
            continue;
        };
        let id = format!("imported-group-{}", short_hash(&path));
        let parent_id = instances
            .get(instance.parent_path.as_str())
            .and_then(|parent| ids.get(parent.path.as_str()))
            .cloned()
            .or_else(|| fallback_parent_id.map(str::to_owned));
        ids.insert(path.clone(), id.clone());
        nodes.push(AuthoringNode {
            id,
            parent_id,
            name: instance.name.clone(),
            transform: Transform::default(),
            components: BTreeMap::new(),
            editor: EditorMetadata::default(),
            source: Some(SourceMetadata {
                format: "roblox".to_owned(),
                class: Some(instance.class.clone()),
                path: Some(path),
                properties: BTreeMap::from([
                    ("generatedBy".to_owned(), json!("import-roblox-scene")),
                    ("representation".to_owned(), json!("native-group")),
                    ("regenerationPolicy".to_owned(), json!("source-generated")),
                ]),
            }),
        });
    }
    (nodes, ids)
}
