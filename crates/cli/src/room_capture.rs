use crate::required_arg;
use cubacadabra_room_capture::{CaptureOptions, capture_video};
use std::{path::PathBuf, sync::atomic::AtomicBool};

pub(crate) fn measurement_command(args: &[String]) -> Result<(), String> {
    use cubacadabra_room_capture::measurements::{MeasuredObject, record_measurement};
    let mut manifest = None;
    let mut id = None;
    let mut label = None;
    let mut dimensions = None;
    let mut index = 0;
    while index < args.len() {
        let name = args[index].as_str();
        let value = required_arg(args, index + 1, name)?;
        match name {
            "--reconstruction" => manifest = Some(PathBuf::from(value)),
            "--object" => id = Some(value.to_owned()),
            "--label" => label = Some(value.to_owned()),
            "--dimensions-meters" => {
                dimensions = Some(
                    value
                        .split(',')
                        .map(|v| {
                            v.parse::<f64>()
                                .map_err(|_| "Invalid measured dimension".to_owned())
                        })
                        .collect::<Result<Vec<_>, _>>()?
                        .try_into()
                        .map_err(|_| "Provide length,depth,height in meters".to_owned())?,
                )
            }
            _ => return Err(format!("Unknown measure-capture option: {name}")),
        }
        index += 2;
    }
    let manifest =
        manifest.ok_or("measure-capture requires --reconstruction <reconstruction.json>")?;
    let id = id.ok_or("measure-capture requires --object <object-id>")?;
    let [length_meters, depth_meters, height_meters]: [f64; 3] =
        dimensions.ok_or("measure-capture requires --dimensions-meters <length,depth,height>")?;
    record_measurement(
        &manifest,
        MeasuredObject {
            id: id.clone(),
            label: label.unwrap_or_else(|| id.clone()),
            length_meters,
            depth_meters,
            height_meters,
        },
    )?;
    println!(
        "Saved dimensions for {id}: {length_meters} × {depth_meters} × {height_meters} meters. Select reviewed point anchors before aligning."
    );
    Ok(())
}

pub(crate) fn reconstruct_command(args: &[String]) -> Result<(), String> {
    use cubacadabra_room_capture::reconstruction::{ReconstructionOptions, recover_cameras};
    let mut capture = None;
    let mut output = None;
    let mut options = ReconstructionOptions::default();
    let mut index = 0;
    while index < args.len() {
        let name = args[index].as_str();
        let value = required_arg(args, index + 1, name)?;
        match name {
            "--capture" => capture = Some(PathBuf::from(value)),
            "--output" => output = Some(PathBuf::from(value)),
            "--colmap" => options.colmap = PathBuf::from(value),
            "--threads" => {
                options.threads = value.parse().map_err(|_| "--threads requires an integer")?
            }
            _ => return Err(format!("Unknown recover-cameras option: {name}")),
        }
        index += 2;
    }
    let capture = capture.ok_or("recover-cameras requires --capture <capture.json>")?;
    let output = output.ok_or("recover-cameras requires --output <new-folder>")?;
    let mut last_stage = None;
    let result = recover_cameras(
        &capture,
        &output,
        options,
        &AtomicBool::new(false),
        |progress| {
            if last_stage != Some(progress.stage) {
                eprintln!(
                    "{} ({}s elapsed)",
                    progress.stage.label(),
                    progress.elapsed.as_secs()
                );
                last_stage = Some(progress.stage);
            }
        },
    )?;
    println!(
        "Registered {} / {} reconstruction frames in {} component(s). Saved {}",
        result.inputs.len() - result.unregistered_frame_ids.len(),
        result.inputs.len(),
        result.components.len(),
        output.join("reconstruction.json").display()
    );
    for component in &result.components {
        println!(
            "{}: {} cameras, {} points, mean point reprojection error {:.3}px",
            component.id,
            component.frames.len(),
            component.point_count,
            component.mean_point_reprojection_error_pixels
        );
    }
    for diagnostic in &result.diagnostics {
        println!("{diagnostic}");
    }
    Ok(())
}

