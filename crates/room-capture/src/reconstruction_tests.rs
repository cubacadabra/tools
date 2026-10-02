use super::*;

fn inputs() -> Vec<FrameInput> {
    (1..=3)
        .map(|i| FrameInput {
            id: format!("frame-{i:06}"),
            file: format!("frames/frame-{i:06}.jpg"),
            sha256: digest(b"snapshot"),
            width: 1600,
            height: 900,
        })
        .collect()
}

fn fixture(root: &Path) {
    fs::write(
        root.join("cameras.txt"),
        "# cameras\n1 SIMPLE_RADIAL 1600 900 1200 800 450 0\n",
    )
    .unwrap();
    fs::write(root.join("images.txt"),"# images\n1 1 0 0 0 0 0 0 1 frame-000001.jpg\n100 100 7 200 100 8 100 200 9\n2 1 0 0 0 -1 0 0 1 frame-000002.jpg\n90 100 7 190 100 8 90 200 9\n3 1 0 0 0 -2 0 0 1 frame-000003.jpg\n80 100 7 180 100 8 80 200 9\n").unwrap();
    fs::write(root.join("points3D.txt"),"# points\n7 0 0 3 100 110 120 0.2 1 0 2 0 3 0\n8 2 0 3 130 140 150 0.4 1 1 2 1 3 1\n9 0 0 1 160 170 180 0.6 1 2 2 2 3 2\n").unwrap();
}

#[test]
fn parses_camera_convention_tracks_and_errors() {
    let root = tempfile::tempdir().unwrap();
    fixture(root.path());
    let (c, p) = parse_model(root.path(), &inputs(), 0).unwrap();
    assert_eq!(c.frames[1].center(), [1.0, 0.0, 0.0]);
    assert_eq!(
        c.frames[1].camera_to_reconstruction([0.0, 0.0, 1.0]),
        [0.0, 0.0, 1.0]
    );
    assert_eq!(p[0].observations[1].pixel, [90.0, 100.0]);
    assert!((c.mean_point_reprojection_error_pixels - 0.4).abs() < 1e-9);
    assert_eq!(c.median_point_reprojection_error_pixels, 0.4);
}

#[test]
fn rejects_unknown_frames_camera_models_and_broken_tracks() {
    let root = tempfile::tempdir().unwrap();
    fixture(root.path());
    let mut input = inputs();
    input.pop();
    assert!(
        parse_model(root.path(), &input, 0)
            .unwrap_err()
            .contains("unknown/evaluation")
    );
    fs::write(
        root.path().join("cameras.txt"),
        "1 UNKNOWN 1600 900 1200 800 450 0\n",
    )
    .unwrap();
    assert!(parse_model(root.path(), &inputs(), 0).is_err());
    fixture(root.path());
    fs::write(
        root.path().join("points3D.txt"),
        "7 0 0 3 100 110 120 0.2 1 1 2 1\n",
    )
    .unwrap();
    assert!(
        parse_model(root.path(), &inputs(), 0)
            .unwrap_err()
            .contains("mismatch")
    );
}

#[test]
fn point_shards_are_bounded_hashed_and_validated() {
    let root = tempfile::tempdir().unwrap();
    fixture(root.path());
    fs::create_dir(root.path().join("points")).unwrap();
    let (mut c, p) = parse_model(root.path(), &inputs(), 0).unwrap();
    c.point_shards = write_points(root.path(), &c.id, &p).unwrap();
    assert_eq!(read_component_points(root.path(), &c).unwrap().len(), 3);
    fs::write(root.path().join(&c.point_shards[0].file), "[]").unwrap();
    assert!(
        read_component_points(root.path(), &c)
            .unwrap_err()
            .contains("mismatch")
    );
}

#[test]
fn repeated_frame_observations_preserve_feature_identity_but_need_two_views() {
    let root = tempfile::tempdir().unwrap();
    fixture(root.path());
    fs::create_dir(root.path().join("points")).unwrap();
    let (mut c, mut p) = parse_model(root.path(), &inputs(), 0).unwrap();
    let mut extra = p[0].observations[0].clone();
    extra.point_index = 42;
    p[0].observations.push(extra);
    c.point_shards = write_points(root.path(), &c.id, &p).unwrap();
    assert_eq!(
        read_component_points(root.path(), &c).unwrap()[0]
            .observations
            .len(),
        4
    );
    p[0].observations.retain(|o| o.frame_id == "frame-000001");
    c.point_shards = write_points(root.path(), &c.id, &p).unwrap();
    assert!(
        read_component_points(root.path(), &c)
            .unwrap_err()
            .contains("distinct views")
    );
}

