"""Import the exact delivery GLBs as editable Blender scenes, one per component.

Run through generate_parts.py --blend. No separate beauty meshes: Blender and
Studio receive the same artwork. Mid/far are retained in hidden collections.
"""
import sys
from pathlib import Path
import bpy

for argument in sys.argv[sys.argv.index("--")+1:]:
    path=Path(argument)
    bpy.ops.wm.read_factory_settings(use_empty=True)
    bpy.ops.import_scene.gltf(filepath=str(path))
    for obj in list(bpy.context.scene.objects):
        if obj.type != "MESH": continue
        level=next((level for level in ("near","mid","far") if level in obj.name.lower()),"near")
        collection=bpy.data.collections.new(f"{level.upper()} · {path.stem}")
        bpy.context.scene.collection.children.link(collection)
        for previous in list(obj.users_collection): previous.objects.unlink(obj)
        collection.objects.link(obj)
        obj.hide_set(level!="near")
        obj.hide_render=level!="near"
        for polygon in obj.data.polygons: polygon.use_smooth=True
        obj.select_set(level=="near")
        if level=="near": bpy.context.view_layer.objects.active=obj
    bpy.context.scene["authoring"]="Edit the near/mid/far meshes; export GLB with these node names. Preserve head-local origin."
    bpy.context.preferences.filepaths.save_version=0
    bpy.ops.file.pack_all()
    bpy.ops.wm.save_as_mainfile(filepath=str(path.with_suffix(".blend")),compress=True)
