#!/usr/bin/env python3
"""Compile the isolated study and capture it through the production engine.

Use --generate to rebuild the Blender sculpt/atlases first. No upload occurs.
"""
import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys

ROOT=Path(__file__).resolve().parent
TOOLS=ROOT.parent
ENGINE=TOOLS.parent/'rust'
STUDIO=TOOLS.parent/'studio'
sys.path.insert(0,str(TOOLS/'src'))
from cubacadabra.morph_release import build_morph_release


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--generate',action='store_true')
    parser.add_argument('--motion',action='store_true',help='Also capture walk, joint stress poses, and reduced LODs')
    args=parser.parse_args()
    study=ROOT/'studies/mockup-person'
    if args.generate:
        blender=shutil.which('blender') or '/Applications/Blender.app/Contents/MacOS/Blender'
        subprocess.run([blender,'--background','--factory-startup','--python',str(ROOT/'artwork/build_study.py')],check=True)
    release=build_morph_release(study,output=TOOLS/'.cubacadabra/generated/mockup-person',
        compiler_manifest=STUDIO/'crates/morph_authoring/Cargo.toml')
    lock=json.loads(release.lock_path.read_text())
    assets=[a for a in lock['assets'] if a.get('artifact')]
    print(f"Study: {len(assets)} packs, {sum(a['artifact']['bytes'] for a in assets):,} bytes",flush=True)
    cases=[('front',0.0713,'rest','near'),('three-quarter',.60,'rest','near'),('back',3.14159,'rest','near')]
    if args.motion:
        cases += [('walk',.60,'walk','near'),('walk-side',1.57,'walk','near'),
                  ('bend',1.1,'bend','near'),('jump-stress',.60,'jump','near'),
                  ('mid',.60,'rest','mid'),('far',.60,'rest','far')]
    for name,yaw,pose,lod in cases:
        env=dict(os.environ)
        # Review controls are explicit so stale portrait/yaw values cannot
        # silently change the reproducible full-character deliverable.
        env.pop('CUBA_STARTER_PORTRAIT',None)
        env.update(CUBA_STARTER_CATALOG=str(release.lock_path),CUBA_STARTER_SCALE='3',
            CUBA_STARTER_POSE=pose,CUBA_STARTER_LOD=lod,
            CUBA_STARTER_YAW=str(yaw),CUBA_STARTER_THUMBNAILS=str(study/'review'/name))
        subprocess.run(['cargo','test','--manifest-path',str(ENGINE/'Cargo.toml'),
                        'capture_starters','--','--ignored'],check=True,env=env,cwd=ENGINE)


if __name__=='__main__': main()
