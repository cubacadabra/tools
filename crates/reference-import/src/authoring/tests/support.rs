use super::super::*;
use std::collections::HashMap;

pub(super) use cubacadabra_scene::{AUTHORING_SCENE_FORMAT_VERSION, serialize_authoring_scene};
use rbx_dom_weak::types::Variant;
pub(super) use tempfile::tempdir;

pub(super) const PLACE: &str = r#"<roblox version="4">
  <Item class="Workspace" referent="RBX0">
<Properties><string name="Name">Workspace</string></Properties>
<Item class="Model" referent="RBX1">
  <Properties><string name="Name">Yard</string><string name="CustomState">keep-me</string></Properties>
  <Item class="Part" referent="RBX2">
    <Properties>
      <string name="Name">Block</string>
      <CoordinateFrame name="CFrame"><X>1</X><Y>2</Y><Z>3</Z><R00>1</R00><R01>0</R01><R02>0</R02><R10>0</R10><R11>1</R11><R12>0</R12><R20>0</R20><R21>0</R21><R22>1</R22></CoordinateFrame>
      <Vector3 name="Size"><X>4</X><Y>2</Y><Z>6</Z></Vector3>
      <Color3 name="Color"><R>1</R><G>0</G><B>0</B></Color3>
      <bool name="Anchored">true</bool><bool name="CanCollide">true</bool><bool name="CastShadow">true</bool>
      <token name="Material">256</token><token name="Shape">1</token>
    </Properties>
  </Item>
  <Item class="ParticleEmitter" referent="RBX3">
    <Properties><string name="Name">PreserveMe</string><float name="Rate">9</float></Properties>
  </Item>
</Item>
  </Item>
</roblox>"#;

pub(super) const KITCHEN_SINK_PLACE: &str =
    include_str!("../../../tests/fixtures/roblox-roundtrip/kitchen-sink.rbxlx");
pub(super) const GENERICITY_AUDIT_PLACE: &str =
    include_str!("../../../tests/fixtures/roblox-roundtrip/services-references-and-types.rbxlx");

pub(super) fn base_scene() -> AuthoringScene {
    AuthoringScene {
        format_version: AUTHORING_SCENE_FORMAT_VERSION,
        world_id: Some("world".to_owned()),
        nodes: vec![AuthoringNode {
            id: "world".to_owned(),
            parent_id: None,
            name: "World".to_owned(),
            transform: Transform::default(),
            components: BTreeMap::new(),
            editor: EditorMetadata::default(),
            source: None,
        }],
    }
}

pub(super) fn named_instance<'a>(dom: &'a WeakDom, name: &str) -> &'a Instance {
    dom.descendants()
        .find(|instance| instance.name == name)
        .unwrap_or_else(|| panic!("missing Roblox instance named {name}"))
}

pub(super) fn parent_name<'a>(dom: &'a WeakDom, instance: &Instance) -> &'a str {
    &dom.get_by_ref(instance.parent()).unwrap().name
}

pub(super) fn child_names(instance: &Instance, dom: &WeakDom) -> Vec<String> {
    instance
        .children()
        .iter()
        .map(|reference| dom.get_by_ref(*reference).unwrap().name.clone())
        .collect()
}

pub(super) fn assert_preserved_instance(source: &WeakDom, exported: &WeakDom, name: &str) {
    let source = named_instance(source, name);
    let exported = named_instance(exported, name);
    assert_eq!(exported.class, source.class, "class changed for {name}");
    assert_eq!(
        exported.properties, source.properties,
        "properties changed for {name}"
    );
}

pub(super) fn encoded_attributes(instance: &Instance) -> Vec<u8> {
    let Some(Variant::Attributes(attributes)) = instance.properties.get(&ustr("Attributes")) else {
        panic!("{} has no decoded Roblox attributes", instance.name);
    };
    let mut encoded = Vec::new();
    attributes.to_writer(&mut encoded).unwrap();
    encoded
}

