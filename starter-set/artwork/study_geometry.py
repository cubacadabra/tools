"""Geometry for the single mockup study. Y-up engine units; Blender is Z-up.

These are garment panels, profiled volumes and continuous strand/cord sweeps.
The study intentionally has its own assets so it cannot change other starters.
"""
import math
import bpy
from mathutils import Vector
from .geometry import Mesh, bezier, sweep, lock, power, profile as taper_profile, ellipsoid, TAU


def xyz(p):
    return (p[0], -p[2], p[1])


def object_from_mesh(name, mesh, material, joint, bucket):
    mesh.clean()
    data = bpy.data.meshes.new(name)
    data.from_pydata([xyz(p) for p in mesh.vertices], [], mesh.faces)
    data.update()
    obj = bpy.data.objects.new(name, data)
    bpy.context.collection.objects.link(obj)
    obj.data.materials.append(material)
    group = obj.vertex_groups.new(name=str(joint))
    group.add(list(range(len(mesh.vertices))), 1.0, 'REPLACE')
    for poly in data.polygons:
        poly.use_smooth = True
    bucket.append(obj)
    return obj


def volume(name, center, size, radius, material, joint, bucket, subdivisions=2):
    """Rounded box with flat central planes and an actual bevel radius."""
    bpy.ops.mesh.primitive_cube_add(size=1, location=xyz(center))
    obj = bpy.context.object
    obj.name = name
    obj.dimensions = (size[0], size[2], size[1])
    bpy.ops.object.transform_apply(location=True, rotation=True, scale=True)
    mod = obj.modifiers.new('Soft manufactured edges', 'BEVEL')
    mod.width = radius
    mod.segments = 5
    bpy.ops.object.modifier_apply(modifier=mod.name)
    if subdivisions:
        mod = obj.modifiers.new('Sculpt surface', 'SUBSURF')
        mod.levels = subdivisions
        bpy.ops.object.modifier_apply(modifier=mod.name)
    obj.data.materials.append(material)
    group = obj.vertex_groups.new(name=str(joint))
    group.add(list(range(len(obj.data.vertices))), 1.0, 'REPLACE')
    for p in obj.data.polygons:
        p.use_smooth = True
    bucket.append(obj)
    return obj


def tube(name, points, radius, material, joint, bucket, depth=None):
    mesh = Mesh()
    sweep(mesh, lambda t: bezier(points, t), radius, depth or radius, 3)
    return object_from_mesh(name, mesh, material, joint, bucket)


def shell(name, profile, center, exponent, material, joint, bucket, folds=0., rib=0):
    """Smooth superellipse loft through measured garment sections, bottom up."""
    mesh = Mesh()
    rows, radial = 44, 96 if rib else 64
    def surface(u, v):
        angle = u*TAU
        y = profile[0][0] + v*(profile[-1][0]-profile[0][0])
        # Piecewise cubic interpolation keeps hems and shoulder transitions.
        index = next((i for i in range(1, len(profile)) if y <= profile[i][0]), len(profile)-1)
        a, b = profile[index-1:index+1]
        t = (y-a[0])/(b[0]-a[0])
        def radius(axis):
            def slope(i):
                low=max(0,i-1); high=min(len(profile)-1,i+1)
                return (profile[high][axis]-profile[low][axis])/(profile[high][0]-profile[low][0])
            span=b[0]-a[0]
            return ((2*t**3-3*t*t+1)*a[axis]+(t**3-2*t*t+t)*span*slope(index-1)
                    +(-2*t**3+3*t*t)*b[axis]+(t**3-t*t)*span*slope(index))
        rx,rz=radius(1),radius(2)
        wrinkle = folds*(math.sin(angle*5+v*11)*math.exp(-((v-.17)/.15)**2)
                         + .6*math.sin(angle*7-v*15)*math.exp(-((v-.69)/.17)**2))
        ribbing = (1+.008*math.cos(angle*32)) if rib else 1.
        x = (rx+wrinkle)*power(math.sin(angle), exponent)*ribbing
        z = -(rz+wrinkle*.7)*power(math.cos(angle), exponent)*ribbing
        return (center[0]+x, center[1]+y, center[2]+z)
    rings = mesh.grid(surface, radial, rows, reverse=True)
    mesh.cap(rings[0])
    mesh.cap(rings[-1], reverse=True)
    return object_from_mesh(name, mesh, material, joint, bucket)


