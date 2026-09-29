use super::super::*;
use super::support::*;

#[test]
fn native_ground_uses_selected_world_and_respects_hidden_ground() {
    let temp = tempdir().unwrap();
    let output = temp.path().join("ground.rbxlx");
    let scene = base_scene();
    let manifest = json!({
        "world": {"groundSize": 70},
        "palette": {"ground": "#20295D"},
        "worlds": {
            "world": {
                "world": {"groundSize": 80, "physics": {"groundY": 4}},
                "palette": {"ground": "#204060"}
            }
        }
    });
    let report = write_roblox_place_with_manifest(&scene, &manifest, None, &output).unwrap();
    assert_eq!(report.added_parts, 1);
    let dom = decode_xml(&output).unwrap();
    let ground = dom
        .descendants()
        .find(|instance| instance.name == "Ground")
        .unwrap();
    assert_eq!(ground.class.as_str(), "Part");
    assert_eq!(
        ground.properties.get(&ustr("Anchored")),
        Some(&Variant::Bool(true))
    );
    assert_eq!(
        ground.properties.get(&ustr("CanCollide")),
        Some(&Variant::Bool(true))
    );
    assert_eq!(
        ground.properties.get(&ustr("Size")),
        Some(&Variant::Vector3(Vector3::new(80.0, 0.16, 80.0)))
    );
    let Some(Variant::Color3uint8(color)) = ground.properties.get(&ustr("Color")) else {
        panic!("ground has no Color3uint8");
    };
    assert_eq!((color.r, color.g, color.b), (32, 64, 96));
    let Some(Variant::CFrame(cframe)) = ground.properties.get(&ustr("CFrame")) else {
        panic!("ground has no CFrame");
    };
    assert_eq!(cframe.position, Vector3::new(0.0, 3.92, 0.0));

    let default_world_color = json!({
        "palette": {"ground": "#20295D"},
        "worlds": {"world": {"palette": {}}}
    });
    write_roblox_place_with_manifest(&scene, &default_world_color, None, &output).unwrap();
    let dom = decode_xml(&output).unwrap();
    let ground = dom
        .descendants()
        .find(|instance| instance.name == "Ground")
        .unwrap();
    let Some(Variant::Color3uint8(color)) = ground.properties.get(&ustr("Color")) else {
        panic!("ground has no Color3uint8");
    };
    assert_eq!((color.r, color.g, color.b), (167, 189, 153));
    assert_eq!(
        ground.properties.get(&ustr("Material")),
        Some(&Variant::Enum(Enum::from_u32(256)))
    );
    for surface in ["TopSurface", "BottomSurface"] {
        assert_eq!(
            ground.properties.get(&ustr(surface)),
            Some(&Variant::Enum(Enum::from_u32(0)))
        );
    }

    let hidden = json!({"worlds": {"world": {"terrain": {
        "hideDefaultGround": true,
        "operations": [{"shape": "block", "operation": "fill", "position": [0, 0, 0], "size": [1, 1, 1], "material": "rock"}]
    }}}});
    let report = write_roblox_place_with_manifest(&scene, &hidden, None, &output).unwrap();
    assert_eq!(report.added_parts, 0);
    assert!(
        report
            .warnings
            .iter()
            .any(|warning| warning.contains("terrain"))
    );
    assert!(
        decode_xml(&output)
            .unwrap()
            .descendants()
            .all(|instance| instance.name != "Ground")
    );

    let no_terrain = json!({"worlds": {"world": {"terrain": {"hideDefaultGround": true}}}});
    let report = write_roblox_place_with_manifest(&scene, &no_terrain, None, &output).unwrap();
    assert_eq!(report.added_parts, 1);

    let no_collision = json!({"worlds": {"world": {"world": {
        "physics": {"groundCollision": false}
    }}}});
    write_roblox_place_with_manifest(&scene, &no_collision, None, &output).unwrap();
    let dom = decode_xml(&output).unwrap();
    let ground = dom
        .descendants()
        .find(|instance| instance.name == "Ground")
        .unwrap();
    assert_eq!(
        ground.properties.get(&ustr("CanCollide")),
        Some(&Variant::Bool(false))
    );
}

