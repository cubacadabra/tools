#!/usr/bin/env python3
"""Capture all recipes through the shared renderer with fixed review settings.

Review evidence is immutable per --label. Pillow is used only for contact
sheets; individual PNGs are unmodified GPU captures. No assets are published.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess

from PIL import Image, ImageDraw

ROOT = Path(__file__).resolve().parent
CASES = [('front',0,0,'rest','near'), ('hero',.26,.12,'rest','near'),
         ('back',3.14159,0,'rest','near'), ('left',1.5708,0,'rest','near'),
         ('right',-1.5708,0,'rest','near'), ('top',0,1.55,'rest','near'),
         ('mid',.26,.12,'rest','mid'), ('far',.26,.12,'rest','far'),
         ('walk',.70,.12,'walk','near'), ('bend',.70,.12,'bend','near'),
         ('jump',.70,.12,'jump','near')]


def sheet(folder):
    paths = sorted(folder.glob('person-*.png'))
    assert len(paths) == 24, folder
    canvas = Image.new('RGB', (6*256,4*342), '#171b24')
    draw = ImageDraw.Draw(canvas)
    for i,path in enumerate(paths):
        x,y = i%6*256,i//6*342
        im = Image.open(path).convert('RGB'); im.thumbnail((256,320))
        canvas.paste(im,(x,y)); draw.text((x+8,y+322),path.stem,fill='white')
    canvas.save(folder.parent / (folder.name+'-sheet.jpg'),quality=94)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--label',required=True)
    parser.add_argument('--skip-build',action='store_true')
    args = parser.parse_args()
    assert Path(args.label).name == args.label
    output = ROOT / 'review' / args.label
    output.mkdir(parents=True, exist_ok=False)
    env = {k:v for k,v in os.environ.items() if not k.startswith('CUBA_STARTER_')}
    if not args.skip_build:
        subprocess.run(['python3','-m','cubacadabra','morph','build'],cwd=ROOT.parent,
                       env={**env,'PYTHONPATH':'src'},check=True)
    catalog = ROOT.parent / '.cubacadabra/generated/morphs/catalog.lock.json'
    env.update(CUBA_STARTER_CATALOG=str(catalog),CUBA_STARTER_SCALE='2')
    report = {'catalogSha256':hashlib.sha256(catalog.read_bytes()).hexdigest(),
              'resolution':[512,640], 'cases':[], 'sourceHashes':{}}
    for path in sorted((ROOT/'source/morphs').glob('*/*/*.glb')):
        report['sourceHashes'][str(path.relative_to(ROOT))] = hashlib.sha256(path.read_bytes()).hexdigest()
    for name,yaw,pitch,pose,lod in CASES:
        capture_env = {**env,'CUBA_STARTER_THUMBNAILS':str(output/name),
                       'CUBA_STARTER_YAW':str(yaw),'CUBA_STARTER_PITCH':str(pitch),
                       'CUBA_STARTER_POSE':pose,'CUBA_STARTER_LOD':lod}
        subprocess.run(['cargo','test','--manifest-path',str(ROOT.parents[1]/'rust/Cargo.toml'),
                        'capture_starters','--','--ignored'],env=capture_env,check=True)
        sheet(output/name)
        report['cases'].append(dict(name=name,yaw=yaw,pitch=pitch,pose=pose,lod=lod,count=24))
        (output/'capture.json').write_text(json.dumps(report,indent=2)+'\n')
    print('Captured 264 shared-renderer images:',output)


if __name__ == '__main__':
    main()