def body(m):
    objects = []
    volume('Soft square head', (0, 2.70, 0), (1.01, .91, .83), .20, m['skin'], 2, objects, 3)
    for side in (-1,1):
        eye=Mesh(); ellipsoid(eye,(side*.17,2.775,-.421),(.05,.115,.018),3)
        object_from_mesh('Graphic oval eye',eye,m['ink'],2,objects)
    tube('Sculpted smile',((-.16,2.613,-.423),(-.083,2.453,-.432),
                            (.083,2.453,-.432),(.16,2.613,-.423)),.012,m['ink'],2,objects)
    # Reduce the head envelope together with its face, keeping the fitted hair
    # clearance below. Do not shrink the eye/mouth depths independently.
    for obj in objects:
        for vertex in obj.data.vertices:
            vertex.co.x *= .95
            vertex.co.y *= .97
    volume('Neck', (0, 2.23, 0), (.33, .35, .32), .10, m['skin'], 1, objects)
    volume('Covered torso', (0, 1.60, 0), (.72, .80, .44), .15, m['skin'], 1, objects)
    for side, upper, lower, hand, thigh, calf in [(-1,3,4,5,9,10),(1,6,7,8,12,13)]:
        volume('Upper arm', (side*.60, 1.52, 0), (.31, .58, .35), .12, m['skin'], upper, objects)
        volume('Forearm', (side*.63, 1.27, -.01), (.26, .43, .29), .105, m['skin'], lower, objects)
        # C-shaped toy hands with a real opening and a separate rounded thumb.
        points = ((side*.60,.64,-.055),(side*.90,.54,-.04),
                  (side*.92,.85,-.035),(side*.67,.84,-.04))
        fit_hand=lambda p:(side*.66+(p[0]-side*.74)*.80,.98+(p[1]-.72)*.85,p[2])
        tube('Curved palm',tuple(fit_hand(p) for p in points),.074,m['skin'],hand,objects,.08)
        tube('Thumb',tuple(fit_hand(p) for p in ((side*.67,.84,-.04),(side*.57,.82,-.08),
                       (side*.59,.72,-.12),(side*.615,.685,-.115))),.055,m['skin'],hand,objects)
        volume('Thigh', (side*.235,.94,0), (.36,.48,.37), .105, m['skin'], thigh, objects)
        volume('Calf', (side*.235,.40,-.01), (.33,.70,.34), .09, m['skin'], calf, objects)
    return objects


