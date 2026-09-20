//! Explicit, locally authored normalized meshes for otherwise unresolved source
//! asset IDs. No network lookup or game-specific mesh identity lives here.
use super::*;
use crate::mesh_export::{
    StaticMeshVertex, add3, append_static_triangle, channel, multiply3, rotate_vector,
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Document {
    format_version: u32,
    meshes: BTreeMap<String, Mesh>,
}

#[derive(Debug, Deserialize)]
pub(super) struct Mesh {
    vertices: Vec<[f32; 3]>,
    triangles: Vec<[usize; 3]>,
}

pub(super) fn load(path: Option<&Path>) -> Result<BTreeMap<String, Mesh>, String> {
    let Some(path) = path else {
        return Ok(BTreeMap::new());
    };
    let metadata = fs::metadata(path).map_err(|error| error.to_string())?;
    if metadata.len() > 32 * 1024 * 1024 {
        return Err("mesh overrides exceed 32 MiB".to_owned());
    }
    let doc: Document = serde_json::from_reader(BufReader::new(
        File::open(path).map_err(|error| error.to_string())?,
    ))
    .map_err(|error| error.to_string())?;
    if doc.format_version != 1 {
        return Err("unsupported mesh override formatVersion".to_owned());
    }
    for (id, mesh) in &doc.meshes {
        if mesh.vertices.len() > 100_000
            || mesh.triangles.len() > 100_000
            || mesh
                .vertices
                .iter()
                .flatten()
                .any(|v| !v.is_finite() || v.abs() > 1.0)
            || mesh
                .triangles
                .iter()
                .flatten()
                .any(|i| *i >= mesh.vertices.len())
        {
            return Err(format!("invalid normalized mesh override {id}"));
        }
    }
    Ok(doc.meshes)
}

impl Mesh {
    pub(super) fn append(&self, output: &mut Vec<StaticMeshVertex>, geometry: &GeometryInstance) {
        let color = [
            channel(geometry.color[0]),
            channel(geometry.color[1]),
            channel(geometry.color[2]),
            channel(1.0 - geometry.transparency),
        ];
        for face in &self.triangles {
            let points = face.map(|i| {
                add3(
                    geometry.transform.position,
                    rotate_vector(
                        geometry.transform.rotation,
                        multiply3(self.vertices[i], geometry.size),
                    ),
                )
            });
            append_static_triangle(output, &points, [0, 1, 2], color);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_bad_indices_and_versions() {
        let directory = tempfile::tempdir().unwrap();
        let file = directory.path().join("meshes.json");
        fs::write(&file, r#"{"formatVersion":2,"meshes":{}}"#).unwrap();
        assert!(load(Some(&file)).unwrap_err().contains("formatVersion"));
        fs::write(
            &file,
            r#"{"formatVersion":1,"meshes":{"id":{"vertices":[[0,0,0]],"triangles":[[0,1,2]]}}}"#,
        )
        .unwrap();
        assert!(
            load(Some(&file))
                .unwrap_err()
                .contains("invalid normalized")
        );
    }
}
