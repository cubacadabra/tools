"""Refresh the skinned wardrobe using Studio's canonical rig authoring tools."""
import json
import sys
from pathlib import Path

ROOT=Path(__file__).resolve().parents[1]
sys.path.insert(0,str(ROOT.parents[1]/"studio/tools"))
import generate_person_asset as person
import generate_person_clothing_assets as clothing


def build():
    paths=[]
    for variant_name,relative in [("person-01","base/person/person_skinned.glb"),
                                  ("person-02","base/person-02/person_02.glb")]:
        path=ROOT/"source/morphs"/relative
        variant=person.PERSON_VARIANTS[variant_name] | {"avatar_tint":True}
        person.make_glb(path,variant)
        counts={level:len(person.weld_surface(person.lod_geometry(detail,variant["head_profile"]))[1])//3
                for level,detail in [("near",3),("mid",2),("far",1)]}
        update(path,counts,f"Cubacadabra starter-set presentation skinned {variant_name} base")
        paths.append(path)
    for key,relative in [
        ("top","top/person-top/person_top.glb"),
        ("bottom","bottom/person-bottom/person_bottom.glb"),
        ("shoes","footwear/person-shoes/person_shoes.glb"),
        ("short_sleeve_collared","top/short-sleeve-collared/person_short_sleeve_collared.glb"),
        ("slacks","bottom/slacks/person_slacks.glb"),
        ("sparkles","footwear/sparkles/person_sparkles.glb"),
    ]:
        path=ROOT/"source/morphs"/relative
        counts=clothing.make_glb(path,clothing.ASSETS[key])
        update(path,counts,"Cubacadabra starter-set presentation wardrobe")
        paths.append(path)
    return paths


def update(path,counts,provenance):
    sidecar=path.with_suffix(".morph.json")
    document=json.loads(sidecar.read_text())
    document["asset"]["lod"]=counts
    document["asset"]["provenance"]["source"]=provenance
    capabilities=document["asset"].setdefault("requiredCapabilities",[])
    if "material.base-color-texture.v1" not in capabilities:
        capabilities.append("material.base-color-texture.v1")
    sidecar.write_text(json.dumps(document,indent=2)+"\n")
    print(f"{path.stem}: {counts}")