def hoodie(m):
    objects=[]
    torso=shell('Fleece body', [(-.55,.443,.297),(-.43,.469,.318),(-.18,.489,.335),
                         (.17,.512,.318),(.35,.431,.254),(.49,.235,.180)],
          (0,1.72,0), .48, m['fleece'], 1, objects, folds=.012)
    shell('Rib knit waistband', [(-.065,.440,.296),(-.045,.454,.313),(.038,.475,.332),(.060,.470,.327)],
          (0,1.185,0), .48, m['rib'], 1, objects, rib=1)
    for side, joint in [(-1,3),(1,6)]:
        sleeve = shell('Relaxed sleeve', [(-.56,.153,.171),(-.46,.169,.188),(-.20,.190,.213),
                                        (.20,.202,.215),(.31,.152,.180),(.38,.09,.11),(.44,.008,.008)],
                       (side*.55,1.62,0), .75, m['fleece'], joint, objects, folds=.008)
        # A gently flared relaxed arm, no spherical shoulder pad.
        for vertex in sleeve.data.vertices:
            vertex.co.x += side*(.095*max(0,(1.90-vertex.co.z)/.9)-.20*max(0,(vertex.co.z-1.82)/.26))
        shell('Rib knit cuff',[(-.048,.153,.171),(-.03,.167,.187),(.04,.169,.188),(.06,.155,.175)],
              (side*.638,1.075,0), .7, m['rib'], joint, objects, rib=1)
    # A hollow fabric hood draped behind the neck, with a rolled open edge.
    mesh=Mesh()
    def hood_surface(u,v):
        a=u*TAU
        rx=.22+.10*math.sin(v*math.pi*.80)
        rz=.18+.09*math.sin(v*math.pi*.85)
        return (rx*math.sin(a), 2.18-.29*v+.055*math.cos(a), .065-rz*math.cos(a)+.11*v)
    rings=mesh.grid(hood_surface,72,30)
    mesh.cap(rings[-1])
    object_from_mesh('Lowered hollow hood',mesh,m['fleece'],1,objects)
    from .geometry import loop
    rim=Mesh()
    loop(rim, lambda t:(.235*math.sin(t*TAU),2.188+.052*math.cos(t*TAU),.105-.20*math.cos(t*TAU)),
         .036,3,outward=(0,1,0))
    object_from_mesh('Rolled hood opening',rim,m['rib'],1,objects)
    # Fit a subdivided cloth panel to the actual torso surface. The previous
    # six-point plane hovered above the curved body and left a dark jagged gap.
    def pocket_point(x,y,offset=.011):
        hit,location,_,_=torso.ray_cast(Vector((x,2.,y)),Vector((0,-1,0)))
        assert hit, ('pocket misses torso',x,y)
        return (x,y,-location.y-offset)
    mesh=Mesh()
    layers=[]; rows=18; columns=28
    for back in (False,True):
        grid=[]
        for j in range(rows+1):
            v=j/rows; y=1.275+.275*v
            width=.323+.025*min(1.,v/.20)-.115*max(0.,(v-.20)/.80)
            grid.append([mesh.vertex(pocket_point((2*i/columns-1)*width,y,
                          -.003 if back else .008+.006*math.sin(math.pi*v)*math.sin(math.pi*i/columns)))
                         for i in range(columns+1)])
        for j in range(rows):
            for i in range(columns):
                a,b,c,d=grid[j][i],grid[j][i+1],grid[j+1][i],grid[j+1][i+1]
                mesh.face(a,c,b) if back else mesh.face(a,b,c)
                mesh.face(b,c,d) if back else mesh.face(b,d,c)
        border=grid[0]+[r[-1] for r in grid[1:]]+list(reversed(grid[-1][:-1]))+[r[0] for r in reversed(grid[1:-1])]
        layers.append(border)
    for i,a in enumerate(layers[0]):
        j=(i+1)%len(layers[0]); b=layers[0][j]; c=layers[1][i]; d=layers[1][j]
        mesh.face(a,c,b); mesh.face(b,c,d)
    object_from_mesh('Fitted kangaroo pocket',mesh,m['fleece'],1,objects)
    for side in (-1,1):
        tube('Pocket hand opening',tuple(pocket_point(side*x,y,.013) for x,y in
             ((.232,1.539),(.262,1.480),(.308,1.388),(.343,1.334))),.006,m['seam'],1,objects)
        tube('Double pocket topstitch',tuple(pocket_point(side*x,y,.017) for x,y in
             ((.219,1.535),(.249,1.476),(.295,1.386),(.329,1.333))),.003,m['stitch'],1,objects)
        tube('Cotton drawcord',((side*.135,2.195,-.129),(side*.158,2.08,-.307),
                               (side*.15,1.94,-.378),(side*.15,1.775,-.376)), .012,m['cotton'],1,objects)
        volume('Drawcord aglet',(side*.15,1.765,-.377),(.031,.068,.027),.008,m['cotton'],1,objects,1)
    tube('Pocket hem stitch',tuple(pocket_point(x,y,.012) for x,y in
         ((-.307,1.286),(-.1,1.282),(.1,1.282),(.307,1.286))),.003,m['stitch'],1,objects)
    return objects