pub(super) fn assert_dom_semantics(
    source: &WeakDom,
    exported: &WeakDom,
    renamed: &BTreeMap<&str, &str>,
    edited_part: Option<&str>,
) {
    let source_paths = ordinal_paths(source);
    let exported_paths = ordinal_paths(exported);
    assert_instance_semantics(
        source,
        source.root_ref(),
        exported,
        exported.root_ref(),
        renamed,
        edited_part,
        &source_paths,
        &exported_paths,
    );
}

#[allow(clippy::too_many_arguments)]
pub(super) fn assert_instance_semantics(
    source_dom: &WeakDom,
    source_ref: Ref,
    exported_dom: &WeakDom,
    exported_ref: Ref,
    renamed: &BTreeMap<&str, &str>,
    edited_part: Option<&str>,
    source_paths: &HashMap<Ref, Vec<usize>>,
    exported_paths: &HashMap<Ref, Vec<usize>>,
) {
    let source = source_dom.get_by_ref(source_ref).unwrap();
    let exported = exported_dom.get_by_ref(exported_ref).unwrap();
    assert_eq!(
        exported.class, source.class,
        "class changed for {}",
        source.name
    );
    assert_eq!(
        exported.name,
        renamed
            .get(source.name.as_str())
            .copied()
            .unwrap_or(&source.name),
        "name changed unexpectedly for {}",
        source.name
    );
    let excluded = if edited_part == Some(source.name.as_str()) {
        &["CFrame", "Size"][..]
    } else {
        &[][..]
    };
    assert_property_semantics(source, exported, excluded, source_paths, exported_paths);
    assert_eq!(
        exported.children().len(),
        source.children().len(),
        "child count changed for {}",
        source.name
    );
    for (source_child, exported_child) in source.children().iter().zip(exported.children().iter()) {
        assert_instance_semantics(
            source_dom,
            *source_child,
            exported_dom,
            *exported_child,
            renamed,
            edited_part,
            source_paths,
            exported_paths,
        );
    }
}

pub(super) fn assert_property_semantics(
    source: &Instance,
    exported: &Instance,
    excluded: &[&str],
    source_paths: &HashMap<Ref, Vec<usize>>,
    exported_paths: &HashMap<Ref, Vec<usize>>,
) {
    let source_names = source
        .properties
        .keys()
        .filter(|name| !excluded.contains(&name.as_str()))
        .map(ToString::to_string)
        .collect::<BTreeSet<_>>();
    let exported_names = exported
        .properties
        .keys()
        .filter(|name| !excluded.contains(&name.as_str()))
        .map(ToString::to_string)
        .collect::<BTreeSet<_>>();
    assert_eq!(
        exported_names, source_names,
        "property names changed for {}",
        source.name
    );
    for name in source_names {
        let source_value = source.properties.get(&ustr(&name)).unwrap();
        let exported_value = exported.properties.get(&ustr(&name)).unwrap();
        match (source_value, exported_value) {
            (Variant::Ref(source_ref), Variant::Ref(exported_ref)) => assert_eq!(
                exported_paths.get(exported_ref),
                source_paths.get(source_ref),
                "reference target changed for {}.{name}",
                source.name
            ),
            _ => assert_eq!(
                format!("{exported_value:?}"),
                format!("{source_value:?}"),
                "property changed for {}.{name}",
                source.name
            ),
        }
    }
}

pub(super) fn ordinal_paths(dom: &WeakDom) -> HashMap<Ref, Vec<usize>> {
    fn collect(
        dom: &WeakDom,
        reference: Ref,
        path: &mut Vec<usize>,
        output: &mut HashMap<Ref, Vec<usize>>,
    ) {
        output.insert(reference, path.clone());
        let instance = dom.get_by_ref(reference).unwrap();
        for (index, child) in instance.children().iter().enumerate() {
            path.push(index);
            collect(dom, *child, path, output);
            path.pop();
        }
    }

    let mut output = HashMap::new();
    collect(dom, dom.root_ref(), &mut Vec::new(), &mut output);
    output
}
