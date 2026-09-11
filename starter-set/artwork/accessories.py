"""Wearable construction: solid frames, padded cups, folded rims, and ear hooks."""
import math
from .geometry import Mesh, TAU, bezier, power, sweep, loop, ellipsoid, lathe


def glasses(mesh, detail, square=False):
    for side in (-1, 1):
        def outline(t, side=side):
            angle = t*TAU
            x = side*.205+.172*power(math.cos(angle), .52 if square else .91)
            y = .066+.133*power(math.sin(angle), .62 if square else 1.)
            z = -.472+.06*(abs(x)/.4)**4
            return x, y, z
        loop(mesh, outline, .023 if square else .021, detail)
        points = ((side*.385,.099,-.408),(side*.49,.114,-.32),
                  (side*.543,.12,.13),(side*.516,-.048,.165))
        sweep(mesh, lambda t, points=points: bezier(points,t), .020,.016,detail)
        ellipsoid(mesh,(side*.39,.096,-.417),(.041,.032,.035),detail,1)
    points = ((-.049,.094,-.475),(-.031,.147,-.495),(.031,.147,-.495),(.049,.094,-.475))
    sweep(mesh,lambda t: bezier(points,t),.018,.018,detail)


def headphones(mesh, detail):
    # Padded arch follows the head, with the cups at the ears. No square bar
    # floating above the crown. The front stays open for glasses and a hat.
    for material, radius, z in [(0,.047,.07),(1,.026,.051)]:
        curve = lambda t, z=z: (.598*math.cos(t*math.pi), .075+.615*math.sin(t*math.pi), z)
        sweep(mesh, curve, radius, .060 if material==0 else .052, detail, material)
    for side in (-1,1):
        ellipsoid(mesh,(side*.533,-.012,.035),(.092,.335,.30),detail,1,exponent=.48)
        ellipsoid(mesh,(side*.612,-.007,.041),(.145,.363,.31),detail,0,exponent=.44)
        ellipsoid(mesh,(side*.687,-.007,.041),(.035,.256,.215),detail,2,exponent=.48)
        points=((side*.573,.245,.07),(side*.636,.233,.07),
                (side*.63,.10,.07),(side*.64,.087,.07))
        sweep(mesh,lambda t, points=points:bezier(points,t),.022,.025,detail,2)


def hearing_aids(mesh, detail):
    for side in (-1,1):
        ellipsoid(mesh,(side*.523,-.012,.077),(.075,.179,.107),detail,0,twist=side*.15)
        points=((side*.522,.06,.095),(side*.559,.18,.02),
                (side*.559,.105,-.09),(side*.517,.012,-.104))
        sweep(mesh,lambda t, points=points:bezier(points,t),.012,.012,detail,1)
        ellipsoid(mesh,(side*.52,.011,-.102),(.036,.045,.04),detail,1)


def top_hat(mesh, detail):
    # Softly rolled oval brim, tapering crown, separate cloth ribbon and a
    # quiet brass buckle. Lower and less saturated than the old test cylinder.
    lathe(mesh,[(.59,.595,.454),(.583,.626,.479),(.556,.638,.486),
                (.528,.626,.475),(.524,.585,.438)],detail)
    lathe(mesh,[(1.18,.343,.303),(1.20,.355,.317),(1.183,.381,.332),
                (1.13,.385,.337),(.78,.415,.352),(.59,.443,.37),
                (.563,.428,.36)],detail)
    lathe(mesh,[(.772,.42,.357),(.754,.425,.363),(.614,.447,.378),
                (.60,.444,.375)],detail,1)
    loop(mesh,lambda t:(.064*power(math.cos(t*TAU),.45),
                       .683+.052*power(math.sin(t*TAU),.5),-.382),.012,detail,2)


def build(slug, detail):
    mesh=Mesh()
    if slug in ("round-glasses","square-glasses"):
        glasses(mesh,detail,slug=="square-glasses")
    else:
        {"headphones":headphones,"hearing-aids":hearing_aids,"test-top-hat":top_hat}[slug](mesh,detail)
    return mesh.clean()