def shorts(m):
    objects=[]
    for side,joint in [(-1,9),(1,12)]:
        center=(side*.235,.97,0)
        shell('Denim shorts leg',[(-.40,.213,.221),(-.32,.222,.23),(-.10,.235,.247),(.20,.225,.23)],
              center,.44,m['denim'],joint,objects,folds=.007)
        shell('Turned denim hem',[(-.025,.215,.223),(-.010,.225,.234),(.032,.225,.234),(.046,.216,.224)],
              (side*.235,.593,0),.44,m['denim-hem'],joint,objects)
        for x in (side*.438,side*.033):
            tube('Leg seam',((x,1.12,-.18),(x,.99,-.22),(x,.74,-.22),(x,.62,-.19)),.0035,m['thread'],joint,objects)
        tube('Front pocket seam',((side*.285,1.13,-.242),(side*.32,1.07,-.25),
                                  (side*.405,1.01,-.234),(side*.445,.97,-.183)),.004,m['thread'],joint,objects)
        # Rear patch pockets are actual shallow shaped panels.
        volume('Back denim pocket',(side*.255,.975,.242),(.235,.215,.027),.028,m['denim-hem'],joint,objects,1)
    shell('Denim waistband',[(-.035,.435,.22),(-.02,.461,.238),(.035,.464,.24),(.05,.44,.223)],
          (0,1.16,0),.43,m['denim-hem'],0,objects)
    # Keep the fly on the inner front of the left panel, above the leg split.
    tube('Fly seam',((-.065,1.16,-.249),(-.055,1.09,-.256),(-.065,1.035,-.258),(-.095,1.02,-.253)),.004,m['thread'],9,objects)
    return objects


def sneakers(m):
    objects=[]
    for side,joint,calf in [(-1,11,10),(1,14,13)]:
        x=side*.235
        volume('Rubber cupsole',(x,.086,-.15),(.46,.115,.73),.041,m['rubber'],joint,objects,2)
        # A continuous low toe rolls up to the ankle. A floating toe-cap box
        # makes the front resemble stacked cushions, so the panel is stitched
        # into this single leather surface instead.
        upper=Mesh()
        def leather(u,v):
            a=u*TAU
            rx=.211*(1-.37*v**2)
            rz=.345*(1-.51*v**2)
            z=-.14+.15*v-rz*power(math.cos(a),.55)
            y=.13+.31*v-.13*max(0.,math.cos(a))*v
            return (x+rx*power(math.sin(a),.60),y,z)
        rings=upper.grid(leather,64,32,reverse=True)
        upper.cap(rings[0]); upper.cap(rings[-1],reverse=True)
        upper_obj=object_from_mesh('Shaped leather upper',upper,m['leather'],joint,objects)
        volume('Padded tongue',(x,.365,-.02),(.225,.115,.225),.040,m['leather'],joint,objects,2)
        shell('Ribbed white sock',[(-.09,.177,.182),(.055,.182,.187),(.08,.178,.183)],
              (x,.35,-.010),.50,m['sock'],calf,objects,rib=1)
        def lace_point(px,pz,offset=.013):
            hit,location,_,_=upper_obj.ray_cast(Vector((px,-pz,1.)),Vector((0,0,-1)))
            assert hit, ('lace misses leather',px,pz)
            return (px,location.z+offset,pz)
        for k in range(4):
            z=-.380+k*.054
            tube('Flat woven laces',tuple(lace_point(px,pz) for px,pz in
                 ((x-.122,z),(x-.061,z-.009),(x+.061,z+.009),(x+.122,z))),
                 .010,m['cotton'],joint,objects,.007)
            for sign in (-1,1):
                volume('Reinforced eyelet',lace_point(x+sign*.129,z,.004),
                       (.032,.015,.030),.006,m['rubber'],joint,objects,1)
        for sign in (-1,1):
            tube('Leather panel stitching',((x+sign*.171,.18,-.34),(x+sign*.212,.19,-.18),
                                             (x+sign*.207,.28,.04),(x+sign*.135,.315,.10)),.0035,m['shoe-thread'],joint,objects)
        for k in range(15):
            z=-.43+k*.036
            for sign in (-1,1):
                volume('Outsole siping',(x+sign*.217,.086,z),(.006,.053,.009),.002,m['shoe-thread'],joint,objects,0)
        for k in range(7):
            volume('Toe outsole siping',(x-.126+k*.042,.086,-.508),
                   (.007,.047,.006),.002,m['shoe-thread'],joint,objects,0)
    return objects


