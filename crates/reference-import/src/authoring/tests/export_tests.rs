use super::super::*;
use super::support::*;

#[test]
fn export_updates_supported_parts_without_dropping_unknown_instances() {
    let temp = tempdir().unwrap();
    let source = temp.path().join("yard.rbxlx");
    let output = temp.path().join("yard-export.rbxlx");
    fs::write(&source, PLACE).unwrap();
    let mut imported =
        import_roblox_authoring_scene(&source, &base_scene(), "imports/roblox/yard/source.rbxlx")
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
    let mut imported =
        import_roblox_authoring_scene(&source, &base_scene(), "imports/roblox/yard/source.rbxlx")
            .unwrap();
    let block = imported
        .scene
        .nodes
        .iter_mut()
        .find(|node| node.components.contains_key("primitive"))
        .unwrap();
    block.name = "Must Not Become Native".to_owned();
    block.source.as_mut().unwrap().path = Some("Workspace:Workspace[1]/Part:Missing[1]".to_owned());

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
