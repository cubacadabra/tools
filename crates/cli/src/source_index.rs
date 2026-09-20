use super::*;

pub(crate) fn migrate_source_index_command(args: &[String]) -> Result<(), String> {
    let mut input = None;
    let mut output = None;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--input" => {
                index += 1;
                input = Some(PathBuf::from(required_arg(args, index, "--input")?));
            }
            "--output" => {
                index += 1;
                output = Some(PathBuf::from(required_arg(args, index, "--output")?));
            }
            value => return Err(format!("unknown migrate-source-index option {value}")),
        }
        index += 1;
    }
    let input = input.ok_or_else(|| "migrate-source-index requires --input".to_owned())?;
    let output = output.ok_or_else(|| "migrate-source-index requires --output".to_owned())?;
    let legacy: Value = serde_json::from_slice(
        &fs::read(&input)
            .map_err(|error| format!("could not read {}: {error}", input.display()))?,
    )
    .map_err(|error| format!("could not decode {}: {error}", input.display()))?;
    if legacy.get("formatVersion").and_then(Value::as_u64) != Some(1)
        || legacy.get("kind").and_then(Value::as_str) != Some("cubacadabra-roblox-source-hierarchy")
    {
        return Err(format!(
            "{} is not a supported v1 source hierarchy index",
            input.display()
        ));
    }
    let nodes = legacy
        .get("nodes")
        .and_then(Value::as_array)
        .ok_or_else(|| format!("{} has no v1 nodes array", input.display()))?
        .iter()
        .map(normalize_legacy_source_node)
        .collect::<Result<Vec<_>, _>>()?;
    let shard_count = write_sharded_source_index(
        &output,
        legacy.get("source").cloned().unwrap_or(Value::Null),
        legacy
            .get("instanceCount")
            .and_then(Value::as_u64)
            .unwrap_or(nodes.len() as u64) as usize,
        legacy
            .get("geometryCount")
            .and_then(Value::as_u64)
            .unwrap_or(0) as usize,
        nodes,
    )?;
    println!(
        "Migrated source hierarchy: {} -> {} shard files in {}",
        input.display(),
        shard_count,
        output.display()
    );
    Ok(())
}

pub(crate) fn normalize_legacy_source_node(node: &Value) -> Result<Value, String> {
    let path = node
        .get("path")
        .and_then(Value::as_str)
        .ok_or_else(|| "legacy source node is missing path".to_owned())?;
    let parent_path = node.get("parentPath").and_then(Value::as_str).unwrap_or("");
    let class = node
        .get("class")
        .and_then(Value::as_str)
        .ok_or_else(|| format!("legacy source node {path:?} is missing class"))?;
    let name = node
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| format!("legacy source node {path:?} is missing name"))?;
    let mut normalized = json!({
        "id": source_node_id(path),
        "parent": if parent_path.is_empty() {
            Value::Null
        } else {
            json!(source_node_id(parent_path))
        },
        "sourceSegment": source_segment(path),
        "class": class,
        "name": name,
    });
    if let Some(transform) = node.get("transform").filter(|value| !value.is_null()) {
        normalized["transform"] = transform.clone();
    }
    if let Some(geometry_count) = node
        .get("geometryCount")
        .and_then(Value::as_u64)
        .filter(|count| *count > 0)
    {
        normalized["geometryCount"] = json!(geometry_count);
    }
    Ok(normalized)
}
pub(crate) fn write_source_index(path: &Path, reference: &ReferenceScene) -> Result<usize, String> {
    let mut geometry_by_path = BTreeMap::<&str, usize>::new();
    for geometry in &reference.geometry {
        *geometry_by_path.entry(geometry.path.as_str()).or_default() += 1;
    }
    let nodes = reference
        .instances
        .iter()
        .map(|instance| {
            let geometry_count = geometry_by_path
                .get(instance.path.as_str())
                .copied()
                .unwrap_or(0);
            let mut node = json!({
                "id": source_node_id(&instance.path),
                "parent": if instance.parent_path.is_empty() {
                    Value::Null
                } else {
                    json!(source_node_id(&instance.parent_path))
                },
                "sourceSegment": source_segment(&instance.path),
                "class": instance.class,
                "name": instance.name,
            });
            if let Some(transform) = &instance.transform {
                node["transform"] = json!(transform);
            }
            if geometry_count > 0 {
                node["geometryCount"] = json!(geometry_count);
            }
            node
        })
        .collect::<Vec<_>>();

    write_sharded_source_index(
        path,
        json!(reference.source),
        reference.instances.len(),
        reference.geometry.len(),
        nodes,
    )
}