#[test]
fn version_checked_before_shape_and_paths_confined() {
    let root = tempfile::tempdir().unwrap();
    let file = root.path().join("reconstruction.json");
    fs::write(&file, "{\"formatVersion\":999}").unwrap();
    assert!(read_reconstruction(&file).unwrap_err().contains("version"));
    for path in [
        "../outside",
        "/tmp/outside",
        "frames\\outside",
        "frames/../outside",
    ] {
        assert!(safe_path(root.path(), path).is_err());
    }
    #[cfg(unix)]
    {
        let outside = tempfile::NamedTempFile::new().unwrap();
        std::os::unix::fs::symlink(outside.path(), root.path().join("escape")).unwrap();
        assert!(safe_path(root.path(), "escape").is_err());
    }
}

#[test]
fn alignment_is_metric_right_handed_and_persists_without_changing_reconstruction() {
    let root = tempfile::tempdir().unwrap();
    fixture(root.path());
    fs::create_dir(root.path().join("points")).unwrap();
    fs::create_dir(root.path().join("frames")).unwrap();
    let (mut c, p) = parse_model(root.path(), &inputs(), 0).unwrap();
    c.point_shards = write_points(root.path(), &c.id, &p).unwrap();
    for f in inputs() {
        fs::write(root.path().join(f.file), "snapshot").unwrap();
    }
    let r = Reconstruction {
        format_version: 1,
        capture_sha256: "0".repeat(64),
        source_video_sha256: "0".repeat(64),
        backend: ReconstructionBackend {
            name: "COLMAP".into(),
            version: "test".into(),
            adapter: "colmap-sparse-v1".into(),
            threads: 4,
            camera_model: "SIMPLE_RADIAL".into(),
            shared_intrinsics: true,
            use_gpu: false,
            sequential_overlap: 20,
            random_seed: 0,
        },
        inputs: inputs(),
        evaluation_frame_ids: vec!["frame-000010".into()],
        unregistered_frame_ids: Vec::new(),
        components: vec![c],
        diagnostics: Vec::new(),
    };
    let manifest = root.path().join("reconstruction.json");
    write_json(&manifest, &r).unwrap();
    let before = fs::read(&manifest).unwrap();
    let a =
        crate::alignment::create_alignment(&manifest, "component-000", &p, [7, 8], 1.0, [7, 8, 9])
            .unwrap();
    assert_eq!(a.meters_per_unit, 0.5);
    assert_eq!(a.transform_point(p[0].position), [0.0; 3]);
    assert_eq!(a.transform_point(p[1].position), [1.0, 0.0, 0.0]);
    assert_eq!(a.transform_point(p[2].position), [0.0, 0.0, -1.0]);
    crate::alignment::save_alignment(&manifest, &a).unwrap();
    let mut invalid = a.clone();
    invalid.translation_meters[0] += 1.0;
    assert!(
        crate::alignment::save_alignment(&manifest, &invalid)
            .unwrap_err()
            .contains("reviewed anchors")
    );
    assert_eq!(
        crate::alignment::read_alignment(&manifest, 0)
            .unwrap()
            .unwrap()
            .floor_point_ids,
        [7, 8, 9]
    );
    assert_eq!(fs::read(&manifest).unwrap(), before);
    use crate::measurements::{MeasuredObject, read_measurements, record_measurement};
    let desk = MeasuredObject {
        id: "desk".into(),
        label: "Desk".into(),
        length_meters: 1.8288,
        depth_meters: 0.9144,
        height_meters: 0.9144,
    };
    record_measurement(&manifest, desk.clone()).unwrap();
    record_measurement(
        &manifest,
        MeasuredObject {
            id: "lamp".into(),
            label: "Lamp".into(),
            length_meters: 0.3,
            depth_meters: 0.3,
            height_meters: 0.6,
        },
    )
    .unwrap();
    let mut changed = desk.clone();
    changed.length_meters = 2.0;
    let measurements = record_measurement(&manifest, changed).unwrap();
    assert_eq!(measurements.objects.len(), 2);
    assert_eq!(
        read_measurements(&manifest).unwrap().unwrap().objects[0].length_meters,
        2.0
    );
    let mut invalid = desk;
    invalid.height_meters = f64::NAN;
    assert!(record_measurement(&manifest, invalid).is_err());
    assert_eq!(fs::read(&manifest).unwrap(), before);
    assert!(
        crate::alignment::create_alignment(&manifest, "component-000", &p, [7, 7], 1.0, [7, 8, 9])
            .is_err()
    );
    assert!(
        crate::alignment::create_alignment(&manifest, "component-000", &p, [7, 8], -1.0, [7, 8, 9])
            .is_err()
    );
    assert!(
        crate::alignment::create_alignment(&manifest, "component-000", &p, [7, 8], 1.0, [7, 8, 8])
            .is_err()
    );
}

