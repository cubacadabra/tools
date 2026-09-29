use super::*;

pub(super) fn native_ground(
    scene: &AuthoringScene,
    manifest: &Value,
) -> Result<(Option<InstanceBuilder>, bool), String> {
    let world = manifest_scene_world(scene, manifest)?;
    let terrain = world.get("terrain");
    let has_terrain = terrain
        .and_then(|terrain| terrain.get("operations"))
        .and_then(Value::as_array)
        .is_some_and(|operations| !operations.is_empty());
    if has_terrain
        && terrain
            .and_then(|terrain| terrain.get("hideDefaultGround"))
            .and_then(Value::as_bool)
            == Some(true)
    {
        return Ok((None, true));
    }
    let size = world
        .get("world")
        .and_then(|settings| settings.get("groundSize"))
        .and_then(Value::as_f64)
        .unwrap_or(120.0)
        .max(10.0) as f32;
    let y = world
        .get("world")
        .and_then(|settings| settings.get("physics"))
        .and_then(|physics| physics.get("groundY"))
        .and_then(Value::as_f64)
        .unwrap_or(0.0) as f32;
    if !size.is_finite() || !y.is_finite() {
        return Err("ground size and height must be finite to export to Roblox".to_owned());
    }
    let color = world
        .get("palette")
        .and_then(|palette| palette.get("ground"))
        .and_then(Value::as_str)
        .and_then(parse_color)
        .unwrap_or([167.0 / 255.0, 189.0 / 255.0, 153.0 / 255.0]);
    let collidable = world
        .get("world")
        .and_then(|settings| settings.get("physics"))
        .and_then(|physics| physics.get("groundCollision"))
        .and_then(Value::as_bool)
        .unwrap_or(true);
    Ok((
        Some(
            InstanceBuilder::new("Part")
                .with_name("Ground")
                .with_property(
                    "CFrame",
                    CFrame::new(Vector3::new(0.0, y - 0.08, 0.0), Matrix3::identity()),
                )
                .with_property("Size", Vector3::new(size, 0.16, size))
                .with_property("Color", Color3::new(color[0], color[1], color[2]))
                .with_property("Anchored", true)
                .with_property("CanCollide", collidable)
                .with_property("Material", Enum::from_u32(256))
                .with_property("TopSurface", Enum::from_u32(0))
                .with_property("BottomSurface", Enum::from_u32(0)),
        ),
        has_terrain,
    ))
}

pub(super) fn manifest_scene_world<'a>(
    scene: &AuthoringScene,
    manifest: &'a Value,
) -> Result<&'a Value, String> {
    Ok(match scene.world_id.as_deref().unwrap_or("lobby") {
        "lobby" => manifest,
        world_id => manifest
            .get("worlds")
            .and_then(|worlds| worlds.get(world_id))
            .ok_or_else(|| format!("scene world `{world_id}` was not found in the manifest"))?,
    })
}