#[test]
fn available_box_interaction_visual_exports_as_a_static_part() {
    let temp = tempdir().unwrap();
    let output = temp.path().join("interaction.rbxlx");
    let mut scene = base_scene();
    scene.nodes.push(AuthoringNode {
        id: "pickup".to_owned(),
        parent_id: Some("world".to_owned()),
        name: "Pickup".to_owned(),
        transform: Transform {
            position: [2.0, 0.0, 3.0],
            ..Transform::default()
        },
        components: BTreeMap::from([(
            "interaction".to_owned(),
            json!({"visual": "pickup-visual", "color": "accent"}),
        )]),
        editor: EditorMetadata::default(),
        source: None,
    });
    let manifest = json!({
        "effects": {"templates": {"pickup-visual": {"nodes": [
            {"shape": "box", "position": [1, 0.25, 0], "size": [2, 0.1, 1], "rotation": [0, 0.5, 0], "color": "$interaction", "visibleStates": ["available"]},
            {"shape": "box", "position": [9, 9, 9], "size": [1, 1, 1], "color": "#ffffff", "visibleStates": ["default"]}
        ]}}},
        "worlds": {"world": {"palette": {"accent": "#123456"}}}
    });
    let report = write_roblox_place_with_manifest(&scene, &manifest, None, &output).unwrap();
    assert_eq!(report.added_parts, 2);
    assert_eq!(report.omitted_nodes, 0);
    assert!(
        report
            .warnings
            .iter()
            .any(|warning| warning.contains("static Parts"))
    );
    let dom = decode_xml(&output).unwrap();
    let pickup = dom
        .descendants()
        .find(|instance| instance.name.starts_with("Pickup Visual"))
        .unwrap();
    assert_eq!(pickup.class.as_str(), "Part");
    assert_eq!(
        pickup.properties.get(&ustr("Size")),
        Some(&Variant::Vector3(Vector3::new(2.0, 0.1, 1.0)))
    );
    assert_eq!(
        pickup.properties.get(&ustr("CanCollide")),
        Some(&Variant::Bool(false))
    );
    let Some(Variant::CFrame(cframe)) = pickup.properties.get(&ustr("CFrame")) else {
        panic!("interaction visual has no CFrame");
    };
    assert_eq!(cframe.position, Vector3::new(3.0, 0.25, 3.0));
    let Some(Variant::Color3uint8(color)) = pickup.properties.get(&ustr("Color")) else {
        panic!("interaction visual has no Color3uint8");
    };
    assert_eq!((color.r, color.g, color.b), (18, 52, 86));
}

#[test]
fn authored_interaction_travel_exports_touch_tween_script() {
    let temp = tempdir().unwrap();
    let output = temp.path().join("interaction-tween.rbxlx");
    let mut scene = base_scene();
    scene.nodes.push(AuthoringNode {
        id: "pickup".to_owned(),
        parent_id: Some("world".to_owned()),
        name: "Pickup".to_owned(),
        transform: Transform {
            position: [2.0, 0.0, 3.0],
            ..Transform::default()
        },
        components: BTreeMap::from([(
            "interaction".to_owned(),
            json!({"visual": "pickup-visual", "color": "accent"}),
        )]),
        editor: EditorMetadata::default(),
        source: None,
    });
    let manifest = json!({
        "effects": {"templates": {"pickup-visual": {
            "duration": 1.5,
            "nodes": [
                {"shape": "box", "position": [1, 0.25, 0], "size": [2, 0.1, 1], "rotation": [0, 0.5, 0], "color": "$interaction", "visibleStates": ["available"]},
                {"shape": "box", "position": [1, 0.25, 0], "size": [2, 0.1, 1], "rotation": [0, 0.5, 0], "color": "$interaction", "visibleStates": ["default"], "animation": {
                    "travelTo": [9, 9, 9], "travelSize": [1, 2, 3], "travelRotation": [0.1, 0.2, 0.3]
                }}
            ]
        }}},
        "worlds": {"world": {"palette": {"accent": "#123456"}}}
    });

    let report = write_roblox_place_with_manifest(&scene, &manifest, None, &output).unwrap();
    assert_eq!(report.added_parts, 2);
    assert!(
        report
            .warnings
            .iter()
            .all(|warning| !warning.contains("static Parts"))
    );

    let dom = decode_xml(&output).unwrap();
    let pickup = dom
        .descendants()
        .find(|instance| instance.name == "Pickup Visual 1")
        .unwrap();
    let Some(Variant::Attributes(attributes)) = pickup.properties.get(&ustr("Attributes")) else {
        panic!("animated interaction has no Roblox attributes");
    };
    let Some(Variant::CFrame(target)) = attributes.get(INTERACTION_TARGET_CFRAME) else {
        panic!("animated interaction has no target CFrame");
    };
    assert_eq!(target.position, Vector3::new(11.0, 9.0, 12.0));
    assert_eq!(
        attributes.get(INTERACTION_TARGET_SIZE),
        Some(&Variant::Vector3(Vector3::new(1.0, 2.0, 3.0)))
    );
    assert_eq!(
        attributes.get(INTERACTION_TWEEN_DURATION),
        Some(&Variant::Float64(1.5))
    );

    let script = dom
        .descendants()
        .find(|instance| instance.name == "Cubacadabra Interaction Runtime")
        .unwrap();
    assert_eq!(script.class.as_str(), "Script");
    let Some(Variant::String(source)) = script.properties.get(&ustr("Source")) else {
        panic!("generated interaction Script has no source");
    };
    assert!(source.contains("part.Touched:Connect"));
    assert!(source.contains("FindFirstChildOfClass(\"Humanoid\")"));
    assert!(source.contains("TweenService:Create"));
}