#[cfg(unix)]
#[test]
fn cancellation_kills_and_reaps_the_child() {
    let root = tempfile::tempdir().unwrap();
    let cancel = AtomicBool::new(false);
    let result = run_colmap(
        Path::new("/bin/sleep"),
        &args(&["20"]),
        &root.path().join("log"),
        &cancel,
        || cancel.store(true, Ordering::Relaxed),
    );
    assert!(result.unwrap_err().contains("cancelled"));
}

#[cfg(unix)]
#[test]
fn adapter_excludes_evaluation_frames_preserves_outputs_and_cleans_failed_work() {
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir().unwrap();
    let capture_root = root.path().join("capture");
    fs::create_dir(&capture_root).unwrap();
    fs::create_dir(capture_root.join("frames")).unwrap();
    let mut frames = Vec::new();
    for index in 1..=4 {
        let file = format!("frames/frame-{index:06}.jpg");
        image::RgbImage::new(1600, 900)
            .save(capture_root.join(&file))
            .unwrap();
        frames.push(crate::CapturedFrame {
            id: format!("frame-{index:06}"),
            file,
            timestamp_seconds: index as f64,
            sharpness: 10.0,
            evaluation: index == 4,
            width: 1600,
            height: 900,
        });
    }
    let video = crate::parse_video_metadata(
        r#"{"streams":[{"width":1600,"height":900,"duration":"4","codec_name":"h264"}]}"#,
    )
    .unwrap();
    let capture = CaptureDataset {
        format_version: 1,
        source: crate::CaptureSource {
            filename: "room.mov".into(),
            sha256: "0".repeat(64),
            bytes: 100,
            video,
        },
        settings: crate::CaptureOptions::default(),
        decoder: "test".into(),
        selector: "temporal-laplacian-v1".into(),
        scale_meters_per_unit: None,
        candidate_count: 4,
        frames,
        diagnostics: Vec::new(),
    };
    let manifest = capture_root.join("capture.json");
    write_json(&manifest, &capture).unwrap();
    let before = fs::read(&manifest).unwrap();
    let backend = root.path().join("fake-colmap");
    let model = root.path().join("fixture");
    fs::create_dir(&model).unwrap();
    fixture(&model);
    fs::write(&backend,r#"#!/bin/sh
if [ "$1" = "-h" ]; then echo 'COLMAP fixture'; exit 0; fi
if [ "$2" = "-h" ]; then echo '--FeatureExtraction.use_gpu --FeatureMatching.use_gpu --default_random_seed'; exit 0; fi
command="$1"
shift
while [ "$#" -gt 0 ]; do
  case "$1" in
    --output_path) output="$2" ;;
    --image_path) images="$2" ;;
  esac
  shift 2
done
case "$command" in
  feature_extractor) test ! -e "$images/frame-000004.jpg" ;;
  sequential_matcher) exit 0 ;;
  mapper) mkdir "$output/0" ;;
  model_converter) cp "$(dirname "$0")/fixture/"*.txt "$output/" ;;
  *) exit 1 ;;
esac
"#).unwrap();
    fs::set_permissions(&backend, fs::Permissions::from_mode(0o755)).unwrap();
    let options = ReconstructionOptions {
        colmap: backend.clone(),
        threads: 2,
    };
    let output = root.path().join("result");
    let r = recover_cameras(
        &manifest,
        &output,
        options.clone(),
        &AtomicBool::new(false),
        |_| {},
    )
    .unwrap();
    assert_eq!(r.inputs.len(), 3);
    assert_eq!(r.evaluation_frame_ids, vec!["frame-000004"]);
    assert!(!output.join("frames/frame-000004.jpg").exists());
    assert_eq!(
        read_reconstruction(&output.join("reconstruction.json"))
            .unwrap()
            .components
            .len(),
        1
    );
    assert_eq!(fs::read(&manifest).unwrap(), before);
    assert!(
        recover_cameras(
            &manifest,
            &output,
            options.clone(),
            &AtomicBool::new(false),
            |_| {}
        )
        .unwrap_err()
        .contains("already exists")
    );
    let cancelled = root.path().join("cancelled");
    assert!(
        recover_cameras(
            &manifest,
            &cancelled,
            options.clone(),
            &AtomicBool::new(true),
            |_| {}
        )
        .is_err()
    );
    assert!(!cancelled.exists());
    fs::write(&backend, "#!/bin/sh\nexit 1\n").unwrap();
    let failed = root.path().join("failed");
    assert!(recover_cameras(&manifest, &failed, options, &AtomicBool::new(false), |_| {}).is_err());
    assert!(!failed.exists());
    assert!(failed.with_extension("failed.log").exists());
    assert!(fs::read_dir(root.path()).unwrap().all(|e| {
        !e.unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".room-cameras-")
    }));
}
