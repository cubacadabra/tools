"""Authored hair silhouettes and cubic lock curves, in head-local coordinates."""
import math
from .geometry import Mesh, TAU, power, smooth, sweep, lock, loop, ellipsoid


def scalp(mesh, detail, rx=.535, rz=.46, top=.565, back=-.23, front=.24,
          bob=False, material=0):
    radial, rows = {3: (64, 24), 2: (40, 16), 1: (24, 10)}[detail]
    def surface(u, v):
        angle = u*TAU
        facing = smooth((math.cos(angle)-.15)/.7)
        bottom = back*(1-facing)+front*facing
        if not bob: bottom += .17*abs(math.sin(angle))**8
        if bob: bottom += .018*math.sin(3*angle)
        y = top-(top-bottom)*v
        # Both base heads are rounded boxes, not ellipsoids. Preserve their
        # broad forehead/temple envelope before rounding over the crown.
        radius = math.sqrt(max(0., 1-((max(y,.26)-.26)/(top-.26))**2))
        radius *= 1-(.045 if bob else .08)*smooth((-y-.12)/.32)
        return (rx*power(math.sin(angle), .55)*radius, y,
                .006-rz*power(math.cos(angle), .55)*radius)
    rings = mesh.grid(surface, radial, rows, material)
    mesh.cap(rings[-1], material)
    return surface


def floppy(mesh, detail):
    scalp(mesh, detail, back=-.15)
    # The dominant sweep crosses the crown and drops at the temple. Supporting
    # locks overlap like broad sculpted ribbons, with asymmetric pointed ends.
    for points, width, depth in [
        (((.27,.48,.015),(.11,.62,-.23),(-.31,.39,-.49),(-.425,.075,-.37)), .21,.09),
        (((.39,.40,-.08),(.38,.54,-.31),(-.12,.31,-.52),(-.22,.155,-.465)), .195,.085),
        (((.47,.26,-.13),(.48,.46,-.36),(.23,.31,-.49),(.10,.245,-.475)), .15,.072),
        (((-.33,.37,.02),(-.58,.32,-.08),(-.54,.00,-.26),(-.485,-.10,-.18)), .13,.063),
        (((.37,.34,.09),(.54,.33,.01),(.55,.08,-.08),(.505,-.055,-.12)), .12,.06),
    ]:
        lock(mesh, points, width, depth, detail)



def buzz(mesh, detail):
    # A short cut follows the actual broad-jaw head profile, not a smaller
    # version of a voluminous hairstyle. The source GLBs share this envelope.
    from pathlib import Path
    import sys
    sys.path.insert(0,str(Path(__file__).resolve().parents[3]/"studio/tools"))
    import generate_person_asset as person
    radial,rows={3:(64,24),2:(40,16),1:(24,10)}[detail]
    def surface(u,v):
        angle=u*TAU
        front=smooth((math.cos(angle)-.15)/.7)
        bottom=-.17+.40*front+.14*abs(math.sin(angle))**8
        y=.488-(.488-bottom)*v
        t=max(0.,min(1.,(y-.018+.47)/.94))
        rx,rz=person.profile(person.PERSON_VARIANTS["person-02"]["head_profile"],t)
        pole=min(1.,v*rows)
        return ((rx*1.02+.012)*power(math.sin(angle),.57)*pole,y,
                -(rz*.85+.012)*power(math.cos(angle),.57)*pole)
    rings=mesh.grid(surface,radial,rows)
    mesh.cap(rings[-1])


def bob(mesh, detail):
    scalp(mesh, detail, rx=.559, rz=.482, top=.59, back=-.47, front=.245, bob=True)
    for points, width, depth in [
        (((.28,.47,-.23),(.22,.63,-.38),(-.18,.28,-.50),(-.35,.20,-.44)), .225,.085),
        (((.41,.34,-.22),(.41,.49,-.42),(.03,.26,-.49),(-.075,.215,-.474)), .18,.07),
        (((.45,.28,-.18),(.54,.19,-.28),(.53,-.25,-.26),(.445,-.46,-.19)), .135,.07),
        (((-.43,.29,-.15),(-.56,.18,-.23),(-.54,-.24,-.26),(-.455,-.45,-.18)), .14,.07),
    ]:
        lock(mesh, points, width, depth, detail)


def pulled_back(mesh, detail):
    scalp(mesh, detail, top=.545, front=.245)


def braids(mesh, detail):
    pulled_back(mesh, detail)
    for side in (-1, 1):
        for strand in range(3):
            def curve(t, strand=strand, side=side):
                angle = TAU*(t*3.2+strand/3)
                taper = 1-.40*t
                return (side*(.482+.055*math.sin(t*math.pi)+.045*math.cos(angle)*taper),
                        .125-.74*t, .105+.044*math.sin(angle)*taper)
            sweep(mesh, curve, lambda t: .042*(1-.48*t), lambda t: .042*(1-.48*t),
                  detail, outward=(0.,0.,-1.))
        loop(mesh, lambda t, side=side: (side*.493+.056*math.sin(t*TAU), -.51,
                                        .105+.051*math.cos(t*TAU)), .018, detail, 1, outward=(0.,1.,0.))
        lock(mesh, ((side*.494,-.54,.11),(side*.51,-.60,.105),(side*.53,-.67,.10),
                    (side*.515,-.705,.11)), .052,.042, detail)


def buns(mesh, detail):
    pulled_back(mesh, detail)
    for side in (-1, 1):
        center = (side*.36,.49,.12)
        ellipsoid(mesh, center, (.36,.29,.34), detail, twist=-side*.25)
        # A rolled spiral gives each bun a hair construction and avoids the
        # perfectly spherical "bear ear" silhouette of the original fixture.
        def curve(t, side=side):
            angle = t*TAU*1.9
            radius = .15*(1-.80*t)
            return (side*.36+radius*math.cos(angle), .503+radius*.68*math.sin(angle),
                    -.035-.045*math.sin(t*math.pi))
        sweep(mesh, curve, lambda t: .029*(1-.65*t), lambda t: .022*(1-.65*t), detail)


BUILDERS = {"floppy": floppy,
            "buzz": buzz, "bob": bob, "braids": braids, "buns": buns}


def build(slug, detail):
    mesh = Mesh()
    BUILDERS[slug](mesh, detail)
    return mesh.clean()