pub(super) fn update_part(
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

pub(super) fn source_property_was_present(node: &AuthoringNode, key: &str) -> bool {
    node.source
        .as_ref()
        .and_then(|source| source.properties.get(key))
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

pub(super) fn cframe_matches(
    value: Option<&rbx_dom_weak::types::Variant>,
    expected: &CFrame,
) -> bool {
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

pub(super) fn vector3_matches(
    value: Option<&rbx_dom_weak::types::Variant>,
    expected: Vector3,
    tolerance: f32,
) -> bool {
    matches!(value, Some(rbx_dom_weak::types::Variant::Vector3(actual)) if vector3_close(*actual, expected, tolerance))
}

pub(super) fn vector3_close(left: Vector3, right: Vector3, tolerance: f32) -> bool {
    (left.x - right.x).abs() <= tolerance
        && (left.y - right.y).abs() <= tolerance
        && (left.z - right.z).abs() <= tolerance
}

pub(super) fn bool_matches(value: Option<&rbx_dom_weak::types::Variant>, expected: bool) -> bool {
    matches!(value, Some(rbx_dom_weak::types::Variant::Bool(actual)) if *actual == expected)
}

pub(super) fn enum_matches(value: Option<&rbx_dom_weak::types::Variant>, expected: u32) -> bool {
    matches!(value, Some(rbx_dom_weak::types::Variant::Enum(actual)) if actual.to_u32() == expected)
        || matches!(value, Some(rbx_dom_weak::types::Variant::EnumItem(actual)) if actual.value == expected)
}

pub(super) fn color_matches(
    value: Option<&rbx_dom_weak::types::Variant>,
    expected: Color3,
) -> bool {
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

pub(super) fn part_builder(
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
    let mut part = InstanceBuilder::new("Part")
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
        .with_property("Material", Enum::from_u32(material));
    if primitive.get("shape").and_then(Value::as_str) == Some("sphere") {
        part = part.with_property("Shape", Enum::from_u32(0));
    }
    Some(part)
}

pub(super) fn cframe(world: &cubacadabra_scene::AuthoringWorldTransform) -> CFrame {
    cframe_values(world.position, world.rotation)
}

pub(super) fn cframe_values(position: [f32; 3], rotation: [f32; 3]) -> CFrame {
    let matrix = Mat3::from_quat(Quat::from_euler(
        EulerRot::XYZ,
        rotation[0],
        rotation[1],
        rotation[2],
    ));
    let rows = matrix.transpose().to_cols_array_2d();
    CFrame::new(
        Vector3::new(position[0], position[1], position[2]),
        Matrix3::new(
            Vector3::new(rows[0][0], rows[0][1], rows[0][2]),
            Vector3::new(rows[1][0], rows[1][1], rows[1][2]),
            Vector3::new(rows[2][0], rows[2][1], rows[2][2]),
        ),
    )
}

pub(super) fn find_or_create_workspace(dom: &mut WeakDom) -> Ref {
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

pub(super) fn instance_refs_by_path(dom: &WeakDom) -> Result<BTreeMap<String, Ref>, String> {
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

pub(super) fn decode_xml(path: &Path) -> Result<WeakDom, String> {
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

pub(super) fn source_import_id(path: &Path) -> Result<String, String> {
    let bytes = fs::read(path)
        .map_err(|error| format!("could not read Roblox XML {}: {error}", path.display()))?;
    let source_sha256 = format!("{:x}", Sha256::digest(bytes));
    Ok(short_hash(&source_sha256))
}

pub(super) fn unique_id(scene: &AuthoringScene, base: &str) -> String {
    if scene.nodes.iter().all(|node| node.id != base) {
        return base.to_owned();
    }
    (2..)
        .map(|suffix| format!("{base}-{suffix}"))
        .find(|candidate| scene.nodes.iter().all(|node| node.id != *candidate))
        .expect("scene IDs have a finite practical range")
}

pub(super) fn source_color(color: [f32; 3]) -> String {
    let channel = |value: f32| (value.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!(
        "#{:02X}{:02X}{:02X}",
        channel(color[0]),
        channel(color[1]),
        channel(color[2])
    )
}

pub(super) fn parse_color(value: &str) -> Option<[f32; 3]> {
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

pub(super) fn roblox_material_value(value: &str) -> Option<u32> {
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

pub(super) fn json_vector3(value: Option<&Value>) -> Option<[f32; 3]> {
    let values = value?.as_array()?;
    Some([
        values.first()?.as_f64()? as f32,
        values.get(1)?.as_f64()? as f32,
        values.get(2)?.as_f64()? as f32,
    ])
}

pub(super) fn multiply(left: [f32; 3], right: [f32; 3]) -> [f32; 3] {
    [left[0] * right[0], left[1] * right[1], left[2] * right[2]]
}

pub(super) fn short_hash(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))[..12].to_owned()
}

pub(super) fn escape_path_component(value: &str) -> String {
    value.replace('%', "%25").replace('/', "%2F")
}