def hair(m):
    objects=[]
    cap=Mesh()
    # The base is a beveled box, not an ellipsoid. A spherical crown shrinks
    # too quickly near the top and exposes the head's flat upper corners.
    from .geometry import smooth
    def foundation(u,v):
        angle=u*TAU
        facing=smooth((math.cos(angle)-.15)/.7)
        bottom=-.18*(1-facing)+.415*facing+.13*abs(math.sin(angle))**8
        y=.505-(.505-bottom)*v
        roundness=math.sqrt(max(0.,1-(max(0.,y-.315)/.190)**2))
        rx=.302+.190*roundness
        rz=.232+.190*roundness
        return (rx*power(math.sin(angle),.55),y+2.70,-rz*power(math.cos(angle),.55))
    rings=cap.grid(foundation,72,28)
    cap.cap(rings[0],reverse=True)
    cap.cap(rings[-1])
    object_from_mesh('Fitted swept foundation',cap,m['hair'],2,objects)
    # The crown parts off-centre. Locks flow away from that part in two fans;
    # each fan layers pointed tips down to the temple instead of crossing as
    # broad capsules. Longitudinal grooves are sculpted into each ribbon.
    paths=[
        ((.075,.48,-.08),(-.06,.70,-.24),(-.43,.38,-.46),(-.48,.035,-.35),.135,.062),
        ((.06,.51,.04),(-.20,.70,-.03),(-.52,.33,-.26),(-.525,-.055,-.15),.115,.058),
        ((.05,.50,-.14),(-.035,.58,-.40),(-.22,.22,-.515),(-.34,.095,-.425),.125,.056),
        ((.075,.46,-.24),(.025,.49,-.46),(-.08,.23,-.51),(-.18,.16,-.446),.10,.048),
        ((.13,.49,-.1),(.34,.64,-.27),(.46,.34,-.455),(.445,.035,-.335),.13,.065),
        ((.12,.52,.04),(.42,.64,.02),(.54,.29,-.23),(.53,-.06,-.105),.12,.06),
        ((.15,.46,-.21),(.28,.49,-.41),(.345,.24,-.485),(.35,.075,-.403),.108,.053),
    ]
    # Side ribbons must be tangential to the head, not front-facing ribbons
    # cutting through the cap. Their bowed paths clear the fitted foundation.
    for side in (-1,1):
        for k in range(5):
            z=-.075+k*.086
            paths.append(((.12+side*.025,.55,z*.55),(side*.64,.72,z),
                          (side*.61,.06,z*.96),(side*.477,-.17+.022*(k%3),z*.84),
                          .078+.006*(k%3),.046))
    for k in range(9):
        x=-.40+k*.10
        paths.append(((x*.55,.55,.16),(x*.92,.65,.54),
                      (x,.02,.49),(x*.94,-.22+.018*(k%3),.417),.072+.004*(k%2),.039))
    for index,(*p,width,depth) in enumerate(paths):
        mesh=Mesh()
        points=[(x*.96,y*.96+2.70,z*.97) for x,y,z in p]
        if index<7:
            width *= .84
            depth *= .87
        shape=(.24,.80,1.,.71,.25,.004)
        if index<7:
            guide=(0,0,-1)
        elif index<17:
            side=-1 if index<12 else 1
            guide=(side, .12, p[-1][2]*1.4)
        else:
            guide=(p[-1][0]*.5,0,1)
        sweep(mesh,lambda t:bezier(points,t),lambda t:width*taper_profile(shape,t),
              lambda t:depth*taper_profile(shape,t),3,
              outward=guide,grooves=.08)
        object_from_mesh(f'Sculpted swept lock {index:02}',mesh,m['hair'],2,objects)
        if index<7:
            strand=Mesh()
            # A raised, finer ridge follows the full clump, tucked into its
            # volume. It adds a second highlight without outlining each hair.
            smaller=[(px+.016,py+.008,pz-.019) for px,py,pz in points]
            lock(strand,smaller,width*.30,depth*.23,3)
            object_from_mesh(f'Fine swept ridge {index:02}',strand,m['hair'],2,objects)
    return objects
