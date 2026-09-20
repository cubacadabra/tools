use super::*;

pub(crate) fn validate_sdk_features(
    manifest: &Map<String, Value>,
    sdk_version: Option<&str>,
) -> Result<()> {
    let has_terrain = |value: Option<&Value>| {
        value
            .and_then(Value::as_object)
            .and_then(|terrain| terrain.get("operations"))
            .and_then(Value::as_array)
            .is_some_and(|operations| !operations.is_empty())
    };
    let mut world_values = manifest
        .get("worlds")
        .and_then(Value::as_object)
        .into_iter()
        .flat_map(|worlds| worlds.values())
        .filter_map(Value::as_object);
    let has_terrain = has_terrain(manifest.get("terrain"))
        || world_values
            .clone()
            .any(|world| has_terrain(world.get("terrain")));
    if has_terrain
        && sdk_version != Some(TERRAIN_SDK_VERSION)
        && sdk_version != Some(LEGACY_CURRENT_SDK_VERSION)
        && sdk_version != Some(CURRENT_SDK_VERSION)
    {
        return Err(BuildError(format!(
            "terrain operations require manifest.sdkVersion {TERRAIN_SDK_VERSION}, {LEGACY_CURRENT_SDK_VERSION}, or {CURRENT_SDK_VERSION}"
        )));
    }

    let has_sdk_05_field = |world: &Map<String, Value>| {
        let camera = world.get("camera").is_some_and(|value| !value.is_null());
        let horizontal_bounds = world
            .get("physics")
            .and_then(Value::as_object)
            .and_then(|physics| physics.get("horizontalBounds"))
            .is_some_and(|value| !value.is_null());
        camera || horizontal_bounds
    };
    let has_collision = manifest
        .get("collision")
        .is_some_and(|value| !value.is_null())
        || world_values
            .clone()
            .any(|world| world.get("collision").is_some_and(|value| !value.is_null()));
    let has_sdk_05_field = manifest
        .get("world")
        .and_then(Value::as_object)
        .is_some_and(has_sdk_05_field)
        || world_values.any(has_sdk_05_field);
    if (has_collision || has_sdk_05_field)
        && sdk_version != Some(LEGACY_CURRENT_SDK_VERSION)
        && sdk_version != Some(CURRENT_SDK_VERSION)
    {
        return Err(BuildError(format!(
            "collision, world.camera, and world.physics.horizontalBounds require manifest.sdkVersion {LEGACY_CURRENT_SDK_VERSION} or {CURRENT_SDK_VERSION}"
        )));
    }
    let mesh_scale_entries = manifest
        .get("worlds")
        .and_then(Value::as_object)
        .into_iter()
        .flat_map(|worlds| worlds.values())
        .filter_map(Value::as_object)
        .flat_map(|world| world.get("decorations"))
        .filter_map(Value::as_array)
        .flatten()
        .filter_map(|decoration| decoration.as_object())
        .filter_map(|decoration| decoration.get("scale3"));
    let mut has_non_uniform_mesh_scale = false;
    for scale3 in mesh_scale_entries {
        let values = scale3.as_array().ok_or_else(|| {
            BuildError(
                "mesh decoration scale3 must be an array of exactly three positive finite numbers"
                    .to_owned(),
            )
        })?;
        if values.len() != 3
            || values.iter().any(|value| {
                value
                    .as_f64()
                    .is_none_or(|value| !value.is_finite() || value < 0.05)
            })
        {
            return Err(BuildError(
                "mesh decoration scale3 must be an array of exactly three positive finite numbers >= 0.05"
                    .to_owned(),
            ));
        }
        has_non_uniform_mesh_scale = true;
    }
    if has_non_uniform_mesh_scale && sdk_version != Some(CURRENT_SDK_VERSION) {
        return Err(BuildError(format!(
            "mesh scale3 requires manifest.sdkVersion {CURRENT_SDK_VERSION}"
        )));
    }
    Ok(())
}

pub(crate) fn absolute_path(path: &Path) -> Result<PathBuf> {
    let candidate = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(io_error("could not resolve current directory"))?
            .join(path)
    };
    if candidate.exists() {
        return fs::canonicalize(candidate).map_err(io_error("could not resolve path"));
    }
    let mut missing = Vec::new();
    let mut existing = candidate.as_path();
    while !existing.exists() {
        let name = existing
            .file_name()
            .ok_or_else(|| BuildError(format!("could not resolve path: {}", path.display())))?;
        missing.push(name.to_owned());
        existing = existing
            .parent()
            .ok_or_else(|| BuildError(format!("could not resolve path: {}", path.display())))?;
    }
    let mut resolved = fs::canonicalize(existing).map_err(io_error("could not resolve path"))?;
    for component in missing.iter().rev() {
        resolved.push(component);
    }
    Ok(resolved)
}

pub(crate) fn required_string<'a>(object: &'a Map<String, Value>, key: &str) -> Result<&'a str> {
    object
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| BuildError(format!("manifest.{key} must be present and a string")))
}

