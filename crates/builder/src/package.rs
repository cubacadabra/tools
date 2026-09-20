use super::*;

pub(crate) fn read_json(path: &Path) -> Result<Value> {
    let source = fs::read_to_string(path).map_err(io_error("could not read JSON file"))?;
    serde_json::from_str(&source)
        .map_err(|error| BuildError(format!("invalid JSON in {}: {error}", path.display())))
}

pub(crate) fn pretty_json(value: &Value) -> Result<String> {
    serde_json::to_string_pretty(value)
        .map(|value| value + "\n")
        .map_err(|error| BuildError(format!("could not serialize JSON: {error}")))
}
pub(crate) fn json_scalar(value: &Value) -> String {
    value
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| value.to_string())
}
pub(crate) fn write_text(path: &Path, text: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(io_error("could not create output directory"))?;
    }
    fs::write(path, text).map_err(io_error("could not write output file"))
}
pub(crate) fn io_error(context: &'static str) -> impl FnOnce(io::Error) -> BuildError {
    move |error| BuildError(format!("{context}: {error}"))
}

pub(crate) fn validate_output(output: &Path, source_root: &Path) -> Result<()> {
    if output.starts_with(source_root) || source_root.starts_with(output) {
        return Err(BuildError(
            "output directory cannot overlap the source directory in either direction".to_owned(),
        ));
    }
    Ok(())
}

pub(crate) fn validate_existing_output(output: &Path) -> Result<()> {
    if !output.exists() {
        return Ok(());
    }
    if !output.is_dir() {
        return Err(BuildError(format!(
            "output exists and is not a directory: {}",
            output.display()
        )));
    }
    let marker = fs::read_to_string(output.join(BUILD_MARKER)).map_err(|_| {
        BuildError(format!(
            "refusing to replace existing output without a Cubacadabra build marker: {}",
            output.display()
        ))
    })?;
    if marker != BUILD_MARKER_CONTENT {
        return Err(BuildError(format!(
            "refusing to replace existing output with an invalid Cubacadabra build marker: {}",
            output.display()
        )));
    }
    Ok(())
}

pub(crate) fn unique_sibling(path: &Path, label: &str) -> Result<PathBuf> {
    let parent = path.parent().unwrap_or(Path::new("."));
    fs::create_dir_all(parent).map_err(io_error("could not create temporary directory parent"))?;
    for attempt in 0..100u32 {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default();
        let candidate = parent.join(format!(
            ".{}.{}-{}-{}",
            path.file_name().unwrap_or_default().to_string_lossy(),
            label,
            std::process::id(),
            suffix + attempt as u128
        ));
        if !candidate.exists() {
            fs::create_dir_all(&candidate)
                .map_err(io_error("could not create staging directory"))?;
            return Ok(candidate);
        }
    }
    Err(BuildError(format!(
        "could not create temporary sibling for {}",
        path.display()
    )))
}

pub(crate) fn unique_file_path(path: &Path, label: &str) -> Result<PathBuf> {
    let parent = path.parent().unwrap_or(Path::new("."));
    fs::create_dir_all(parent).map_err(io_error("could not create temporary file parent"))?;
    for attempt in 0..100u32 {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default();
        let candidate = parent.join(format!(
            ".{}.{}-{}-{}",
            path.file_name().unwrap_or_default().to_string_lossy(),
            label,
            std::process::id(),
            suffix + attempt as u128
        ));
        if !candidate.exists() {
            return Ok(candidate);
        }
    }
    Err(BuildError(format!(
        "could not create temporary file beside {}",
        path.display()
    )))
}

pub(crate) fn install_staging(staging: &Path, output: &Path) -> Result<()> {
    let backup = if output.exists() {
        Some(unique_sibling(output, "old")?)
    } else {
        None
    };
    if let Some(backup) = &backup {
        fs::remove_dir(backup).map_err(io_error("could not prepare output backup"))?;
        fs::rename(output, backup).map_err(io_error("could not retain previous package"))?;
    }
    if let Err(error) = fs::rename(staging, output) {
        if let Some(backup) = &backup {
            let _ = fs::rename(backup, output);
        }
        return Err(io_error("could not install package output")(error));
    }
    if let Some(backup) = backup {
        let _ = fs::remove_dir_all(backup);
    }
    Ok(())
}

pub(crate) fn copy_tree_if_present(source: &Path, destination: &Path) -> Result<()> {
    if !source.is_dir() {
        return Ok(());
    }
    fs::create_dir_all(destination).map_err(io_error("could not create asset directory"))?;
    for entry in fs::read_dir(source).map_err(io_error("could not read asset directory"))? {
        let entry = entry.map_err(io_error("could not inspect asset directory"))?;
        let name = entry.file_name();
        if name == ".DS_Store" {
            continue;
        }
        let source_path = entry.path();
        let destination_path = destination.join(name);
        if entry
            .file_type()
            .map_err(io_error("could not inspect asset type"))?
            .is_dir()
        {
            copy_tree_if_present(&source_path, &destination_path)?;
        } else if entry
            .file_type()
            .map_err(io_error("could not inspect asset type"))?
            .is_file()
        {
            fs::copy(&source_path, &destination_path).map_err(io_error("could not copy asset"))?;
        }
    }
    Ok(())
}

pub(crate) fn files_under(root: &Path) -> Result<Vec<String>> {
    let mut files = Vec::new();
    collect_files(root, root, &mut files)?;
    files.sort();
    Ok(files)
}
pub(crate) fn collect_files(root: &Path, current: &Path, files: &mut Vec<String>) -> Result<()> {
    for entry in fs::read_dir(current).map_err(io_error("could not enumerate package"))? {
        let entry = entry.map_err(io_error("could not inspect package"))?;
        let path = entry.path();
        if entry
            .file_type()
            .map_err(io_error("could not inspect package entry"))?
            .is_dir()
        {
            collect_files(root, &path, files)?;
        } else if path.file_name().and_then(|name| name.to_str()) != Some(BUILD_MARKER) {
            files.push(
                path.strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/"),
            );
        }
    }
    Ok(())
}

pub(crate) fn sha256_file(path: &Path) -> Result<String> {
    let mut file = fs::File::open(path).map_err(io_error("could not open package file"))?;
    let mut digest = Sha256::new();
    let mut buffer = [0u8; 1024 * 1024];
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(io_error("could not hash package file"))?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

pub(crate) fn write_zip(source: &Path, destination: &Path) -> Result<()> {
    let file =
        fs::File::create(destination).map_err(io_error("could not create package archive"))?;
    let mut archive = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    for name in files_under(source)? {
        archive
            .start_file(&name, options)
            .map_err(|error| BuildError(format!("could not write package archive: {error}")))?;
        let bytes =
            fs::read(source.join(&name)).map_err(io_error("could not read package file"))?;
        archive
            .write_all(&bytes)
            .map_err(io_error("could not write package archive"))?;
    }
    archive
        .finish()
        .map_err(|error| BuildError(format!("could not finish package archive: {error}")))?;
    Ok(())
}
