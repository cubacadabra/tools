//! Local, creator-only video intake. Reconstruction and runtime packaging are separate stages.

use image::{GrayImage, imageops::FilterType};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::{BufRead, BufReader, Read},
    path::Path,
    process::{Command, Stdio},
    sync::atomic::{AtomicBool, Ordering},
    thread,
    time::Duration,
};

pub const CAPTURE_FORMAT_VERSION: u32 = 1;
const MAX_JSON_BYTES: usize = 4_000_000;

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CaptureOptions {
    pub max_frames: usize,
    pub max_dimension: u32,
}

impl Default for CaptureOptions {
    fn default() -> Self {
        Self {
            max_frames: 180,
            max_dimension: 1600,
        }
    }
}

impl CaptureOptions {
    pub fn validate(self) -> Result<(), String> {
        if !(10..=300).contains(&self.max_frames) {
            return Err("Frame budget must be between 10 and 300.".into());
        }
        if !(320..=2560).contains(&self.max_dimension) {
            return Err("Maximum image dimension must be between 320 and 2560 pixels.".into());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VideoMetadata {
    pub width: u32,
    pub height: u32,
    pub duration_seconds: f64,
    pub codec: String,
    pub rotation_degrees: i32,
    pub frame_rate: String,
    pub pixel_format: String,
    pub color_transfer: String,
    pub color_primaries: String,
    pub color_range: String,
    pub sample_aspect_ratio: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CaptureSource {
    pub filename: String,
    pub sha256: String,
    pub bytes: u64,
    pub video: VideoMetadata,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CapturedFrame {
    pub id: String,
    pub file: String,
    /// Presentation time relative to the first decoded video frame, before selection.
    pub timestamp_seconds: f64,
    pub sharpness: f64,
    pub evaluation: bool,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CaptureDataset {
    pub format_version: u32,
    pub source: CaptureSource,
    pub settings: CaptureOptions,
    pub decoder: String,
    pub selector: String,
    /// RGB alone does not establish metric scale. Future alignment is a separate reviewed stage.
    pub scale_meters_per_unit: Option<f64>,
    pub candidate_count: usize,
    pub frames: Vec<CapturedFrame>,
    pub diagnostics: Vec<String>,
}

fn cancelled(flag: &AtomicBool) -> Result<(), String> {
    if flag.load(Ordering::Relaxed) {
        Err("Capture cancelled.".into())
    } else {
        Ok(())
    }
}

fn decoder_output(program: &str, args: &[&str]) -> Result<String, String> {
    let result = Command::new(program).args(args).output()
        .map_err(|error| format!("Could not run {program}: {error}. Install FFmpeg (ffmpeg and ffprobe) and restart Studio."))?;
    if !result.status.success() {
        return Err(format!(
            "{program} could not inspect the video. Check that the file is readable and supported."
        ));
    }
    String::from_utf8(result.stdout).map_err(|_| format!("{program} returned invalid text."))
}

fn parse_video_metadata(text: &str) -> Result<VideoMetadata, String> {
    let value: Value =
        serde_json::from_str(text).map_err(|error| format!("Invalid video metadata: {error}"))?;
    let stream = value["streams"]
        .as_array()
        .and_then(|streams| streams.first())
        .ok_or("The file has no usable video stream.")?;
    let number = |value: &Value| value.as_f64().or_else(|| value.as_str()?.parse().ok());
    let duration = number(&stream["duration"])
        .or_else(|| number(&value["format"]["duration"]))
        .ok_or("The video has no duration; use a finalized local video file.")?;
    if !duration.is_finite() || !(0.1..=7200.0).contains(&duration) {
        return Err("Capture videos must be between 0.1 seconds and two hours long.".into());
    }
    let width = stream["width"].as_u64().unwrap_or(0);
    let height = stream["height"].as_u64().unwrap_or(0);
    if width == 0 || height == 0 || width > 16384 || height > 16384 {
        return Err("Unsupported video dimensions (maximum 16384 pixels per side).".into());
    }
    let rotation = stream["side_data_list"]
        .as_array()
        .and_then(|items| items.iter().find_map(|item| item["rotation"].as_i64()))
        .or_else(|| {
            stream["tags"]["rotate"]
                .as_str()
                .and_then(|value| value.parse().ok())
        })
        .unwrap_or(0);
    Ok(VideoMetadata {
        width: width as u32,
        height: height as u32,
        duration_seconds: duration,
        codec: stream["codec_name"].as_str().unwrap_or("unknown").into(),
        rotation_degrees: rotation as i32,
        frame_rate: stream["avg_frame_rate"]
            .as_str()
            .unwrap_or("unknown")
            .into(),
        pixel_format: stream["pix_fmt"].as_str().unwrap_or("unknown").into(),
        color_transfer: stream["color_transfer"]
            .as_str()
            .unwrap_or("unknown")
            .into(),
        color_primaries: stream["color_primaries"]
            .as_str()
            .unwrap_or("unknown")
            .into(),
        color_range: stream["color_range"].as_str().unwrap_or("unknown").into(),
        sample_aspect_ratio: stream["sample_aspect_ratio"]
            .as_str()
            .unwrap_or("unknown")
            .into(),
    })
}

fn timestamp_from_log(line: &str) -> Option<f64> {
    if !line.contains("Parsed_showinfo") || !line.contains(" n:") {
        return None;
    }
    let value = line
        .split("pts_time:")
        .nth(1)?
        .split_whitespace()
        .next()?
        .parse::<f64>()
        .ok()?;
    (value.is_finite() && value >= 0.0).then_some(value)
}

/// Variance of a discrete Laplacian, measured at a consistent 320px review size.
/// This is a ranking heuristic, not a calibrated blur/coverage confidence.
fn sharpness(image: &GrayImage) -> f64 {
    if image.width() < 3 || image.height() < 3 {
        return 0.0;
    }
    let mut sum = 0.0;
    let mut squared = 0.0;
    let mut count = 0.0;
    for y in 1..image.height() - 1 {
        for x in 1..image.width() - 1 {
            let p = |x, y| f64::from(image.get_pixel(x, y).0[0]);
            let laplacian = p(x - 1, y) + p(x + 1, y) + p(x, y - 1) + p(x, y + 1) - 4.0 * p(x, y);
            sum += laplacian;
            squared += laplacian * laplacian;
            count += 1.0;
        }
    }
    (squared / count - (sum / count).powi(2)).max(0.0)
}

struct Candidate {
    index: usize,
    timestamp: f64,
    sharpness: f64,
    width: u32,
    height: u32,
}

fn select_candidates(candidates: Vec<Candidate>, duration: f64, budget: usize) -> Vec<Candidate> {
    let mut windows = BTreeMap::<usize, Candidate>::new();
    for candidate in candidates {
        let window =
            ((candidate.timestamp / duration * budget as f64).floor() as usize).min(budget - 1);
        if windows
            .get(&window)
            .is_none_or(|previous| candidate.sharpness > previous.sharpness)
        {
            windows.insert(window, candidate);
        }
    }
    windows.into_values().collect()
}

/// Creates a new capture folder with capture.json as its completion marker.
/// Never replaces an existing dataset; recoverable failures clean up their own staging.
/// Only the selected JPEGs and metadata are retained; the original is identified by hash.
pub fn capture_video(
    source: &Path,
    output: &Path,
    options: CaptureOptions,
    cancel: &AtomicBool,
    progress: impl Fn(&str),
) -> Result<CaptureDataset, String> {
    options.validate()?;
    cancelled(cancel)?;
    if output.try_exists().map_err(|error| error.to_string())? {
        return Err(
            "The capture output already exists. Choose a new folder to preserve earlier work."
                .into(),
        );
    }
    let source = source
        .canonicalize()
        .map_err(|error| format!("Could not open video: {error}"))?;
    if !source.is_file() {
        return Err("Choose a local video file.".into());
    }
    let original_metadata = fs::metadata(&source).map_err(|error| error.to_string())?;
    progress("Inspecting video…");
    let source_text = source
        .to_str()
        .ok_or("The video path must be valid Unicode.")?;
    let video = parse_video_metadata(&decoder_output(
        "ffprobe",
        &[
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=width,height,codec_name,duration,avg_frame_rate,pix_fmt,color_transfer,color_primaries,color_range,sample_aspect_ratio:stream_tags=rotate:stream_side_data=rotation:format=duration",
            "-of",
            "json",
            source_text,
        ],
    )?)?;
    let decoder = decoder_output("ffmpeg", &["-version"])?
        .lines()
        .next()
        .unwrap_or("ffmpeg")
        .to_owned();
    progress("Hashing source video…");
    let mut input = File::open(&source).map_err(|error| error.to_string())?;
    let mut hash = Sha256::new();
    let mut buffer = vec![0; 1024 * 1024];
    loop {
        cancelled(cancel)?;
        let count = input.read(&mut buffer).map_err(|error| error.to_string())?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    let parent = output
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    if !parent.is_dir() {
        return Err("Choose an existing parent folder for the capture.".into());
    }
    let staging = tempfile::Builder::new()
        .prefix(".room-capture-")
        .tempdir_in(parent)
        .map_err(|error| format!("Could not create capture folder: {error}"))?;
    let candidate_dir = staging.path().join("candidates");
    fs::create_dir(&candidate_dir).map_err(|error| error.to_string())?;
    let log_path = staging.path().join("decode.log");
    let log = File::create(&log_path).map_err(|error| error.to_string())?;
    let candidate_limit = options.max_frames * 3;
    let interval = (video.duration_seconds / candidate_limit as f64).max(1.0 / 30.0);
    let side = options.max_dimension;
    // showinfo runs before encoding: timestamps refer to original decoded presentation frames,
    // normalized to the first frame. Autorotation is FFmpeg's default; no fabricated cameras.
    let filter = format!(
        "setpts=PTS-STARTPTS,select='isnan(prev_selected_t)+gte(t-prev_selected_t,{interval:.9})',scale=w='min(iw,{side})':h='min(ih,{side})':force_original_aspect_ratio=decrease:force_divisible_by=2,setsar=1,showinfo"
    );
    progress("Decoding candidate frames…");
    let mut child = Command::new("ffmpeg")
        .args([
            "-hide_banner",
            "-nostdin",
            "-nostats",
            "-loglevel",
            "info",
            "-i",
        ])
        .arg(&source)
        .args([
            "-map",
            "0:v:0",
            "-an",
            "-sn",
            "-dn",
            "-vf",
            &filter,
            "-fps_mode",
            "passthrough",
            "-frames:v",
        ])
        .arg(candidate_limit.to_string())
        .args(["-q:v", "2", "-threads", "2"])
        .arg(candidate_dir.join("%06d.jpg"))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::from(log))
        .spawn()
        .map_err(|error| format!("Could not decode video: {error}"))?;
    let status = loop {
        if cancel.load(Ordering::Relaxed) {
            let _ = child.kill();
            let _ = child.wait();
            return Err("Capture cancelled.".into());
        }
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => thread::sleep(Duration::from_millis(100)),
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("Could not wait for video decoder: {error}"));
            }
        }
    };
    if !status.success() {
        return Err("Video decoding failed. Check the file and FFmpeg codec support.".into());
    }
    let times = BufReader::new(File::open(&log_path).map_err(|error| error.to_string())?)
        .lines()
        .map_while(Result::ok)
        .filter_map(|line| timestamp_from_log(&line))
        .collect::<Vec<_>>();
    progress("Selecting sharp frames across the video…");
    let mut candidates = Vec::new();
    for (index, timestamp) in times.into_iter().enumerate().take(candidate_limit) {
        cancelled(cancel)?;
        let path = candidate_dir.join(format!("{:06}.jpg", index + 1));
        if !path.is_file() {
            break;
        } // showinfo can log one buffered frame beyond the encoder limit.
        let image =
            image::open(&path).map_err(|error| format!("Could not read decoded frame: {error}"))?;
        let score = sharpness(&image.resize(320, 320, FilterType::Triangle).to_luma8());
        candidates.push(Candidate {
            index: index + 1,
            timestamp,
            sharpness: score,
            width: image.width(),
            height: image.height(),
        });
    }
    if candidates.is_empty() {
        return Err("No frames could be extracted from this video.".into());
    }
    let candidate_count = candidates.len();
    let selected = select_candidates(candidates, video.duration_seconds, options.max_frames);
    let frame_dir = staging.path().join("frames");
    fs::create_dir(&frame_dir).map_err(|error| error.to_string())?;
    let mut frames = Vec::new();
    for (index, candidate) in selected.into_iter().enumerate() {
        cancelled(cancel)?;
        let id = format!("frame-{:06}", candidate.index);
        let file = format!("frames/{id}.jpg");
        fs::rename(
            candidate_dir.join(format!("{:06}.jpg", candidate.index)),
            staging.path().join(&file),
        )
        .map_err(|error| error.to_string())?;
        frames.push(CapturedFrame {
            id,
            file,
            timestamp_seconds: candidate.timestamp,
            sharpness: candidate.sharpness,
            evaluation: index % 10 == 9,
            width: candidate.width,
            height: candidate.height,
        });
    }
    let mut diagnostics = vec![
        "Camera poses, intrinsics, overlap, and coverage have not been solved. Review the frames before reconstruction.".into(),
        "Metric scale is unknown. Measure a visible distance before aligning a reconstructed room.".into(),
        "Sharpness is a relative ranking heuristic; it does not certify usable geometry or absence of blur.".into(),
        "Every tenth selected frame is reserved for evaluation. Exclude these frames from reconstruction/training.".into(),
    ];
    let mut scores = frames
        .iter()
        .map(|frame| frame.sharpness)
        .collect::<Vec<_>>();
    scores.sort_by(f64::total_cmp);
    let median = scores[scores.len() / 2];
    if median < 1.0 {
        diagnostics.push("The selected frames have very low image detail. Inspect for blank surfaces, blur, or darkness before reconstruction.".into());
    }
    if matches!(video.color_transfer.as_str(), "smpte2084" | "arib-std-b67") {
        diagnostics.push("This source uses HDR transfer. The current JPEG decoder does not perform calibrated HDR tone mapping; review color and exposure before using these frames.".into());
    }
    let weak = frames
        .iter()
        .filter(|frame| frame.sharpness < median * 0.25)
        .count();
    if weak > 0 {
        diagnostics.push(format!("{weak} selected frames have less than one quarter of the median sharpness; inspect them for blur or low texture."));
    }
    if frames.len() < 20 {
        diagnostics.push("Fewer than 20 frames were selected; camera reconstruction may need a longer capture or additional stills.".into());
    }
    let current_metadata = fs::metadata(&source).map_err(|error| error.to_string())?;
    if current_metadata.len() != original_metadata.len()
        || current_metadata.modified().ok() != original_metadata.modified().ok()
    {
        return Err("The video changed during import. Retry with a finalized video file.".into());
    }
    let dataset = CaptureDataset {
        format_version: CAPTURE_FORMAT_VERSION,
        source: CaptureSource {
            filename: source
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned(),
            sha256: format!("{:x}", hash.finalize()),
            bytes: original_metadata.len(),
            video,
        },
        settings: options,
        decoder,
        selector: "temporal-laplacian-v1".into(),
        scale_meters_per_unit: None,
        candidate_count,
        frames,
        diagnostics,
    };
    progress("Saving capture dataset…");
    let mut json = serde_json::to_vec_pretty(&dataset).map_err(|error| error.to_string())?;
    json.push(b'\n');
    if json.len() > MAX_JSON_BYTES {
        return Err("Capture metadata exceeds the 4 MB source limit.".into());
    }
    fs::write(staging.path().join("capture.json"), json).map_err(|error| error.to_string())?;
    fs::remove_dir_all(&candidate_dir).map_err(|error| error.to_string())?;
    fs::remove_file(&log_path).map_err(|error| error.to_string())?;
    cancelled(cancel)?;
    // Reserve without replacement on every desktop OS. capture.json is the completion marker;
    // readers must ignore directories without it. No directory-overwrite rename (Windows).
    fs::create_dir(output).map_err(|error| format!("Could not reserve capture output: {error}"))?;
    if let Err(error) = fs::rename(staging.path().join("frames"), output.join("frames")) {
        let _ = fs::remove_dir(output);
        return Err(format!("Could not finalize capture: {error}"));
    }
    if let Err(error) = fs::rename(
        staging.path().join("capture.json"),
        output.join("capture.json"),
    ) {
        let _ = fs::remove_dir_all(output.join("frames"));
        let _ = fs::remove_dir(output);
        return Err(format!("Could not finalize capture metadata: {error}"));
    }
    Ok(dataset)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranking_preserves_time_coverage_and_prefers_sharp_frames() {
        let candidate = |index, timestamp, sharpness| Candidate {
            index,
            timestamp,
            sharpness,
            width: 100,
            height: 100,
        };
        let selected = select_candidates(
            vec![
                candidate(1, 0.0, 10.0),
                candidate(2, 0.1, 20.0),
                candidate(3, 1.0, 5.0),
                candidate(4, 1.1, 5.0),
            ],
            2.0,
            2,
        );
        assert_eq!(
            selected.iter().map(|frame| frame.index).collect::<Vec<_>>(),
            [2, 3]
        );
    }

    #[test]
    fn sharpness_distinguishes_flat_and_textured_images() {
        let flat = GrayImage::from_pixel(32, 32, image::Luma([100]));
        let textured = GrayImage::from_fn(32, 32, |x, y| {
            image::Luma([if (x + y) % 2 == 0 { 255 } else { 0 }])
        });
        assert_eq!(sharpness(&flat), 0.0);
        assert!(sharpness(&textured) > 1000.0);
    }

    #[test]
    fn timestamps_are_real_decoded_pts_and_ignore_other_log_lines() {
        assert_eq!(
            timestamp_from_log(
                "[Parsed_showinfo_4 @ 0x00] n:   0 pts: 320 pts_time:0.533333 duration:20"
            ),
            Some(0.533333)
        );
        assert_eq!(
            timestamp_from_log("[Parsed_showinfo_4 @ 0x00] config in time_base: 1/600"),
            None
        );
        assert_eq!(
            timestamp_from_log("[Parsed_showinfo_4 @ 0x00] n: 0 pts_time:NaN"),
            None
        );
    }

    #[test]
    fn malformed_metadata_is_rejected_and_rotation_retained() {
        assert!(parse_video_metadata(r#"{"streams":[],"format":{"duration":"2"}}"#).is_err());
        assert!(
            parse_video_metadata(r#"{"streams":[{"width":10,"height":10,"duration":"NaN"}]}"#)
                .is_err()
        );
        let metadata = parse_video_metadata(r#"{"streams":[{"width":1080,"height":1920,"duration":"2","codec_name":"hevc","side_data_list":[{"rotation":90}]}]}"#).unwrap();
        assert_eq!(metadata.rotation_degrees, 90);
    }

    #[test]
    fn existing_outputs_and_cancellation_do_not_touch_creator_work() {
        let root = tempfile::tempdir().unwrap();
        let marker = root.path().join("keep.txt");
        fs::write(&marker, "keep").unwrap();
        let result = capture_video(
            Path::new("missing.mov"),
            root.path(),
            CaptureOptions::default(),
            &AtomicBool::new(false),
            |_| {},
        );
        assert!(result.unwrap_err().contains("already exists"));
        let output = root.path().join("cancelled");
        let result = capture_video(
            Path::new("missing.mov"),
            &output,
            CaptureOptions::default(),
            &AtomicBool::new(true),
            |_| {},
        );
        assert!(result.unwrap_err().contains("cancelled"));
        assert!(!output.exists());
        assert_eq!(fs::read_to_string(marker).unwrap(), "keep");
    }

    #[test]
    fn real_decoder_emits_bounded_frames_times_and_provenance() {
        if Command::new("ffmpeg").arg("-version").output().is_err()
            || Command::new("ffprobe").arg("-version").output().is_err()
        {
            eprintln!("Skipping decoder integration: FFmpeg is not installed.");
            return;
        }
        let root = tempfile::tempdir().unwrap();
        let video = root.path().join("source.mp4");
        let status = Command::new("ffmpeg")
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-nostdin",
                "-f",
                "lavfi",
                "-i",
                "testsrc2=size=192x108:rate=30",
                "-t",
                "2",
                "-c:v",
                "mpeg4",
            ])
            .arg(&video)
            .status()
            .unwrap();
        assert!(status.success());
        let output = root.path().join("capture");
        let options = CaptureOptions {
            max_frames: 10,
            max_dimension: 320,
        };
        let dataset =
            capture_video(&video, &output, options, &AtomicBool::new(false), |_| {}).unwrap();
        assert_eq!(dataset.frames.len(), 10);
        assert!(dataset.candidate_count <= 30);
        assert_eq!(
            dataset.source.sha256,
            format!("{:x}", Sha256::digest(fs::read(&video).unwrap()))
        );
        assert_eq!(
            dataset
                .frames
                .iter()
                .filter(|frame| frame.evaluation)
                .count(),
            1
        );
        assert!(
            dataset
                .frames
                .windows(2)
                .all(|frames| frames[0].timestamp_seconds < frames[1].timestamp_seconds)
        );
        assert!(
            dataset
                .frames
                .iter()
                .all(|frame| frame.timestamp_seconds < 2.0
                    && frame.width == 192
                    && frame.height == 108)
        );
        for frame in &dataset.frames {
            let image = image::open(output.join(&frame.file)).unwrap();
            assert_eq!((image.width(), image.height()), (frame.width, frame.height));
        }
        let manifest = fs::read_to_string(output.join("capture.json")).unwrap();
        assert!(manifest.len() < MAX_JSON_BYTES);
        assert!(!manifest.contains(&root.path().to_string_lossy().to_string()));
        assert!(serde_json::from_str::<CaptureDataset>(&manifest).is_ok());
        assert_eq!(fs::read_dir(&output).unwrap().count(), 2);
        let cancel = AtomicBool::new(false);
        let cancelled_output = root.path().join("cancelled");
        let result = capture_video(&video, &cancelled_output, options, &cancel, |message| {
            if message.starts_with("Decoding") {
                cancel.store(true, Ordering::Relaxed);
            }
        });
        assert!(result.unwrap_err().contains("cancelled"));
        assert!(!cancelled_output.exists());
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 2);
    }
}