#[test]
fn preserved_place_does_not_gain_native_ground() {
    let temp = tempdir().unwrap();
    let source = temp.path().join("source.rbxlx");
    let output = temp.path().join("export.rbxlx");
    fs::write(&source, PLACE).unwrap();
    let imported =
        import_roblox_authoring_scene(&source, &base_scene(), "imports/roblox/source.rbxlx")
            .unwrap();
    write_roblox_place_with_manifest(
        &imported.scene,
        &json!({"worlds": {"world": {"world": {"groundSize": 80}}}}),
        Some(&source),
        &output,
    )
    .unwrap();
    assert!(
        decode_xml(&output)
            .unwrap()
            .descendants()
            .all(|instance| instance.name != "Ground")
    );
}

#[test]
fn imports_editable_parts_and_preserves_the_source_contract() {
    let temp = tempdir().unwrap();
    let source = temp.path().join("yard.rbxlx");
    fs::write(&source, PLACE).unwrap();
    let imported =
        import_roblox_authoring_scene(&source, &base_scene(), "imports/roblox/yard/source.rbxlx")
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
fn imports_unanchored_block_and_sphere_as_static_preview_geometry() {
    let temp = tempdir().unwrap();
    let source = temp.path().join("dynamic-shapes.rbxlx");
    let dynamic = PLACE.replace(
        "<bool name=\"Anchored\">true</bool>",
        "<bool name=\"Anchored\">false</bool>",
    );
    let sphere = r#"<Item class="Part" referent="RBX4">
    <Properties>
      <string name="Name">Sphere</string>
      <CoordinateFrame name="CFrame"><X>8</X><Y>2</Y><Z>3</Z><R00>1</R00><R01>0</R01><R02>0</R02><R10>0</R10><R11>1</R11><R12>0</R12><R20>0</R20><R21>0</R21><R22>1</R22></CoordinateFrame>
      <Vector3 name="Size"><X>4</X><Y>4</Y><Z>4</Z></Vector3>
      <Color3 name="Color"><R>0</R><G>1</G><B>0</B></Color3>
      <bool name="Anchored">false</bool><bool name="CanCollide">true</bool>
      <token name="Material">256</token><token name="Shape">0</token>
    </Properties>
  </Item>
  <Item class="ParticleEmitter""#;
    fs::write(
        &source,
        dynamic.replace("<Item class=\"ParticleEmitter\"", sphere),
    )
    .unwrap();
    let imported =
        import_roblox_authoring_scene(&source, &base_scene(), "imports/roblox/source.rbxlx")
            .unwrap();
    assert_eq!(imported.editable_parts, 2);
    let sphere = imported
        .scene
        .nodes
        .iter()
        .find(|node| node.name == "Sphere")
        .unwrap();
    assert_eq!(sphere.components["primitive"]["shape"], "sphere");
    assert_eq!(
        sphere.source.as_ref().unwrap().properties["sourceAnchored"],
        false
    );
    let mut manifest = json!({
        "id": "test", "version": "0.1.0", "sdkVersion": "0.3.0",
        "launch": {"destinationWorld": "world"}, "worlds": {"world": {}}
    });
    imported.scene.compile_into_manifest(&mut manifest).unwrap();
    assert_eq!(
        manifest["worlds"]["world"]["blocks"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        manifest["worlds"]["world"]["decorations"][0]["kind"],
        "sphere"
    );
    let output = temp.path().join("export.rbxlx");
    write_roblox_place_with_manifest(&imported.scene, &manifest, Some(&source), &output).unwrap();
    let exported = decode_xml(&output).unwrap();
    let sphere = named_instance(&exported, "Sphere");
    assert!(matches!(
        sphere.properties.get(&ustr("Shape")).or_else(|| sphere.properties.get(&ustr("shape"))),
        Some(Variant::Enum(shape)) if shape.to_u32() == 0
    ));
    assert!(matches!(
        sphere.properties.get(&ustr("Anchored")),
        Some(Variant::Bool(false))
    ));
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
