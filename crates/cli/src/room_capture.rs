use crate::required_arg;
use cubacadabra_room_capture::{CaptureOptions, capture_video};
use std::{path::PathBuf, sync::atomic::AtomicBool};

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
