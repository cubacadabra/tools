use super::required_arg;
use std::path::PathBuf;

pub(crate) fn shard_command(args: &[String]) -> Result<(), String> {
    let mut input = None;
    let mut output = None;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--input" => input = Some(PathBuf::from(required_arg(args, index + 1, "--input")?)),
            "--output" => output = Some(PathBuf::from(required_arg(args, index + 1, "--output")?)),
            option => return Err(format!("unknown shard-collision-source option {option:?}")),
        }
        index += 2;
    }
    let input = input.ok_or("shard-collision-source requires --input")?;
    let output = output.ok_or("shard-collision-source requires --output")?;
    let count = cubacadabra_reference_import::shard_collision_source(&input, &output)?;
    println!("Wrote {count} collision triangles to {}", output.display());
    Ok(())
}

pub(crate) fn merge_command(args: &[String]) -> Result<(), String> {
    let mut inputs = Vec::new();
    let mut output = None;
    let mut round_decimals = None;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--input" => inputs.push(PathBuf::from(required_arg(args, index + 1, "--input")?)),
            "--output" => output = Some(PathBuf::from(required_arg(args, index + 1, "--output")?)),
            "--round-decimals" => {
                round_decimals = Some(
                    required_arg(args, index + 1, "--round-decimals")?
                        .parse::<u32>()
                        .map_err(|_| "--round-decimals must be 0 through 6".to_owned())?,
                )
            }
            option => return Err(format!("unknown merge-collision-sources option {option:?}")),
        }
        index += 2;
    }
    let output = output.ok_or("merge-collision-sources requires --output")?;
    let merged = cubacadabra_builder::merge_collision_sources(&inputs, round_decimals)
        .map_err(|error| error.0)?;
    let count = cubacadabra_reference_import::write_collision_source(&output, &merged)?;
    println!(
        "Wrote {count} merged collision triangles to {}",
        output.display()
    );
    Ok(())
}
