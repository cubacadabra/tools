use serde_json::{Value, json};
use std::{fs, process::Command};

#[test]
fn merges_relative_source_indexes_and_preserves_output_on_invalid_geometry() {
    let root = tempfile::tempdir().unwrap();
    let part = json!({"formatVersion": 1, "triangles": [
        [[0.1234,0,0], [1.1234,0,0], [0.1234,0,1]]
    ]});
    fs::write(root.path().join("part.json"), part.to_string()).unwrap();
    fs::write(
        root.path().join("index.json"),
        json!({
            "formatVersion": 1, "sources": ["part.json"]
        })
        .to_string(),
    )
    .unwrap();
    let command = || {
        let mut command = Command::new(env!("CARGO_BIN_EXE_cubacadabra"));
        command.current_dir(root.path()).args([
            "merge-collision-sources",
            "--input",
            "index.json",
            "--input",
            "part.json",
            "--round-decimals",
            "3",
            "--output",
            "merged.json",
        ]);
        command
    };
    let result = command().output().unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let bytes = fs::read(root.path().join("merged.json")).unwrap();
    let merged: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(merged["triangles"].as_array().unwrap().len(), 2);
    assert_eq!(merged["triangles"][0][0][0], 0.123);
    fs::write(
        root.path().join("part.json"),
        json!({"formatVersion": 1,
            "triangles": [[[0,0,0], [0.0004,0,0], [0,0,1]]]
        })
        .to_string(),
    )
    .unwrap();
    let result = command().output().unwrap();
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("non-zero area"));
    assert_eq!(fs::read(root.path().join("merged.json")).unwrap(), bytes);
}