pub(crate) fn align_command(args: &[String]) -> Result<(), String> {
    use cubacadabra_room_capture::{
        alignment::{create_alignment, save_alignment},
        reconstruction::{read_component_points, read_reconstruction},
    };
    let mut manifest = None;
    let mut component = None;
    let mut endpoints = None;
    let mut floor = None;
    let mut meters = None;
    let mut index = 0;
    while index < args.len() {
        let name = args[index].as_str();
        let value = required_arg(args, index + 1, name)?;
        match name {
            "--reconstruction" => manifest = Some(PathBuf::from(value)),
            "--component" => component = Some(value.to_owned()),
            "--distance-points" => endpoints = Some(parse_ids::<2>(value)?),
            "--floor-points" => floor = Some(parse_ids::<3>(value)?),
            "--meters" => {
                meters = Some(
                    value
                        .parse::<f64>()
                        .map_err(|_| "--meters requires a number")?,
                )
            }
            _ => return Err(format!("Unknown align-capture option: {name}")),
        }
        index += 2;
    }
    let manifest =
        manifest.ok_or("align-capture requires --reconstruction <reconstruction.json>")?;
    let component = component.ok_or("align-capture requires --component <component-id>")?;
    let r = read_reconstruction(&manifest)?;
    let c = r
        .components
        .iter()
        .find(|c| c.id == component)
        .ok_or("Unknown component")?;
    let points = read_component_points(manifest.parent().unwrap(), c)?;
    let a = create_alignment(
        &manifest,
        &component,
        &points,
        endpoints.ok_or("--distance-points requires two comma-separated point IDs")?,
        meters.ok_or("--meters requires a measured distance")?,
        floor.ok_or("--floor-points requires three comma-separated point IDs")?,
    )?;
    save_alignment(&manifest, &a)?;
    println!(
        "Saved reviewed alignment: {:.6} meters per reconstruction unit; +Y up, floor at y=0.",
        a.meters_per_unit
    );
    Ok(())
}

fn parse_ids<const N: usize>(value: &str) -> Result<[u64; N], String> {
    value
        .split(',')
        .map(|s| s.parse::<u64>().map_err(|_| "Invalid point ID".to_owned()))
        .collect::<Result<Vec<_>, _>>()?
        .try_into()
        .map_err(|_| format!("Expected {N} comma-separated point IDs"))
}

pub(crate) fn capture_command(args: &[String]) -> Result<(), String> {
    let mut source = None;
    let mut output = None;
    let mut options = CaptureOptions::default();
    let mut index = 0;
    while index < args.len() {
        let option = args[index].as_str();
        let value = required_arg(args, index + 1, option)?;
        match option {
            "--video" => source = Some(PathBuf::from(value)),
            "--output" => output = Some(PathBuf::from(value)),
            "--max-frames" => {
                options.max_frames = value
                    .parse()
                    .map_err(|_| "--max-frames requires an integer")?
            }
            "--max-dimension" => {
                options.max_dimension = value
                    .parse()
                    .map_err(|_| "--max-dimension requires an integer")?
            }
            _ => return Err(format!("Unknown capture-video option: {option}")),
        }
        index += 2;
    }
    let source = source.ok_or("capture-video requires --video <local-video>")?;
    let output = output.ok_or("capture-video requires --output <new-capture-folder>")?;
    let capture = capture_video(
        &source,
        &output,
        options,
        &AtomicBool::new(false),
        |message| eprintln!("{message}"),
    )?;
    let evaluation = capture
        .frames
        .iter()
        .filter(|frame| frame.evaluation)
        .count();
    println!(
        "Saved {} frames ({} reconstruction, {} evaluation) to {}",
        capture.frames.len(),
        capture.frames.len() - evaluation,
        evaluation,
        output.display()
    );
    println!(
        "Next: review frames, recover camera poses, and measure a visible reference distance."
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reconstruction_and_alignment_commands_reject_missing_or_malformed_arguments() {
        assert!(
            measurement_command(&[])
                .unwrap_err()
                .contains("--reconstruction")
        );
        assert!(
            measurement_command(&["--dimensions-meters".into(), "1,2".into()])
                .unwrap_err()
                .contains("length,depth,height")
        );
        assert!(reconstruct_command(&[]).unwrap_err().contains("--capture"));
        assert!(
            reconstruct_command(&["--capture".into(), "capture.json".into()])
                .unwrap_err()
                .contains("--output")
        );
        assert!(
            reconstruct_command(&["--threads".into(), "many".into()])
                .unwrap_err()
                .contains("integer")
        );
        assert!(align_command(&[]).unwrap_err().contains("--reconstruction"));
        assert_eq!(parse_ids::<2>("7,8").unwrap(), [7, 8]);
        assert!(parse_ids::<2>("7,8,9").is_err());
        assert!(parse_ids::<3>("7,eight,9").is_err());
    }
}
