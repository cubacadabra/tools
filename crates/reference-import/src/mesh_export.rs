use super::*;

#[derive(Clone, Copy)]
pub(crate) struct StaticMeshVertex {
    pub(crate) position: [f32; 3],
    pub(crate) normal: [f32; 3],
    pub(crate) color: [u8; 4],
}

pub(crate) struct StaticMeshGroup {
    pub(crate) material: String,
    pub(crate) vertices: Vec<StaticMeshVertex>,
}

pub fn export_reference_mesh(options: &MeshExportOptions) -> Result<MeshExportResult, String> {
    if !options.scale.is_finite() || options.scale <= 0.0 {
        return Err("export scale must be finite and positive".to_owned());
    }
    let scene = read_reference_scene(&options.scene_path)?;
    let overrides = mesh_overrides::load(options.mesh_overrides.as_deref())?;
    let local_frame = if options.local_space {
        let root = options.instance_root.as_deref().ok_or_else(|| {
            "--local-space requires --instance-root so the source frame can be inferred".to_owned()
        })?;
        Some(reference_instance_frame(&scene, root)?)
    } else {
        None
    };
    if options.local_space && options.origin != [0.0; 3] {
        return Err("--origin cannot be combined with --local-space".to_owned());
    }
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
                && options
                    .instance_root
                    .as_deref()
                    .is_none_or(|root| geometry.path.starts_with(&format!("{root}/")))
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
            if let Some(frame) = local_frame {
                vertex.position =
                    inverse_rotate_vector(frame.rotation, sub3(vertex.position, frame.position));
                vertex.normal = inverse_rotate_vector(frame.rotation, vertex.normal);
            } else {
                vertex.position = sub3(vertex.position, options.origin);
            }
            vertex.position = scale3(vertex.position, options.scale);
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

pub(crate) fn append_static_geometry(
    vertices: &mut Vec<StaticMeshVertex>,
    geometry: &GeometryInstance,
) {
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

pub(crate) fn append_static_triangle<const N: usize>(
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

pub(crate) fn rotate_vector(rows: [[f32; 3]; 3], vector: [f32; 3]) -> [f32; 3] {
    [
        dot3(rows[0], vector),
        dot3(rows[1], vector),
        dot3(rows[2], vector),
    ]
}

fn inverse_rotate_vector(rows: [[f32; 3]; 3], vector: [f32; 3]) -> [f32; 3] {
    [
        rows[0][0] * vector[0] + rows[1][0] * vector[1] + rows[2][0] * vector[2],
        rows[0][1] * vector[0] + rows[1][1] * vector[1] + rows[2][1] * vector[2],
        rows[0][2] * vector[0] + rows[1][2] * vector[1] + rows[2][2] * vector[2],
    ]
}

pub(crate) fn add3(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
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

pub(crate) fn multiply3(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
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

pub(crate) fn channel(value: f32) -> u8 {
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

pub(crate) fn write_static_glb(path: &Path, groups: &[StaticMeshGroup]) -> Result<(), String> {
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
