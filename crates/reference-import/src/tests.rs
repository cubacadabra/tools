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
        export_reference_mesh(&mesh_options(first_mesh.clone(), first_collision.clone())).unwrap();
    export_reference_mesh(&mesh_options(second_mesh.clone(), second_collision.clone())).unwrap();
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
    let document: serde_json::Value = serde_json::from_slice(&bytes[20..20 + json_length]).unwrap();
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