pub(crate) fn valid_game_id(value: &str) -> bool {
    (3..=64).contains(&value.len())
        && value.chars().enumerate().all(|(index, c)| {
            c.is_ascii_lowercase()
                || c.is_ascii_digit()
                || (c == '-'
                    && index > 0
                    && index + 1 < value.len()
                    && !value[..index].ends_with('-'))
        })
}

pub(crate) fn validate_version(value: &Value, field: &str) -> Result<()> {
    if value.as_u64().is_some_and(|number| number >= 1) {
        return Ok(());
    }
    let Some(value) = value.as_str() else {
        return Err(BuildError(format!(
            "{field} must be SemVer or a legacy positive integer"
        )));
    };
    let core = value.split_once('+').map_or(value, |(core, _)| core);
    let core = core.split_once('-').map_or(core, |(core, _)| core);
    let parts: Vec<_> = core.split('.').collect();
    if parts.len() != 3
        || parts
            .iter()
            .any(|part| part.is_empty() || part.parse::<u64>().is_err())
    {
        return Err(BuildError(format!(
            "{field} must be SemVer or a legacy positive integer"
        )));
    }
    Ok(())
}

pub(crate) fn resolve_effects_source(
    manifest: &mut Map<String, Value>,
    project_root: &Path,
) -> Result<()> {
    let Some(effects) = manifest.get("effects").cloned() else {
        return Ok(());
    };
    let Some(object) = effects.as_object() else {
        return Ok(());
    };
    let Some(source) = object.get("source").and_then(Value::as_str) else {
        return Ok(());
    };
    if object.len() != 1
        || source.is_empty()
        || source.starts_with('/')
        || source.contains("..")
        || !source.ends_with(".json")
    {
        return Err(BuildError(
            "manifest.effects.source must be a relative JSON path inside the game project"
                .to_owned(),
        ));
    }
    let path = project_root.join(source);
    let value = read_json(&path)?;
    if !value.is_object() {
        return Err(BuildError(
            "manifest.effects.source must contain a JSON object".to_owned(),
        ));
    }
    manifest.insert("effects".to_owned(), value);
    Ok(())
}

pub(crate) fn validate_assets(manifest: &Map<String, Value>, project_root: &Path) -> Result<()> {
    let Some(assets) = manifest.get("assets").and_then(Value::as_object) else {
        return Ok(());
    };
    for (kind, max, extensions, max_bytes) in [
        (
            "images",
            16usize,
            &["png", "jpg", "jpeg"][..],
            MAX_IMAGE_ASSET_BYTES,
        ),
        ("audio", 64usize, &["wav"][..], MAX_AUDIO_ASSET_BYTES),
        ("models", 64usize, &["glb"][..], MAX_MODEL_ASSET_BYTES),
    ] {
        let Some(entries) = assets.get(kind) else {
            continue;
        };
        let Some(entries) = entries.as_object() else {
            return Err(BuildError(format!(
                "manifest.assets.{kind} must be an object"
            )));
        };
        if entries.len() > max {
            return Err(BuildError(format!(
                "manifest.assets.{kind} cannot contain more than {max} assets"
            )));
        }
        for (id, definition) in entries {
            if id.is_empty()
                || id.len() > 64
                || !id
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
            {
                return Err(BuildError(format!(
                    "manifest.assets.{kind} ids must use ASCII letters, numbers, dots, dashes, or underscores"
                )));
            }
            let object = definition.as_object().ok_or_else(|| {
                BuildError(format!("manifest.assets.{kind}.{id} must be an object"))
            })?;
            let path = object.get("path").and_then(Value::as_str).ok_or_else(|| {
                BuildError(format!("manifest.assets.{kind}.{id}.path must be a string"))
            })?;
            if path.starts_with('/')
                || path.contains("..")
                || !path.starts_with("assets/")
                || !extensions
                    .iter()
                    .any(|extension| path.to_ascii_lowercase().ends_with(extension))
            {
                return Err(BuildError(format!(
                    "manifest.assets.{kind}.{id}.path is invalid: {path}"
                )));
            }
            let full = project_root.join(path);
            let metadata = fs::metadata(&full).map_err(|_| {
                BuildError(format!(
                    "manifest.assets.{kind}.{id}.path was not found: {path}"
                ))
            })?;
            if metadata.len() > max_bytes {
                return Err(BuildError(format!(
                    "manifest.assets.{kind}.{id}.path exceeds the asset size limit: {path}"
                )));
            }
            if kind == "models"
                && let Some(bounds) = object.get("bounds")
            {
                let values = bounds.as_array().ok_or_else(|| {
                    BuildError(format!(
                        "manifest.assets.models.{id}.bounds must be an array of three positive finite numbers"
                    ))
                })?;
                if values.len() != 3
                    || values.iter().any(|value| {
                        value
                            .as_f64()
                            .is_none_or(|value| !value.is_finite() || value <= 0.0)
                    })
                {
                    return Err(BuildError(format!(
                        "manifest.assets.models.{id}.bounds must be an array of three positive finite numbers"
                    )));
                }
            }
        }
    }
    Ok(())
}
