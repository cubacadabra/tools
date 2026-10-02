//! Optional COLMAP adapter and creator-only, versioned camera evidence.
//! No runtime scene or package is inferred from a sparse reconstruction.

use crate::{CAPTURE_FORMAT_VERSION, CaptureDataset, MAX_JSON_BYTES};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File},
    io::{BufRead, BufReader, Read},
    path::{Component as PathComponent, Path, PathBuf},
    process::{Command, Stdio},
    sync::atomic::{AtomicBool, Ordering},
    thread,
    time::{Duration, Instant},
};

pub const RECONSTRUCTION_FORMAT_VERSION: u32 = 1;
const MAX_POINTS: usize = 500_000;
const MAX_TEXT_BYTES: u64 = 512_000_000;
const MAX_OBSERVATIONS: usize = 4096;

#[derive(Clone, Debug)]
pub struct ReconstructionOptions {
    pub colmap: PathBuf,
    pub threads: usize,
}

impl Default for ReconstructionOptions {
    fn default() -> Self {
        Self {
            colmap: default_colmap(),
            threads: 4,
        }
    }
}

fn default_colmap() -> PathBuf {
    if let Some(paths) = std::env::var_os("PATH") {
        if let Some(executable) = std::env::split_paths(&paths)
            .map(|p| p.join("colmap"))
            .find(|p| p.is_file())
        {
            return executable;
        }
    }
    // Finder-launched applications commonly omit package-manager executables from PATH.
    #[cfg(target_os = "macos")]
    for location in [
        "/opt/homebrew/bin/colmap",
        "/usr/local/bin/colmap",
        "/opt/local/bin/colmap",
    ] {
        let path = PathBuf::from(location);
        if path.is_file() {
            return path;
        }
    }
    PathBuf::from("colmap")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReconstructionStage {
    Preparing,
    Extracting,
    Matching,
    Mapping,
    Exporting,
    Saving,
    Complete,
}

impl ReconstructionStage {
    pub fn label(self) -> &'static str {
        match self {
            Self::Preparing => "Preparing reconstruction frames…",
            Self::Extracting => "Extracting image features…",
            Self::Matching => "Matching overlapping views…",
            Self::Mapping => "Recovering cameras and sparse geometry…",
            Self::Exporting => "Reading camera recovery results…",
            Self::Saving => "Saving reconstruction evidence…",
            Self::Complete => "Camera recovery complete",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ReconstructionProgress {
    pub stage: ReconstructionStage,
    pub elapsed: Duration,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReconstructionBackend {
    pub name: String,
    pub version: String,
    pub adapter: String,
    pub threads: usize,
    pub camera_model: String,
    pub shared_intrinsics: bool,
    pub use_gpu: bool,
    pub sequential_overlap: usize,
    pub random_seed: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FrameInput {
    pub id: String,
    pub file: String,
    pub sha256: String,
    pub width: u32,
    pub height: u32,
}

/// Pixel intrinsics for x'=x(1+k1*r²), y'=y(1+k1*r²), u=f*x'+cx, v=f*y'+cy.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CameraIntrinsics {
    pub id: u32,
    pub model: CameraModel,
    pub width: u32,
    pub height: u32,
    pub focal_length_pixels: f64,
    pub principal_point_pixels: [f64; 2],
    pub radial_distortion: f64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CameraModel {
    #[serde(rename = "SIMPLE_RADIAL")]
    SimpleRadial,
}

/// Hamilton quaternion, scalar first: camera_point = R * reconstruction_point + t.
/// Camera axes are right, down, forward. Reconstruction axes/scale are arbitrary.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CameraPose {
    pub frame_id: String,
    pub camera_id: u32,
    pub rotation_wxyz: [f64; 4],
    pub translation: [f64; 3],
}

impl CameraPose {
    pub fn rotation(&self) -> [[f64; 3]; 3] {
        let [w, x, y, z] = self.rotation_wxyz;
        [
            [
                1.0 - 2.0 * (y * y + z * z),
                2.0 * (x * y - z * w),
                2.0 * (x * z + y * w),
            ],
            [
                2.0 * (x * y + z * w),
                1.0 - 2.0 * (x * x + z * z),
                2.0 * (y * z - x * w),
            ],
            [
                2.0 * (x * z - y * w),
                2.0 * (y * z + x * w),
                1.0 - 2.0 * (x * x + y * y),
            ],
        ]
    }

    pub fn center(&self) -> [f64; 3] {
        let r = self.rotation();
        std::array::from_fn(|i| -(0..3).map(|j| r[j][i] * self.translation[j]).sum::<f64>())
    }

    pub fn camera_to_reconstruction(&self, vector: [f64; 3]) -> [f64; 3] {
        let r = self.rotation();
        std::array::from_fn(|i| (0..3).map(|j| r[j][i] * vector[j]).sum())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PointObservation {
    pub frame_id: String,
    pub point_index: u32,
    pub pixel: [f64; 2],
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SparsePoint {
    pub id: u64,
    pub position: [f64; 3],
    pub color: [u8; 3],
    pub reprojection_error_pixels: f64,
    pub observations: Vec<PointObservation>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PointShard {
    pub file: String,
    pub sha256: String,
    pub count: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReconstructionComponent {
    pub id: String,
    pub cameras: Vec<CameraIntrinsics>,
    pub frames: Vec<CameraPose>,
    pub point_shards: Vec<PointShard>,
    pub point_count: usize,
    pub mean_point_reprojection_error_pixels: f64,
    pub median_point_reprojection_error_pixels: f64,
    /// For each point, largest ray angle between its first chronological view and other views.
    /// The median is a parallax diagnostic, not a geometry confidence score.
    pub median_track_angle_degrees: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Reconstruction {
    pub format_version: u32,
    pub capture_sha256: String,
    pub source_video_sha256: String,
    pub backend: ReconstructionBackend,
    pub inputs: Vec<FrameInput>,
    pub evaluation_frame_ids: Vec<String>,
    pub unregistered_frame_ids: Vec<String>,
    pub components: Vec<ReconstructionComponent>,
    pub diagnostics: Vec<String>,
}

pub(crate) fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub(crate) fn read_json<T: serde::de::DeserializeOwned>(
    path: &Path,
    version: Option<u32>,
) -> Result<T, String> {
    let mut file = File::open(path).map_err(|e| format!("Cannot read {}: {e}", path.display()))?;
    let mut bytes = Vec::new();
    (&mut file)
        .take(MAX_JSON_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > MAX_JSON_BYTES {
        return Err("Creator JSON exceeds the 4,000,000 byte limit.".into());
    }
    let value: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|e| format!("Invalid creator JSON: {e}"))?;
    if let Some(expected) = version
        && value["formatVersion"].as_u64() != Some(expected as u64)
    {
        return Err(format!(
            "Unsupported creator format version; expected {expected}."
        ));
    }
    serde_json::from_value(value).map_err(|e| format!("Invalid creator data: {e}"))
}

pub(crate) fn write_json(path: &Path, value: &impl Serialize) -> Result<String, String> {
    let mut bytes = serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?;
    bytes.push(b'\n');
    if bytes.len() > MAX_JSON_BYTES {
        return Err("Creator JSON exceeds the 4,000,000 byte limit; split the evidence.".into());
    }
    fs::write(path, &bytes).map_err(|e| e.to_string())?;
    Ok(digest(&bytes))
}

pub(crate) fn finite(values: &[f64]) -> Result<(), String> {
    if values.iter().any(|v| !v.is_finite() || v.abs() > 1e12) {
        return Err("Non-finite or unbounded reconstruction value.".into());
    }
    Ok(())
}

fn hash_valid(hash: &str) -> bool {
    hash.len() == 64
        && hash
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn frame_id_valid(id: &str) -> bool {
    id.strip_prefix("frame-")
        .is_some_and(|digits| digits.len() == 6 && digits.bytes().all(|b| b.is_ascii_digit()))
}

fn bounded_bytes(path: &Path, limit: u64) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    File::open(path)
        .map_err(|e| e.to_string())?
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > limit {
        return Err("Evidence file exceeds the adapter byte limit.".into());
    }
    Ok(bytes)
}

pub(crate) fn safe_path(root: &Path, relative: &str) -> Result<PathBuf, String> {
    let root = if root.as_os_str().is_empty() {
        Path::new(".")
    } else {
        root
    };
    let path = Path::new(relative);
    if relative.contains('\\')
        || path
            .components()
            .any(|p| !matches!(p, PathComponent::Normal(_)))
        || relative.is_empty()
    {
        return Err("Invalid relative evidence path.".into());
    }
    let root = root.canonicalize().map_err(|e| e.to_string())?;
    let resolved = root
        .join(path)
        .canonicalize()
        .map_err(|e| format!("Missing evidence file: {e}"))?;
    if !resolved.starts_with(&root) || !resolved.is_file() {
        return Err("Evidence path leaves its dataset.".into());
    }
    Ok(resolved)
}

pub fn read_capture(manifest: &Path) -> Result<CaptureDataset, String> {
    let capture: CaptureDataset = read_json(manifest, Some(CAPTURE_FORMAT_VERSION))?;
    let root = manifest.parent().ok_or("Capture needs a parent folder.")?;
    if capture.frames.is_empty()
        || capture.frames.len() > 300
        || !hash_valid(&capture.source.sha256)
    {
        return Err("Invalid capture identity or frame count.".into());
    }
    let mut ids = BTreeSet::new();
    let mut last = -1.0;
    for f in &capture.frames {
        let digits =
            f.id.strip_prefix("frame-")
                .ok_or("Invalid frame identity.")?;
        if digits.len() != 6
            || !digits.bytes().all(|b| b.is_ascii_digit())
            || f.file != format!("frames/{}.jpg", f.id)
            || !ids.insert(&f.id)
        {
            return Err("Invalid or duplicate capture frame identity/path.".into());
        }
        finite(&[f.timestamp_seconds, f.sharpness])?;
        if f.timestamp_seconds < last
            || f.sharpness < 0.0
            || f.width == 0
            || f.height == 0
            || f.width > 2560
            || f.height > 2560
        {
            return Err("Invalid capture frame dimensions/timing.".into());
        }
        last = f.timestamp_seconds;
        safe_path(root, &f.file)?;
    }
    Ok(capture)
}

pub fn read_reconstruction(manifest: &Path) -> Result<Reconstruction, String> {
    let result: Reconstruction = read_json(manifest, Some(RECONSTRUCTION_FORMAT_VERSION))?;
    if !hash_valid(&result.capture_sha256)
        || !hash_valid(&result.source_video_sha256)
        || result.inputs.len() > 300
        || result.components.len() > 300
        || result.inputs.len() < 3
        || result.inputs.len() + result.evaluation_frame_ids.len() > 300
    {
        return Err("Invalid reconstruction identity/counts.".into());
    }
    if result.backend.name != "COLMAP"
        || result.backend.adapter != "colmap-sparse-v1"
        || result.backend.camera_model != "SIMPLE_RADIAL"
        || !result.backend.shared_intrinsics
        || result.backend.use_gpu
        || !(1..=16).contains(&result.backend.threads)
    {
        return Err("Unsupported reconstruction adapter/settings.".into());
    }
    let mut inputs = BTreeMap::new();
    for f in &result.inputs {
        if !hash_valid(&f.sha256)
            || !frame_id_valid(&f.id)
            || f.file != format!("frames/{}.jpg", f.id)
            || inputs.insert(f.id.as_str(), f).is_some()
            || f.width == 0
            || f.height == 0
            || f.width > 2560
            || f.height > 2560
        {
            return Err("Invalid reconstruction frame input.".into());
        }
        let snapshot = safe_path(
            manifest.parent().ok_or("Missing reconstruction folder.")?,
            &f.file,
        )?;
        if digest(&bounded_bytes(&snapshot, 32_000_000)?) != f.sha256 {
            return Err("Reconstruction frame snapshot hash mismatch.".into());
        }
    }
    if result
        .evaluation_frame_ids
        .iter()
        .any(|id| inputs.contains_key(id.as_str()))
    {
        return Err("Evaluation frames must not enter reconstruction.".into());
    }
    let unique_evaluation: BTreeSet<_> = result.evaluation_frame_ids.iter().collect();
    if unique_evaluation.len() != result.evaluation_frame_ids.len()
        || unique_evaluation.iter().any(|id| !frame_id_valid(id))
    {
        return Err("Invalid evaluation frame identities.".into());
    }
    let mut component_ids = BTreeSet::new();
    let mut registered = BTreeSet::new();
    for c in &result.components {
        if !component_ids.insert(&c.id)
            || c.frames.len() < 2
            || c.frames.len() > 300
            || c.cameras.len() > 300
            || c.point_count == 0
            || c.point_count > MAX_POINTS
            || c.point_shards.len() > MAX_POINTS
        {
            return Err("Invalid reconstruction component counts/identity.".into());
        }
        finite(&[
            c.mean_point_reprojection_error_pixels,
            c.median_point_reprojection_error_pixels,
            c.median_track_angle_degrees,
        ])?;
        if c.mean_point_reprojection_error_pixels < 0.0
            || c.median_point_reprojection_error_pixels < 0.0
            || !(0.0..=180.0).contains(&c.median_track_angle_degrees)
        {
            return Err("Invalid reconstruction diagnostics.".into());
        }
        let mut cameras = BTreeMap::new();
        for camera in &c.cameras {
            finite(&[
                camera.focal_length_pixels,
                camera.radial_distortion,
                camera.principal_point_pixels[0],
                camera.principal_point_pixels[1],
            ])?;
            if camera.focal_length_pixels <= 0.0 || cameras.insert(camera.id, camera).is_some() {
                return Err("Invalid camera intrinsics.".into());
            }
        }
        let mut frames = BTreeSet::new();
        for pose in &c.frames {
            finite(&pose.rotation_wxyz)?;
            finite(&pose.translation)?;
            let norm: f64 = pose.rotation_wxyz.iter().map(|v| v * v).sum();
            let input = inputs
                .get(pose.frame_id.as_str())
                .ok_or("Pose has no reconstruction input.")?;
            let camera = cameras
                .get(&pose.camera_id)
                .ok_or("Pose has no intrinsics.")?;
            if (norm - 1.0).abs() > 1e-5
                || !frames.insert(&pose.frame_id)
                || input.width != camera.width
                || input.height != camera.height
            {
                return Err("Invalid pose quaternion/dimensions/identity.".into());
            }
            registered.insert(pose.frame_id.as_str());
        }
        if c.point_shards
            .iter()
            .any(|s| s.count == 0 || s.count > 1000 || !hash_valid(&s.sha256))
        {
            return Err("Invalid sparse point shard.".into());
        }
        if c.point_shards.iter().map(|s| s.count).sum::<usize>() != c.point_count {
            return Err("Point shard counts do not match.".into());
        }
    }
    let expected: BTreeSet<_> = inputs
        .keys()
        .copied()
        .filter(|id| !registered.contains(id))
        .collect();
    let actual: BTreeSet<_> = result
        .unregistered_frame_ids
        .iter()
        .map(String::as_str)
        .collect();
    if expected != actual || actual.len() != result.unregistered_frame_ids.len() {
        return Err("Invalid unregistered frame report.".into());
    }
    Ok(result)
}

pub fn read_component_points(
    root: &Path,
    component: &ReconstructionComponent,
) -> Result<Vec<SparsePoint>, String> {
    let mut points = Vec::new();
    let mut ids = BTreeSet::new();
    let frames: BTreeSet<_> = component
        .frames
        .iter()
        .map(|f| f.frame_id.as_str())
        .collect();
    for shard in &component.point_shards {
        let path = safe_path(root, &shard.file)?;
        let shard_points: Vec<SparsePoint> = read_json(&path, None)?;
        if digest(&bounded_bytes(&path, MAX_JSON_BYTES as u64)?) != shard.sha256
            || shard_points.len() != shard.count
        {
            return Err("Sparse point shard identity/count mismatch.".into());
        }
        for p in &shard_points {
            finite(&p.position)?;
            finite(&[p.reprojection_error_pixels])?;
            let mut observed = BTreeSet::new();
            let mut observed_frames = BTreeSet::new();
            if !ids.insert(p.id)
                || p.reprojection_error_pixels < 0.0
                || p.observations.len() < 2
                || p.observations.len() > MAX_OBSERVATIONS
            {
                return Err("Invalid sparse point/track.".into());
            }
            for o in &p.observations {
                finite(&o.pixel)?;
                if !frames.contains(o.frame_id.as_str())
                    || !observed.insert((&o.frame_id, o.point_index))
                {
                    return Err("Invalid point observation frame.".into());
                }
                observed_frames.insert(&o.frame_id);
            }
            if observed_frames.len() < 2 {
                return Err("Sparse tracks need at least two distinct views.".into());
            }
        }
        if points.len() + shard_points.len() > MAX_POINTS {
            return Err("Sparse point limit exceeded.".into());
        }
        points.extend(shard_points);
    }
    if points.len() != component.point_count {
        return Err("Sparse point count mismatch.".into());
    }
    Ok(points)
}

fn check_cancel(cancel: &AtomicBool) -> Result<(), String> {
    if cancel.load(Ordering::Relaxed) {
        Err("Camera recovery cancelled.".into())
    } else {
        Ok(())
    }
}

/// Subprocess output goes to disk, never an undrained pipe or an unbounded memory buffer.
/// Children are killed and reaped before the staging directory is removed.
fn run_colmap(
    program: &Path,
    args: &[String],
    log_path: &Path,
    cancel: &AtomicBool,
    mut tick: impl FnMut(),
) -> Result<(), String> {
    check_cancel(cancel)?;
    let log = File::create(log_path).map_err(|e| e.to_string())?;
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::from(log.try_clone().map_err(|e| e.to_string())?))
        .stderr(Stdio::from(log))
        .spawn()
        .map_err(|_| {
            "Could not start COLMAP. Install COLMAP on PATH or choose its executable, then retry."
                .to_owned()
        })?;
    loop {
        if cancel.load(Ordering::Relaxed) {
            let _ = child.kill();
            let _ = child.wait();
            return Err("Camera recovery cancelled.".into());
        }
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(_)) => {
                return Err(format!(
                    "COLMAP {} failed. Check the local reconstruction log and capture overlap, then retry.",
                    args[0]
                ));
            }
            Ok(None) => {
                tick();
                thread::sleep(Duration::from_millis(250));
            }
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("Could not wait for COLMAP.".into());
            }
        }
    }
}

fn log_text(path: &Path) -> Result<String, String> {
    let mut text = String::new();
    File::open(path)
        .map_err(|e| e.to_string())?
        .take(1_000_000)
        .read_to_string(&mut text)
        .map_err(|e| e.to_string())?;
    Ok(text)
}

fn args(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| s.to_string()).collect()
}
fn path_arg(args: &mut Vec<String>, name: &str, value: &Path) -> Result<(), String> {
    args.push(name.into());
    args.push(value.to_str().ok_or("COLMAP requires UTF-8 paths.")?.into());
    Ok(())
}

/// Creates a new evidence folder. capture.json and the original frames are never changed.
pub fn recover_cameras(
    manifest: &Path,
    output: &Path,
    options: ReconstructionOptions,
    cancel: &AtomicBool,
    mut progress: impl FnMut(ReconstructionProgress),
) -> Result<Reconstruction, String> {
    if output.exists() {
        return Err("Reconstruction output already exists; choose a new folder.".into());
    }
    if !(1..=16).contains(&options.threads) {
        return Err("COLMAP threads must be between 1 and 16.".into());
    }
    let capture_hash = digest(&bounded_bytes(manifest, MAX_JSON_BYTES as u64)?);
    let capture = read_capture(manifest)?;
    if digest(&bounded_bytes(manifest, MAX_JSON_BYTES as u64)?) != capture_hash {
        return Err(
            "Capture metadata changed during loading. Retry with a finalized capture.".into(),
        );
    }
    let input_root = manifest.parent().unwrap();
    let training: Vec<_> = capture.frames.iter().filter(|f| !f.evaluation).collect();
    if training.len() < 3 {
        return Err("Camera recovery needs at least three reconstruction frames.".into());
    }
    let size = (training[0].width, training[0].height);
    if training.iter().any(|f| (f.width, f.height) != size) {
        return Err("Shared intrinsics require consistent frame dimensions.".into());
    }
    let parent = output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let parent = parent.canonicalize().map_err(|e| e.to_string())?;
    if parent.components().any(|c| {
        c.as_os_str().eq_ignore_ascii_case("assets")
            || c.as_os_str().eq_ignore_ascii_case("runtime")
    }) {
        return Err("Reconstruction evidence belongs outside runtime/assets folders.".into());
    }
    let staging = tempfile::Builder::new()
        .prefix(".room-cameras-")
        .tempdir_in(&parent)
        .map_err(|e| e.to_string())?;
    let root = staging.path();
    for dir in ["frames", "logs", "colmap", "points"] {
        fs::create_dir(root.join(dir)).map_err(|e| e.to_string())?;
    }
    let started = Instant::now();
    let mut report = |stage| {
        progress(ReconstructionProgress {
            stage,
            elapsed: started.elapsed(),
        })
    };
    report(ReconstructionStage::Preparing);
    let mut inputs = Vec::new();
    for frame in training {
        check_cancel(cancel)?;
        let source = safe_path(input_root, &frame.file)?;
        let destination = root.join(&frame.file);
        let snapshot = bounded_bytes(&source, 32_000_000)?;
        fs::write(&destination, &snapshot).map_err(|e| e.to_string())?;
        let dimensions = image::ImageReader::open(&destination)
            .map_err(|e| e.to_string())?
            .into_dimensions()
            .map_err(|_| "A reconstruction JPEG is unreadable.".to_owned())?;
        if dimensions != (frame.width, frame.height) {
            return Err("JPEG dimensions disagree with capture metadata.".into());
        }
        // Decode a copied snapshot: corrupted images and dimension mismatch fail before COLMAP.
        let image = image::open(&destination)
            .map_err(|_| "A reconstruction JPEG is unreadable.".to_owned())?;
        if (image.width(), image.height()) != (frame.width, frame.height) {
            return Err("JPEG dimensions disagree with capture metadata.".into());
        }
        inputs.push(FrameInput {
            id: frame.id.clone(),
            file: frame.file.clone(),
            sha256: digest(&snapshot),
            width: frame.width,
            height: frame.height,
        });
    }
    let mut invoke = |stage, command: Vec<String>, name: &str| {
        report(stage);
        let log = root.join("logs").join(name);
        run_colmap(&options.colmap, &command, &log, cancel, || report(stage)).map_err(|error| {
            if !cancel.load(Ordering::Relaxed) {
                let failure_log = output.with_extension("failed.log");
                if let Ok(mut destination) = fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&failure_log)
                    && let Ok(source) = File::open(&log)
                {
                    let _ = std::io::copy(&mut source.take(1_000_000), &mut destination);
                    return format!("{error} Details: {}", failure_log.display());
                }
            }
            error
        })
    };
    invoke(ReconstructionStage::Preparing, args(&["-h"]), "version.log")?;
    let help = log_text(&root.join("logs/version.log"))?;
    let version = help
        .lines()
        .find(|l| l.contains("COLMAP "))
        .ok_or("Cannot identify the installed COLMAP version.")?
        .trim()
        .to_owned();
    invoke(
        ReconstructionStage::Preparing,
        args(&["feature_extractor", "-h"]),
        "features-help.log",
    )?;
    invoke(
        ReconstructionStage::Preparing,
        args(&["sequential_matcher", "-h"]),
        "matching-help.log",
    )?;
    let feature_help = log_text(&root.join("logs/features-help.log"))?;
    let matching_help = log_text(&root.join("logs/matching-help.log"))?;
    let (feature_prefix, matching_prefix) = if feature_help.contains("--FeatureExtraction.use_gpu")
        && matching_help.contains("--FeatureMatching.use_gpu")
    {
        ("FeatureExtraction", "FeatureMatching")
    } else if feature_help.contains("--SiftExtraction.use_gpu")
        && matching_help.contains("--SiftMatching.use_gpu")
    {
        ("SiftExtraction", "SiftMatching")
    } else {
        return Err(
            "Unsupported COLMAP feature/matching CLI. Choose a compatible COLMAP build.".into(),
        );
    };
    let threads = options.threads.to_string();
    let database = root.join("colmap/database.db");
    let images = root.join("frames");
    let sparse = root.join("colmap/sparse");
    fs::create_dir(&sparse).map_err(|e| e.to_string())?;
    let mut features = args(&[
        "feature_extractor",
        "--ImageReader.camera_model",
        "SIMPLE_RADIAL",
        "--ImageReader.single_camera",
        "1",
        "--random_seed",
        "0",
    ]);
    // Some releases call this option default_random_seed. Detect instead of silently changing settings.
    if feature_help.contains("--default_random_seed") {
        features[5] = "--default_random_seed".into();
    }
    features.extend(args(&[
        &format!("--{feature_prefix}.use_gpu"),
        "0",
        &format!("--{feature_prefix}.num_threads"),
        &threads,
    ]));
    path_arg(&mut features, "--database_path", &database)?;
    path_arg(&mut features, "--image_path", &images)?;
    invoke(ReconstructionStage::Extracting, features, "features.log")?;
    let mut matches = args(&[
        "sequential_matcher",
        "--SequentialMatching.overlap",
        "20",
        "--SequentialMatching.loop_detection",
        "0",
    ]);
    matches.extend(args(&["--TwoViewGeometry.random_seed", "0"]));
    matches.extend(args(&[
        &format!("--{matching_prefix}.use_gpu"),
        "0",
        &format!("--{matching_prefix}.num_threads"),
        &threads,
    ]));
    path_arg(&mut matches, "--database_path", &database)?;
    invoke(ReconstructionStage::Matching, matches, "matching.log")?;
    let mut mapper = args(&[
        "mapper",
        "--Mapper.num_threads",
        &threads,
        "--Mapper.min_model_size",
        "3",
        "--Mapper.random_seed",
        "0",
    ]);
    path_arg(&mut mapper, "--database_path", &database)?;
    path_arg(&mut mapper, "--image_path", &images)?;
    path_arg(&mut mapper, "--output_path", &sparse)?;
    invoke(ReconstructionStage::Mapping, mapper, "mapping.log")?;
    let mut model_dirs: Vec<_> = fs::read_dir(&sparse)
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    model_dirs.sort();
    let mut components = Vec::new();
    for (index, model) in model_dirs.iter().enumerate() {
        check_cancel(cancel)?;
        let text = root.join("colmap").join(format!("model-{index:03}"));
        fs::create_dir(&text).map_err(|e| e.to_string())?;
        let mut convert = args(&["model_converter", "--output_type", "TXT"]);
        path_arg(&mut convert, "--input_path", model)?;
        path_arg(&mut convert, "--output_path", &text)?;
        invoke(
            ReconstructionStage::Exporting,
            convert,
            &format!("export-{index:03}.log"),
        )?;
        let (mut component, points) = parse_model(&text, &inputs, index)?;
        component.point_shards = write_points(root, &component.id, &points)?;
        components.push(component);
    }
    components.sort_by(|a, b| b.frames.len().cmp(&a.frames.len()).then(a.id.cmp(&b.id)));
    report(ReconstructionStage::Saving);
    let registered: BTreeSet<_> = components
        .iter()
        .flat_map(|c| c.frames.iter().map(|f| f.frame_id.as_str()))
        .collect();
    let unregistered_frame_ids: Vec<_> = inputs
        .iter()
        .filter(|f| !registered.contains(f.id.as_str()))
        .map(|f| f.id.clone())
        .collect();
    let mut diagnostics=vec![
        "Sparse points are reconstruction evidence, not a surface mesh or collision.".into(),
        "Scale, floor direction, and origin need creator measurements and review.".into(),
        "Intrinsics are estimated with one shared SIMPLE_RADIAL camera; stabilization, zoom, and lens changes can violate this assumption.".into(),
        "Evaluation frames were excluded from feature extraction, matching, and mapping; their poses are not recovered.".into(),
    ];
    if components.is_empty() {
        diagnostics.push("No usable camera reconstruction. Capture sharper overlapping views from different positions; panning alone cannot establish depth.".into());
    }
    if components.len() > 1 {
        diagnostics.push(format!("{} reconstruction components have independent coordinates and scales; some may be overlapping alternative solutions. Review and align each separately.",components.len()));
    }
    if !unregistered_frame_ids.is_empty() {
        diagnostics.push(format!(
            "{} of {} reconstruction frames did not register.",
            unregistered_frame_ids.len(),
            inputs.len()
        ));
    }
    if capture.source.video.duration_seconds < 5.0 {
        diagnostics.push(
            "This short clip may test camera recovery, but does not establish full-room coverage."
                .into(),
        );
    }
    for component in &components {
        if component.median_track_angle_degrees < 5.0 {
            diagnostics.push(format!("{}: median track angle {:.2}°. Limited parallax can make depth unreliable even with small reprojection errors; review against a wider capture.",component.id,component.median_track_angle_degrees));
        }
        if component
            .cameras
            .iter()
            .any(|c| c.radial_distortion.abs() > 0.5)
        {
            diagnostics.push(format!("{}: unusually large estimated radial distortion. Inspect calibration and geometry before using this solution.",component.id));
        }
    }
    let result = Reconstruction {
        format_version: RECONSTRUCTION_FORMAT_VERSION,
        capture_sha256: capture_hash,
        source_video_sha256: capture.source.sha256,
        backend: ReconstructionBackend {
            name: "COLMAP".into(),
            version,
            adapter: "colmap-sparse-v1".into(),
            threads: options.threads,
            camera_model: "SIMPLE_RADIAL".into(),
            shared_intrinsics: true,
            use_gpu: false,
            sequential_overlap: 20,
            random_seed: 0,
        },
        inputs,
        evaluation_frame_ids: capture
            .frames
            .iter()
            .filter(|f| f.evaluation)
            .map(|f| f.id.clone())
            .collect(),
        unregistered_frame_ids,
        components,
        diagnostics,
    };
    write_json(&root.join("reconstruction.json"), &result)?;
    read_reconstruction(&root.join("reconstruction.json"))?;
    for component in &result.components {
        read_component_points(root, component)?;
    }
    if digest(&bounded_bytes(manifest, MAX_JSON_BYTES as u64)?) != result.capture_sha256 {
        return Err(
            "Capture metadata changed during camera recovery. Retry with a finalized capture."
                .into(),
        );
    }
    check_cancel(cancel)?;
    // Reserve exclusively, including on Windows; publish the completion marker last.
    fs::create_dir(output)
        .map_err(|_| "Reconstruction output already exists or cannot be created.".to_owned())?;
    let publish = (|| {
        for name in ["frames", "logs", "colmap", "points"] {
            fs::rename(root.join(name), output.join(name)).map_err(|e| e.to_string())?;
        }
        fs::rename(
            root.join("reconstruction.json"),
            output.join("reconstruction.json"),
        )
        .map_err(|e| e.to_string())
    })();
    if publish.is_err() {
        let _ = fs::remove_dir_all(output);
    }
    publish?;
    report(ReconstructionStage::Complete);
    Ok(result)
}

fn text_lines(path: &Path) -> Result<Vec<String>, String> {
    let file = File::open(path).map_err(|e| e.to_string())?;
    if file.metadata().map_err(|e| e.to_string())?.len() > MAX_TEXT_BYTES {
        return Err("COLMAP text evidence exceeds the adapter limit.".into());
    }
    BufReader::new(file)
        .lines()
        .collect::<Result<_, _>>()
        .map_err(|e| e.to_string())
}
fn number<T: std::str::FromStr>(text: &str) -> Result<T, String> {
    text.parse()
        .map_err(|_| "Invalid COLMAP numeric field.".into())
}

fn parse_model(
    root: &Path,
    inputs: &[FrameInput],
    index: usize,
) -> Result<(ReconstructionComponent, Vec<SparsePoint>), String> {
    let mut cameras = Vec::new();
    for line in text_lines(&root.join("cameras.txt"))? {
        if line.trim().is_empty() || line.starts_with('#') {
            continue;
        }
        let v: Vec<_> = line.split_whitespace().collect();
        if v.len() != 8 || v[1] != "SIMPLE_RADIAL" {
            return Err("Unsupported COLMAP camera model/parameters.".into());
        }
        cameras.push(CameraIntrinsics {
            id: number(v[0])?,
            model: CameraModel::SimpleRadial,
            width: number(v[2])?,
            height: number(v[3])?,
            focal_length_pixels: number(v[4])?,
            principal_point_pixels: [number(v[5])?, number(v[6])?],
            radial_distortion: number(v[7])?,
        });
    }
    cameras.sort_by_key(|c| c.id);
    let known: BTreeMap<_, _> = inputs
        .iter()
        .map(|f| (format!("{}.jpg", f.id), f.id.clone()))
        .collect();
    let mut frames = Vec::new();
    type ImageKeypoints = (String, Vec<(f64, f64, i64)>);
    let mut images: BTreeMap<u64, ImageKeypoints> = BTreeMap::new();
    let lines = text_lines(&root.join("images.txt"))?;
    let mut rows = lines.iter().filter(|l| !l.starts_with('#'));
    while let Some(line) = rows.next() {
        if line.trim().is_empty() {
            continue;
        }
        let v: Vec<_> = line.split_whitespace().collect();
        if v.len() != 10 {
            return Err("Malformed COLMAP image pose.".into());
        }
        let frame_id = known
            .get(v[9])
            .ok_or("COLMAP returned an unknown/evaluation frame.")?
            .clone();
        let image_id = number(v[0])?;
        let mut q = [number(v[1])?, number(v[2])?, number(v[3])?, number(v[4])?];
        finite(&q)?;
        let norm = q.iter().map(|v| v * v).sum::<f64>().sqrt();
        if (norm - 1.0).abs() > 1e-5 {
            return Err("Invalid COLMAP quaternion.".into());
        }
        for v in &mut q {
            *v /= norm;
        }
        // Canonical sign makes q and -q serialize identically.
        if q[0] < 0.0 {
            for v in &mut q {
                *v = -*v;
            }
        }
        frames.push(CameraPose {
            frame_id: frame_id.clone(),
            camera_id: number(v[8])?,
            rotation_wxyz: q,
            translation: [number(v[5])?, number(v[6])?, number(v[7])?],
        });
        let coordinates = rows.next().ok_or("COLMAP image has no keypoint row.")?;
        let v: Vec<_> = coordinates.split_whitespace().collect();
        if v.len() % 3 != 0 || v.len() > 3_000_000 {
            return Err("Invalid COLMAP keypoint row.".into());
        }
        let pixels = v
            .as_chunks::<3>()
            .0
            .iter()
            .map(|p| Ok((number(p[0])?, number(p[1])?, number(p[2])?)))
            .collect::<Result<Vec<_>, String>>()?;
        if images.insert(image_id, (frame_id, pixels)).is_some() {
            return Err("Duplicate COLMAP image identity.".into());
        }
    }
    frames.sort_by(|a, b| a.frame_id.cmp(&b.frame_id));
    let mut points = Vec::new();
    for line in text_lines(&root.join("points3D.txt"))? {
        if line.trim().is_empty() || line.starts_with('#') {
            continue;
        }
        let v: Vec<_> = line.split_whitespace().collect();
        if v.len() < 12 || (v.len() - 8) % 2 != 0 || v.len() > 8 + 2 * MAX_OBSERVATIONS {
            return Err("Invalid COLMAP point track.".into());
        }
        let id: u64 = number(v[0])?;
        let mut observations = Vec::new();
        for track in v[8..].as_chunks::<2>().0 {
            let (frame_id, pixels) = images
                .get(&number(track[0])?)
                .ok_or("Point track references a missing image.")?;
            let (x, y, point_id) = *pixels
                .get(number::<usize>(track[1])?)
                .ok_or("Point track references a missing keypoint.")?;
            if point_id < 0 || point_id as u64 != id {
                return Err("COLMAP track/keypoint mismatch.".into());
            }
            observations.push(PointObservation {
                frame_id: frame_id.clone(),
                point_index: number(track[1])?,
                pixel: [x, y],
            });
        }
        observations.sort_by(|a, b| {
            a.frame_id
                .cmp(&b.frame_id)
                .then(a.point_index.cmp(&b.point_index))
        });
        points.push(SparsePoint {
            id,
            position: [number(v[1])?, number(v[2])?, number(v[3])?],
            color: [number(v[4])?, number(v[5])?, number(v[6])?],
            reprojection_error_pixels: number(v[7])?,
            observations,
        });
        if points.len() > MAX_POINTS {
            return Err("Sparse point limit exceeded.".into());
        }
    }
    if frames.len() < 2 || points.is_empty() {
        return Err("COLMAP component contains no usable sparse reconstruction.".into());
    }
    points.sort_by_key(|p| p.id);
    let mut errors: Vec<_> = points.iter().map(|p| p.reprojection_error_pixels).collect();
    finite(&errors)?;
    errors.sort_by(f64::total_cmp);
    let centers: BTreeMap<_, _> = frames
        .iter()
        .map(|p| (p.frame_id.as_str(), p.center()))
        .collect();
    let mut angles = Vec::with_capacity(points.len());
    for point in &points {
        let rays: Vec<[f64; 3]> = point
            .observations
            .iter()
            .map(|o| {
                let delta =
                    crate::alignment::subtract(centers[o.frame_id.as_str()], point.position);
                let length = delta.iter().map(|v| v * v).sum::<f64>().sqrt();
                if length < 1e-12 {
                    return Err("Sparse point coincides with a camera center.".to_owned());
                }
                Ok(delta.map(|v| v / length))
            })
            .collect::<Result<_, String>>()?;
        let angle = rays
            .iter()
            .map(|ray| {
                (0..3)
                    .map(|i| rays[0][i] * ray[i])
                    .sum::<f64>()
                    .clamp(-1.0, 1.0)
                    .acos()
                    .to_degrees()
            })
            .fold(0.0, f64::max);
        angles.push(angle);
    }
    angles.sort_by(f64::total_cmp);
    let component = ReconstructionComponent {
        id: format!("component-{index:03}"),
        cameras,
        frames,
        point_shards: Vec::new(),
        point_count: points.len(),
        mean_point_reprojection_error_pixels: errors.iter().sum::<f64>() / errors.len() as f64,
        median_point_reprojection_error_pixels: errors[errors.len() / 2],
        median_track_angle_degrees: angles[angles.len() / 2],
    };
    Ok((component, points))
}

fn write_points(
    root: &Path,
    component: &str,
    points: &[SparsePoint],
) -> Result<Vec<PointShard>, String> {
    let mut shards = Vec::new();
    let mut first = 0;
    while first < points.len() {
        let mut count = (points.len() - first).min(1000);
        loop {
            if serde_json::to_vec_pretty(&points[first..first + count])
                .map_err(|e| e.to_string())?
                .len()
                < MAX_JSON_BYTES
            {
                break;
            }
            count /= 2;
            if count == 0 {
                return Err("One sparse point exceeds the JSON evidence limit.".into());
            }
        }
        let file = format!("points/{component}-{:05}.json", shards.len());
        let sha256 = write_json(&root.join(&file), &points[first..first + count].to_vec())?;
        shards.push(PointShard {
            file,
            sha256,
            count,
        });
        first += count;
    }
    Ok(shards)
}

#[cfg(test)]
#[path = "reconstruction_tests.rs"]
mod tests;
