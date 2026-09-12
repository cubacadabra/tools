"""Authored eyes, brows, and mouths for the native v2 face assets.

The old face implementation was a procedural renderer feature. Native v2
characters deliberately do not use that feature, so each expression is now a
small rigid MorphPack attached to the head joint. The construction keeps the
same proportions as the former person face while making the visible pixels
real authored geometry.
"""
import math

from .geometry import Mesh, bezier, ellipsoid, sweep
from . import study_geometry as geometry


EXPRESSIONS = {
    "neutral": (1.00, 0.00, 0.00, 0.00, 0.00, 0.00, 0.00),
    "happy": (.92, 0.00, 0.00, 0.02, .00, .80, .18),
    "surprised": (1.18, 0.00, 0.00, 0.00, .00, .00, .65),
    "determined": (.82, 0.00, 0.00, -.01, .02, -.15, .00),
    "sad": (.88, 0.00, 0.00, -.03, .00, -.75, .04),
    "laughing": (.48, 0.00, 0.00, 0.00, .00, .80, .80),
    "smile": (.95, 0.00, 0.00, 0.00, .00, .55, .05),
    "grin": (.90, 0.00, 0.00, 0.00, .00, .90, .34),
    "curious": (1.08, .03, .025, .03, .08, .12, .08),
    "amazed": (1.20, 0.00, 0.00, .02, .00, .00, .82),
    "angry": (.78, 0.00, 0.00, 0.00, .00, -.20, .10),
    "crying": (.45, 0.00, 0.00, -.04, .00, -.65, .20),
    "worried": (.92, 0.00, 0.00, -.02, .00, -.30, .08),
    "embarrassed": (.62, 0.00, 0.00, -.06, .10, .20, .00),
    "sleepy": (.40, 0.00, 0.00, -.08, .00, .00, .00),
    "squinting": (.38, 0.00, 0.00, 0.00, .00, .10, .00),
    "wink": (.58, .45, .02, 0.00, -.04, .45, .00),
    "smirk": (.85, 0.00, 0.00, 0.00, .05, .42, .03),
    "confused": (1.00, .10, .045, .02, -.07, -.08, .12),
    "excited": (1.18, 0.00, 0.00, .04, .00, .65, .48),
    "unimpressed": (.68, 0.00, 0.00, -.01, .00, -.04, .00),
}


def _mesh_object(name, mesh, material, objects):
    return geometry.object_from_mesh(name, mesh, material, 2, objects)


def _eye(mesh, side, opening, look_x, look_y, iris, pupil, highlight, material):
    x = side * (.225 + look_x)
    y = 2.80 + look_y
    # Keep the graphic plane in front of both person bases. The head joint
    # attachment subtracts its bind origin during GLB export.
    z = -.426
    ellipsoid(mesh, (x, y, z), (.22, .22 * max(opening, .16), .026), 3, material)
    ellipsoid(iris, (x + look_x * .22, y + look_y * .22, z - .018),
              (.072, .105 * max(opening, .40), .016), 3, material)
    ellipsoid(pupil, (x + look_x * .24, y + look_y * .24, z - .028),
              (.034, .060 * max(opening, .42), .012), 3, material)
    ellipsoid(highlight, (x - .018, y + .035, z - .037), (.018, .026, .008), 2, material)


def _brow(mesh, side, tilt, asymmetry, material):
    x = side * .225
    y = 2.97 + side * asymmetry
    width = .21
    angle = side * tilt
    points = []
    for at in (0.0, .33, .67, 1.0):
        local_x = -width * .5 + width * at
        local_y = .018 * math.sin(at * math.pi) + math.tan(angle) * local_x
        points.append((x + side * local_x, y + local_y, -.435))
    sweep(mesh, lambda t: bezier(points, t), .018, .012, 3, material)


def _mouth(mesh, opening, curve, dark, tongue, teeth, material):
    width = .34 + opening * .06
    center_y = 2.52 + curve * .025
    # A curved ink stroke keeps closed expressions readable. Open expressions
    # get a shallow volume behind it, plus restrained tooth/tongue cues.
    points = ((-width * .5, center_y + curve * .018, -.432),
              (-width * .24, center_y - curve * .065, -.432),
              (width * .24, center_y - curve * .065, -.432),
              (width * .5, center_y + curve * .018, -.432))
    sweep(mesh, lambda t: bezier(points, t), .014, .012, 3, material)
    if opening <= .12:
        return
    ellipsoid(dark, (0.0, center_y - curve * .01, -.438),
              (width, .045 + opening * .11, .018), 3, material)
    if opening > .38:
        ellipsoid(teeth, (0.0, center_y + .018, -.451),
                  (width * .72, .022, .010), 2, material)
    if opening > .30:
        ellipsoid(tongue, (0.0, center_y - .026, -.452),
                  (width * .52, .024, .010), 2, material)


def build(slug, detail, materials):
    """Return Blender objects for one expression at the requested detail."""
    if slug not in EXPRESSIONS:
        raise ValueError(f"unknown authored face expression: {slug}")
    opening, eye_asymmetry, brow_asymmetry, look_y, look_x, curve, mouth_opening = EXPRESSIONS[slug]
    objects = []
    eye_material = materials["sclera"]
    ink_material = materials["ink"]
    iris_material = materials["iris"]
    pupil_material = materials["pupil"]
    highlight_material = materials["highlight"]
    mouth_material = materials["mouth"]
    tongue_material = materials["tongue"]
    teeth_material = materials["teeth"]
    for side in (-1, 1):
        sclera = Mesh(); iris = Mesh(); pupil = Mesh(); highlight = Mesh(); brow = Mesh()
        side_opening = max(.05, opening + side * eye_asymmetry)
        _eye(sclera, side, side_opening, look_x, look_y, iris, pupil, highlight,
             eye_material)
        _brow(brow, side, curve * .18 if slug not in ("angry", "determined") else curve * .18,
              brow_asymmetry, ink_material)
        _mesh_object("Authored sclera", sclera, eye_material, objects)
        _mesh_object("Authored iris", iris, iris_material, objects)
        _mesh_object("Authored pupil", pupil, pupil_material, objects)
        _mesh_object("Authored catchlight", highlight, highlight_material, objects)
        _mesh_object("Authored brow", brow, ink_material, objects)
    mouth = Mesh(); dark = Mesh(); tongue = Mesh(); teeth = Mesh()
    _mouth(mouth, mouth_opening, curve, dark, tongue, teeth, mouth_material)
    _mesh_object("Authored mouth stroke", mouth, mouth_material, objects)
    _mesh_object("Authored mouth opening", dark, mouth_material, objects)
    _mesh_object("Authored tongue", tongue, tongue_material, objects)
    _mesh_object("Authored teeth", teeth, teeth_material, objects)
    return objects