pub(crate) fn write_sharded_source_index(
    path: &Path,
    source: Value,
    instance_count: usize,
    geometry_count: usize,
    nodes: Vec<Value>,
) -> Result<usize, String> {
    let node_store_name = if path.file_name().and_then(|name| name.to_str()) == Some("index.json") {
        "nodes".to_owned()
    } else {
        format!(
            "{}.nodes",
            path.file_stem()
                .and_then(|stem| stem.to_str())
                .unwrap_or("source-hierarchy")
        )
    };
    let node_store = path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(&node_store_name);
    if node_store.exists() {
        fs::remove_dir_all(&node_store)
            .map_err(|error| format!("could not replace {}: {error}", node_store.display()))?;
    }
    fs::create_dir_all(&node_store)
        .map_err(|error| format!("could not create {}: {error}", node_store.display()))?;

    let shards = shard_source_nodes(&nodes)?;
    for shard in &shards {
        let shard_path = node_store.join(&shard.file_name);
        if let Some(parent) = shard_path.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| format!("could not create {}: {error}", parent.display()))?;
        }
        write_text(&shard_path, &format!("{}\n", shard.encoded))?;
    }

    let index = json!({
        "formatVersion": SOURCE_INDEX_FORMAT_VERSION,
        "kind": "cubacadabra-roblox-source-hierarchy",
        "source": source,
        "instanceCount": instance_count,
        "geometryCount": geometry_count,
        "nodeStore": {
            "kind": "sharded-json",
            "path": node_store_name,
            "targetBytes": SOURCE_SHARD_TARGET_BYTES,
            "maxBytes": SOURCE_SHARD_MAX_BYTES,
            "shards": shards.iter().map(|shard| json!({
                "path": shard.file_name,
                "nodeCount": shard.node_count,
                "bytes": shard.bytes,
            })).collect::<Vec<_>>(),
        },
    });
    let encoded = serde_json::to_string_pretty(&index)
        .map_err(|error| format!("could not encode source hierarchy: {error}"))?;
    write_text(path, &format!("{encoded}\n"))?;
    Ok(shards.len())
}

pub(crate) struct SourceShard {
    pub(crate) file_name: String,
    pub(crate) node_count: usize,
    pub(crate) bytes: usize,
    pub(crate) encoded: String,
}

pub(crate) fn shard_source_nodes(nodes: &[Value]) -> Result<Vec<SourceShard>, String> {
    let mut buckets = BTreeMap::<String, Vec<Value>>::new();
    for node in nodes {
        let id = node
            .get("id")
            .and_then(Value::as_str)
            .ok_or_else(|| "source node is missing a string id".to_owned())?;
        let prefix = id
            .strip_prefix("source-")
            .and_then(|value| value.chars().next())
            .ok_or_else(|| format!("source node id {id:?} has no hash prefix"))?;
        buckets
            .entry(prefix.to_string())
            .or_default()
            .push(node.clone());
    }

    let mut shards = Vec::new();
    for (prefix, nodes) in buckets {
        split_source_bucket(prefix, nodes, &mut shards)?;
    }
    Ok(shards)
}

pub(crate) fn split_source_bucket(
    prefix: String,
    nodes: Vec<Value>,
    shards: &mut Vec<SourceShard>,
) -> Result<(), String> {
    let encoded = serde_json::to_string_pretty(&nodes)
        .map_err(|error| format!("could not encode source shard {prefix}: {error}"))?;
    let bytes = encoded.len() + 1;
    if bytes <= SOURCE_SHARD_TARGET_BYTES || prefix.len() >= 16 {
        if bytes > SOURCE_SHARD_MAX_BYTES {
            return Err(format!(
                "source shard {prefix} is {} bytes, above the {} byte limit",
                bytes, SOURCE_SHARD_MAX_BYTES
            ));
        }
        let file_name = if prefix.len() == 1 {
            format!("{prefix}.json")
        } else {
            format!("{}/{prefix}.json", &prefix[..1])
        };
        shards.push(SourceShard {
            file_name,
            node_count: nodes.len(),
            bytes,
            encoded,
        });
        return Ok(());
    }

    let next_index = prefix.len();
    let mut children = BTreeMap::<char, Vec<Value>>::new();
    for node in nodes {
        let id = node
            .get("id")
            .and_then(Value::as_str)
            .ok_or_else(|| "source node is missing a string id".to_owned())?;
        let hash = id
            .strip_prefix("source-")
            .ok_or_else(|| format!("source node id {id:?} has no hash prefix"))?;
        let next = hash
            .chars()
            .nth(next_index)
            .ok_or_else(|| format!("source node id {id:?} has a short hash"))?;
        children.entry(next).or_default().push(node);
    }
    for (next, child_nodes) in children {
        split_source_bucket(format!("{prefix}{next}"), child_nodes, shards)?;
    }
    Ok(())
}

pub(crate) fn source_segment(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

pub(crate) fn source_dataset_name(place_name: &str) -> String {
    let stem = Path::new(place_name)
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or(place_name);
    let sanitized = stem
        .chars()
        .map(|value| {
            if value.is_ascii_alphanumeric() || matches!(value, '-' | '_') {
                value
            } else {
                '_'
            }
        })
        .collect::<String>();
    if sanitized.is_empty() {
        "import".to_owned()
    } else {
        sanitized
    }
}

pub(crate) fn source_index_reference(scene_path: &Path, source_index: &Path) -> String {
    let base = scene_path.parent().unwrap_or_else(|| Path::new("."));
    let value = if source_index.is_absolute() {
        source_index
            .strip_prefix(base)
            .unwrap_or(source_index)
            .to_path_buf()
    } else {
        source_index.to_path_buf()
    };
    value.to_string_lossy().replace('\\', "/")
}

pub(crate) fn source_node_id(path: &str) -> String {
    format!("source-{}", short_hash(path))
}

pub(crate) fn short_hash(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))[..16].to_owned()
}

pub(crate) fn write_text(path: &Path, content: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("could not create {}: {error}", parent.display()))?;
    }
    fs::write(path, content).map_err(|error| format!("could not write {}: {error}", path.display()))
}
