#!/usr/bin/env python3
"""Compile the isolated study and capture it through the production engine.

Use --generate to rebuild the Blender sculpt/atlases first. No upload occurs.
"""
import argparse
import hashlib
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
    parser.add_argument('--label',help='Preserve an iteration in review/iterations/LABEL')
    parser.add_argument('--beauty-only',action='store_true',help='Only the fixed exact-preset reference view')
    args=parser.parse_args()
    if args.label and (not args.label.replace('-','').replace('_','').isalnum()):
        parser.error('--label must contain only letters, numbers, hyphens or underscores')
    study=ROOT/'studies/mockup-person'
    output=study/'review'
    if args.label:
        output=output/'iterations'/args.label
        if output.exists(): parser.error(f'Iteration already exists: {output}')
    if args.generate:
        blender=shutil.which('blender') or '/Applications/Blender.app/Contents/MacOS/Blender'
        subprocess.run([blender,'--background','--factory-startup','--python-exit-code','1',
                        '--python',str(ROOT/'artwork/build_study.py')],check=True)
    release=build_morph_release(study,output=TOOLS/'.cubacadabra/generated/mockup-person',
        compiler_manifest=STUDIO/'crates/morph_authoring/Cargo.toml')
    lock=json.loads(release.lock_path.read_text())
    assets=[a for a in lock['assets'] if a.get('artifact')]
    print(f"Study: {len(assets)} packs, {sum(a['artifact']['bytes'] for a in assets):,} bytes",flush=True)
    cases=[('beauty',.26,'rest','near')]
    if not args.beauty_only:
        cases += [('front',0.,'rest','near'),('three-quarter',.60,'rest','near'),('back',3.14159,'rest','near'),
                  ('left',1.570796,'rest','near'),('right',-1.570796,'rest','near'),('top',0.,'rest','near')]
    if args.motion:
        cases += [('walk',.60,'walk','near'),('walk-side',1.57,'walk','near'),
                  ('bend',1.1,'bend','near'),('jump-stress',.60,'jump','near'),
                  ('mid',.60,'rest','mid'),('far',.60,'rest','far')]
    for name,yaw,pose,lod in cases:
        env=dict(os.environ)
        # Review controls are explicit so stale portrait/yaw values cannot
        # silently change the reproducible full-character deliverable.
        env.pop('CUBA_STARTER_PORTRAIT',None)
        env.pop('CUBA_STARTER_REFERENCE',None)
        if name=='beauty': env['CUBA_STARTER_REFERENCE']='1'
        env.update(CUBA_STARTER_CATALOG=str(release.lock_path),CUBA_STARTER_SCALE='3',
            CUBA_STARTER_POSE=pose,CUBA_STARTER_LOD=lod,
            CUBA_STARTER_PITCH=str(1.55 if name=='top' else .0855053),
            CUBA_STARTER_YAW=str(yaw),CUBA_STARTER_THUMBNAILS=str(output/name))
        subprocess.run(['cargo','test','--manifest-path',str(ENGINE/'Cargo.toml'),
                        'capture_starters','--','--ignored'],check=True,env=env,cwd=ENGINE)
    output.mkdir(parents=True,exist_ok=True)
    metadata={'preset':lock['presets'][0], 'capture':'production CharacterRenderer; no Studio UI',
        'referenceCamera':{'yaw':.26,'pitch':.12,'centerY':1.62,'orthoHalfHeight':2.28,'size':[768,960]},
        'cases':[{'name':n,'yaw':y,'pose':p,'lod':l} for n,y,p,l in cases],
        'sources':{str(p.relative_to(ROOT)):hashlib.sha256(p.read_bytes()).hexdigest()
                   for p in sorted((ROOT/'artwork').glob('*.py'))},
        'captureSources':{str(p.relative_to(ENGINE)):hashlib.sha256(p.read_bytes()).hexdigest()
            for p in [ENGINE/'src/renderer/character_starter_review.rs',ENGINE/'src/renderer/character_reference_stage.rs']},
        'packs':{a['definition']['id']:a['artifact'] for a in assets}}
    (output/'capture.json').write_text(json.dumps(metadata,indent=2)+'\n')


if __name__=='__main__': main()
