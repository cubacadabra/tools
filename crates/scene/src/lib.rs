//! The project-owned authoring scene format.
//!
//! This is deliberately an entity/component document rather than a copy of
//! Roblox's class hierarchy. Builders and editor hosts consume this model;
//! they do not own its serialized representation.

const AUTHORING_FLOAT_PRECISION: f64 = 1_000_000.0;

mod compile;
mod model;
mod serialization;
mod transforms;
mod validation;

pub use model::*;
pub use serialization::{parse_authoring_scene, serialize_authoring_scene};

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::BTreeMap;

    fn node(id: &str, parent_id: Option<&str>) -> AuthoringNode {
        AuthoringNode {
            id: id.to_owned(),
            parent_id: parent_id.map(str::to_owned),
            name: id.to_owned(),
            transform: Transform::default(),
            components: BTreeMap::new(),
            editor: EditorMetadata::default(),
            source: None,
        }
    }

    #[test]
    fn duplicate_ids_are_rejected() {
        let mut scene = AuthoringScene {
            format_version: 1,
            world_id: None,
            nodes: vec![node("same", None), node("same", None)],
        };
        let error = scene.validate().unwrap_err();
        assert!(error.contains("same"));
        scene.nodes[1].id = "other".to_owned();
        assert!(scene.validate().is_ok());
    }

    #[test]
    fn missing_parent_and_cycle_are_rejected() {
        let mut scene = AuthoringScene {
            format_version: 1,
            world_id: None,
            nodes: vec![node("child", Some("missing"))],
        };
        assert!(scene.validate().unwrap_err().contains("missing parent"));
        scene.nodes = vec![node("a", Some("b")), node("b", Some("a"))];
        assert!(scene.validate().unwrap_err().contains("cycle"));
    }

    #[test]
    fn serialization_is_deterministic_and_position_is_undoable_by_caller() {
        let scene = AuthoringScene {
            format_version: 1,
            world_id: None,
            nodes: vec![node("root", None)],
        };
        let first = serialize_authoring_scene(&scene).unwrap();
        let reloaded = parse_authoring_scene(&first).unwrap();
        let second = serialize_authoring_scene(&reloaded).unwrap();
        assert_eq!(first, second);
        let mut edited = reloaded.clone();
        let old = edited.set_position("root", [5.0, 0.0, 0.0]).unwrap();
        assert_eq!(old, [0.0; 3]);
        assert_eq!(
            edited.node("root").unwrap().transform.position,
            [5.0, 0.0, 0.0]
        );
        assert!(
            edited
                .set_scale("root", [MIN_AUTHORING_SCALE - 0.01, 1.0, 1.0])
                .is_err()
        );
    }

    #[test]
    fn serialization_normalizes_authoring_float_noise() {
        let mut root = node("root", None);
        root.transform.position = [1.2000000476837158, 3.812197685241699, -0.0];
        root.components.insert(
            "interaction".to_owned(),
            json!({ "id": "use", "radius": 3.812197685241699 }),
        );
        let source = serialize_authoring_scene(&AuthoringScene {
            format_version: 1,
            world_id: None,
            nodes: vec![root],
        })
        .unwrap();

        assert!(source.contains("1.2"));
        assert!(source.contains("3.812198"));
        assert!(!source.contains("1.2000000476837158"));
        assert!(!source.contains("3.812197685241699"));
    }

    #[test]
    fn world_resize_converts_position_and_scale_back_to_local_parent_space() {
        let mut parent = node("parent", None);
        parent.transform.scale = [2.0, 3.0, 4.0];
        let child = node("child", Some("parent"));
        let scene = AuthoringScene {
            format_version: 1,
            world_id: None,
            nodes: vec![parent, child],
        };
        let (position, scale) = scene
            .local_transform_for_world("child", [8.0, 6.0, 12.0], [4.0, 6.0, 8.0])
            .unwrap();
        assert_eq!(position, [4.0, 2.0, 3.0]);
        assert_eq!(scale, [2.0, 2.0, 2.0]);
    }

    #[test]
    fn mesh_compile_rejects_non_uniform_parent_scale_with_child_rotation() {
        let mut parent = node("parent", None);
        parent.transform.scale = [2.0, 1.0, 1.0];
        let mut child = node("child", Some("parent"));
        child.transform.rotation[1] = 0.5;
        child
            .components
            .insert("render".to_owned(), json!({ "mesh": "chair" }));
        let scene = AuthoringScene {
            format_version: 1,
            world_id: None,
            nodes: vec![parent, child],
        };
        let mut manifest = json!({
            "id": "game",
            "version": "0.1.0",
            "sdkVersion": "0.6.0",
            "startWorld": "world",
            "worlds": { "world": {} }
        });
        let error = scene.compile_into_manifest(&mut manifest).unwrap_err();
        assert!(error.contains("cannot compile losslessly"));
    }

    #[test]
    fn primitive_box_compiles_to_a_runtime_block() {
        let mut block = node("block", None);
        block.transform.position = [2.0, 1.0, -3.0];
        block.components.insert(
            "primitive".to_owned(),
            json!({
                "shape": "box",
                "size": [4.0, 1.0, 4.0],
                "material": "signal"
            }),
        );
        block
            .components
            .insert("collision".to_owned(), json!({ "kind": "box" }));
        let scene = AuthoringScene {
            format_version: 1,
            world_id: Some("world".to_owned()),
            nodes: vec![block],
        };
        let mut manifest = json!({
            "id": "game",
            "version": "0.1.0",
            "sdkVersion": "0.6.0",
            "startWorld": "world",
            "worlds": { "world": {} }
        });
        scene.compile_into_manifest(&mut manifest).unwrap();
        assert_eq!(manifest["worlds"]["world"]["blocks"][0]["id"], "block");
        assert_eq!(
            manifest["worlds"]["world"]["blocks"][0]["position"],
            json!([2.0, 1.0, -3.0])
        );
        assert_eq!(manifest["worlds"]["world"]["blocks"][0]["color"], "signal");
        assert_eq!(manifest["worlds"]["world"]["blocks"][0]["collidable"], true);
    }

    #[test]
    fn primitive_collidable_flag_compiles_as_a_visual_only_block() {
        let mut block = node("decor", None);
        block.components.insert(
            "primitive".to_owned(),
            json!({"shape": "box", "size": [1.0, 1.0, 1.0], "collidable": false}),
        );
        let scene = AuthoringScene {
            format_version: 1,
            world_id: Some("world".to_owned()),
            nodes: vec![block],
        };
        let mut manifest = json!({"startWorld": "world", "worlds": {"world": {}}});
        scene.compile_into_manifest(&mut manifest).unwrap();
        assert_eq!(
            manifest["worlds"]["world"]["blocks"][0]["collidable"],
            false
        );
        assert_eq!(manifest["worlds"]["world"]["blocks"][0]["castShadow"], true);
    }

    #[test]
    fn primitive_cast_shadow_flag_compiles_to_a_non_shadowing_block() {
        let mut block = node("decor", None);
        block.components.insert(
            "primitive".to_owned(),
            json!({
                "shape": "box",
                "size": [1.0, 1.0, 1.0],
                "castShadow": false
            }),
        );
        let scene = AuthoringScene {
            format_version: 1,
            world_id: Some("world".to_owned()),
            nodes: vec![block],
        };
        let mut manifest = json!({"startWorld": "world", "worlds": {"world": {}}});
        scene.compile_into_manifest(&mut manifest).unwrap();
        assert_eq!(
            manifest["worlds"]["world"]["blocks"][0]["castShadow"],
            false
        );
    }

    #[test]
    fn primitive_runtime_id_overrides_the_authoring_node_id() {
        let mut block = node("block-platform", None);
        block.transform.position = [2.0, 1.0, -3.0];
        block.components.insert(
            "primitive".to_owned(),
            json!({
                "shape": "box",
                "size": [4.0, 1.0, 4.0],
                "runtimeId": "platform"
            }),
        );
        let scene = AuthoringScene {
            format_version: 1,
            world_id: Some("world".to_owned()),
            nodes: vec![block],
        };
        let mut manifest = json!({
            "startWorld": "world",
            "worlds": { "world": {} }
        });

        scene.compile_into_manifest(&mut manifest).unwrap();

        assert_eq!(manifest["worlds"]["world"]["blocks"][0]["id"], "platform");
    }

    #[test]
    fn mesh_collision_compiles_as_a_world_transform_handoff() {
        let mut chair = node("chair", None);
        chair.transform.position = [10.0, 2.0, 4.0];
        chair.transform.rotation[1] = 0.5;
        chair.transform.scale = [2.0, 1.5, 0.75];
        chair
            .components
            .insert("render".to_owned(), json!({ "mesh": "chair" }));
        chair.components.insert(
            "collision".to_owned(),
            json!({ "kind": "mesh", "asset": "chair" }),
        );
        let scene = AuthoringScene {
            format_version: 1,
            world_id: Some("world".to_owned()),
            nodes: vec![chair],
        };
        let mut manifest = json!({
            "startWorld": "world",
            "worlds": { "world": {} }
        });

        scene.compile_into_manifest(&mut manifest).unwrap();

        assert_eq!(
            manifest["worlds"]["world"][AUTHORING_COLLISION_INSTANCES_KEY][0]["asset"],
            "chair"
        );
        assert_eq!(
            manifest["worlds"]["world"][AUTHORING_COLLISION_INSTANCES_KEY][0]["position"],
            json!([10.0, 2.0, 4.0])
        );
        assert_eq!(
            manifest["worlds"]["world"][AUTHORING_COLLISION_INSTANCES_KEY][0]["scale"],
            json!([2.0, 1.5, 0.75])
        );
    }

    #[test]
    fn primitive_box_bakes_parent_scale_into_runtime_size() {
        let mut group = node("group", None);
        group.transform.scale = [2.0, 2.0, 2.0];
        let mut block = node("block", Some("group"));
        block.components.insert(
            "primitive".to_owned(),
            json!({ "shape": "box", "size": [4.0, 1.0, 4.0] }),
        );
        let scene = AuthoringScene {
            format_version: 1,
            world_id: Some("world".to_owned()),
            nodes: vec![group, block],
        };
        let mut manifest = json!({ "startWorld": "world", "worlds": { "world": {} } });

        scene.compile_into_manifest(&mut manifest).unwrap();

        assert_eq!(
            manifest["worlds"]["world"]["blocks"][0]["size"],
            json!([8.0, 2.0, 8.0])
        );
    }

    #[test]
    fn primitive_box_rejects_non_axis_aligned_rotation_and_shear() {
        let mut rotated = node("rotated", None);
        rotated.transform.rotation[1] = 0.25;
        rotated.components.insert(
            "primitive".to_owned(),
            json!({ "shape": "box", "size": [4.0, 1.0, 4.0] }),
        );
        let mut manifest = json!({ "startWorld": "world", "worlds": { "world": {} } });
        let error = AuthoringScene {
            format_version: 1,
            world_id: Some("world".to_owned()),
            nodes: vec![rotated],
        }
        .compile_into_manifest(&mut manifest)
        .unwrap_err();
        assert!(error.contains("non-axis-aligned rotation"));

        let mut parent = node("parent", None);
        parent.transform.scale = [2.0, 1.0, 1.0];
        let mut sheared = node("sheared", Some("parent"));
        sheared.transform.rotation[1] = 0.5;
        sheared.components.insert(
            "primitive".to_owned(),
            json!({ "shape": "box", "size": [4.0, 1.0, 4.0] }),
        );
        let error = AuthoringScene {
            format_version: 1,
            world_id: Some("world".to_owned()),
            nodes: vec![parent, sheared],
        }
        .compile_into_manifest(&mut manifest)
        .unwrap_err();
        assert!(error.contains("shear"));
    }

    #[test]
    fn compiling_a_scene_without_blocks_clears_stale_runtime_blocks() {
        let scene = AuthoringScene {
            format_version: 1,
            world_id: Some("world".to_owned()),
            nodes: vec![node("world", None)],
        };
        let mut manifest = json!({
            "startWorld": "world",
            "worlds": {
                "world": {
                    "blocks": [{ "id": "stale" }]
                }
            }
        });

        scene.compile_into_manifest(&mut manifest).unwrap();

        assert_eq!(manifest["worlds"]["world"]["blocks"], json!([]));
    }

    #[test]
    fn serialization_omits_implicit_optional_metadata() {
        let source = r#"{
  "formatVersion": 1,
  "nodes": [{
    "id": "root",
    "name": "Root",
    "transform": {
      "position": [0, 0, 0],
      "rotation": [0, 0, 0],
      "scale": [1, 1, 1]
    },
    "components": {},
    "editor": {
      "visible": true,
      "locked": false
    },
    "source": {
      "format": "roblox"
    }
  }]
}"#;

        let scene = parse_authoring_scene(source).unwrap();
        let rendered = serialize_authoring_scene(&scene).unwrap();

        assert!(!rendered.contains("\"parentId\""));
        assert!(!rendered.contains("\"lockReason\""));
        assert!(!rendered.contains("\"class\""));
        assert!(!rendered.contains("\"path\""));
        assert!(rendered.contains("\"source\": {"));

        let source = source.replace(
            ",\n    \"source\": {\n      \"format\": \"roblox\"\n    }",
            "",
        );
        let scene_without_source = parse_authoring_scene(&source).unwrap();
        let rendered_without_source = serialize_authoring_scene(&scene_without_source).unwrap();
        assert!(!rendered_without_source.contains("\"source\""));
    }

    #[test]
    fn world_transforms_compose_parent_rotation_scale_and_inverse_position() {
        let mut root = node("root", None);
        root.transform.position = [10.0, 0.0, 4.0];
        root.transform.rotation = [0.0, std::f32::consts::FRAC_PI_2, 0.0];
        root.transform.scale = [2.0, 2.0, 2.0];
        let mut child = node("child", Some("root"));
        child.transform.position = [1.0, 0.0, 0.0];
        let scene = AuthoringScene {
            format_version: 1,
            world_id: None,
            nodes: vec![root, child],
        };

        let world = scene.world_transform("child").unwrap();
        assert!((world.position[0] - 10.0).abs() < 0.0001);
        assert!((world.position[2] - 2.0).abs() < 0.0001);
        let local = scene
            .local_position_for_world("child", world.position)
            .unwrap();
        assert!(
            local
                .into_iter()
                .zip([1.0, 0.0, 0.0])
                .all(|(actual, expected)| (actual - expected).abs() < 0.0001)
        );
    }

    #[test]
    fn world_transform_keeps_y_only_rotation_canonical_for_meshes() {
        let mut mesh = node("mesh", None);
        mesh.transform.rotation = [0.0, -2.879817, 0.0];
        mesh.components
            .insert("render".to_owned(), json!({ "mesh": "chair" }));
        let scene = AuthoringScene {
            format_version: 1,
            world_id: None,
            nodes: vec![mesh],
        };

        let world = scene.world_transform("mesh").unwrap();
        assert!(world.rotation[0].abs() < 0.0001);
        assert!((world.rotation[1] + 2.879817).abs() < 0.0001);
        assert!(world.rotation[2].abs() < 0.0001);
    }

    #[test]
    fn components_compile_to_the_existing_runtime_collections() {
        let mut mesh = node("mesh", Some("root"));
        mesh.transform.scale = [1.0, 1.5, 0.75];
        mesh.components
            .insert("render".to_owned(), json!({ "mesh": "casino" }));
        let mut sign = node("sign", Some("root"));
        sign.transform.position = [1.0, 2.0, 3.0];
        sign.components.insert(
            "text".to_owned(),
            json!({ "text": "TABLES", "maxWidth": 8, "color": "butter" }),
        );
        let mut interaction = node("interaction", Some("root"));
        interaction.components.insert(
            "interaction".to_owned(),
            json!({ "id": "table", "label": "PLAY", "radius": 7.5 }),
        );
        let mut actor = node("maya", Some("root"));
        actor.transform.position = [4.0, 0.0, -2.0];
        actor.transform.rotation[1] = 0.25;
        actor.components.insert(
            "actor".to_owned(),
            json!({
                "id": "maya",
                "name": "Maya",
                "yaw": 0.5,
                "appearance": { "skin": "#E8AE86" }
            }),
        );
        let mut ladder = node("ladder", Some("root"));
        ladder.components.insert(
            "ladder".to_owned(),
            json!({ "id": "up", "size": [2, 6, 1], "climbAxis": "z" }),
        );
        let mut checkpoint = node("checkpoint", Some("root"));
        checkpoint.components.insert(
            "checkpoint".to_owned(),
            json!({ "id": "save", "radius": 3 }),
        );
        let mut hazard = node("hazard", Some("root"));
        hazard.components.insert(
            "hazard".to_owned(),
            json!({ "id": "hurt", "kind": "damage", "size": [4, 1, 4], "damagePerSecond": 10 }),
        );
        let mut safe_zone = node("safe", Some("root"));
        safe_zone.components.insert(
            "safeZone".to_owned(),
            json!({ "id": "camp", "radius": 5, "healPerSecond": 4 }),
        );
        let scene = AuthoringScene {
            format_version: 1,
            world_id: None,
            nodes: vec![
                node("root", None),
                mesh,
                sign,
                interaction,
                actor,
                ladder,
                checkpoint,
                hazard,
                safe_zone,
            ],
        };
        let mut manifest = json!({
            "startWorld": "world",
            "worlds": { "world": {} }
        });
        scene.compile_into_manifest(&mut manifest).unwrap();
        let world = &manifest["worlds"]["world"];
        assert_eq!(world["decorations"][0]["asset"], "casino");
        assert_eq!(world["decorations"][0]["scale3"], json!([1.0, 1.5, 0.75]));
        assert_eq!(world["signs"][0]["text"], "TABLES");
        assert_eq!(world["interactions"][0]["id"], "table");
        assert_eq!(world["actors"][0]["id"], "maya");
        assert_eq!(world["actors"][0]["name"], "Maya");
        assert_eq!(world["actors"][0]["position"], json!([4.0, 0.0, -2.0]));
        assert!((world["actors"][0]["yaw"].as_f64().unwrap() - 0.75).abs() < 0.0001);
        assert_eq!(world["ladders"][0]["id"], "up");
        assert_eq!(world["checkpoints"][0]["id"], "save");
        assert_eq!(world["hazards"][0]["damagePerSecond"], 10);
        assert_eq!(world["safeZones"][0]["healPerSecond"], 4);
    }
}
