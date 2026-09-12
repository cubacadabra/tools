"""Small, deterministic surface modelling tools. Cubacadabra coordinates: Y up.

Surfaces share their seam vertices so the runtime's generated normals remain
smooth. Curves are modelled as tapered sweeps, not chains of intersecting balls.
"""
import math

TAU = math.tau


def add(a, b): return tuple(x + y for x, y in zip(a, b))
def sub(a, b): return tuple(x - y for x, y in zip(a, b))
def mul(a, s): return tuple(x * s for x in a)
def dot(a, b): return sum(x * y for x, y in zip(a, b))
def cross(a, b): return (a[1]*b[2]-a[2]*b[1], a[2]*b[0]-a[0]*b[2], a[0]*b[1]-a[1]*b[0])
def unit(a): return mul(a, 1 / max(math.sqrt(dot(a, a)), 1e-12))
def mix(a, b, t): return add(mul(a, 1-t), mul(b, t))
def smooth(t): return max(0., min(1., t))**2 * (3 - 2 * max(0., min(1., t)))
def power(x, exponent): return math.copysign(abs(x)**exponent, x)


def bezier(points, t):
    a, b, c, d = points
    u = 1-t
    return add(add(mul(a, u**3), mul(b, 3*u*u*t)), add(mul(c, 3*u*t*t), mul(d, t**3)))


def profile(values, t):
    at = t * (len(values)-1)
    i = min(int(at), len(values)-2)
    f = at-i
    a, b, c, d = [values[max(0, min(len(values)-1, j))] for j in (i-1, i, i+1, i+2)]
    return max(0.001, .5*((2*b)+(-a+c)*f+(2*a-5*b+4*c-d)*f*f+(-a+3*b-3*c+d)*f**3))


class Mesh:
    def __init__(self):
        self.vertices, self.uvs, self.faces, self.materials = [], [], [], []

    def vertex(self, point, uv=(0., 0.)):
        self.vertices.append(tuple(point))
        self.uvs.append(uv)
        return len(self.vertices)-1

    def face(self, a, b, c, material=0):
        self.faces.append((a, b, c))
        self.materials.append(material)

    def grid(self, surface, radial, rows, material=0, closed=False, reverse=False):
        rings = []
        # No duplicate longitude vertices: recomputed normals have no seam.
        for row in range(rows if closed else rows+1):
            rings.append([self.vertex(surface(col/radial, row/rows), (col/radial, row/rows))
                          for col in range(radial)])
        for row in range(rows):
            near, far = rings[row], rings[(row+1) % len(rings)]
            for col in range(radial):
                nxt = (col+1) % radial
                if reverse:
                    self.face(near[col], far[col], near[nxt], material)
                    self.face(near[nxt], far[col], far[nxt], material)
                else:
                    self.face(near[col], near[nxt], far[col], material)
                    self.face(near[nxt], far[nxt], far[col], material)
        return rings

    def cap(self, ring, material=0, reverse=False):
        center = self.vertex(tuple(sum(self.vertices[i][j] for i in ring)/len(ring) for j in range(3)))
        for index, first in enumerate(ring):
            second = ring[(index+1) % len(ring)]
            self.face(center, second, first, material) if reverse else self.face(center, first, second, material)

    def clean(self):
        """Weld poles and reject degenerate triangles before export."""
        old_vertices, old_uvs = self.vertices, self.uvs
        self.vertices, self.uvs = [], []
        lookup, indices = {}, []
        for position, uv in zip(old_vertices, old_uvs):
            key = tuple(round(v, 7) for v in position)
            if key not in lookup:
                lookup[key] = self.vertex(position, uv)
            indices.append(lookup[key])
        faces, materials = self.faces, self.materials
        self.faces, self.materials = [], []
        for face, material in zip(faces, materials):
            a, b, c = [indices[i] for i in face]
            normal = cross(sub(self.vertices[b], self.vertices[a]), sub(self.vertices[c], self.vertices[a]))
            if len({a, b, c}) == 3 and dot(normal, normal) > 1e-18:
                self.face(a, b, c, material)
        return self


def sweep(mesh, curve, width, depth, detail, material=0, outward=(0., 0., -1.), twist=0., grooves=0.):
    # Near and mid are presentation meshes: their curved silhouettes remain
    # visible in the editor, portrait captures, and close gameplay cameras.
    # Far deliberately stays small for crowds and mobile play.
    rows, radial = {3: (32, 16), 2: (20, 10), 1: (8, 6)}[detail]
    def surface(u, t):
        tangent = unit(sub(curve(min(1., t+.0001)), curve(max(0., t-.0001))))
        guide = outward(t) if callable(outward) else outward
        normal = unit(sub(guide, mul(tangent, dot(guide, tangent))))
        if dot(normal, normal) < .1:
            normal = unit(cross(tangent, (1., .1, .01)))
        side = unit(cross(tangent, normal))
        angle = u*TAU + twist*t
        rib = 1. - grooves*(.5+.5*math.cos(angle*5+t*2))
        w = width(t) if callable(width) else width
        d = depth(t) if callable(depth) else depth
        return add(curve(t), add(mul(side, math.cos(angle)*w*rib), mul(normal, math.sin(angle)*d*rib)))
    rings = mesh.grid(surface, radial, rows, material, reverse=True)
    mesh.cap(rings[0], material)
    mesh.cap(rings[-1], material, reverse=True)


def lock(mesh, points, width, depth, detail, material=0, outward=(0., 0., -1.)):
    # Rounded roots, broad middle, a genuinely pointed end, no sausage caps.
    taper = (.45, .94, 1., .78, .40, .015)
    sweep(mesh, lambda t: bezier(points, t), lambda t: width*profile(taper, t),
          lambda t: depth*profile(taper, t), detail, material, outward, grooves=.045)


def loop(mesh, curve, radius, detail, material=0, outward=(0., 0., -1.)):
    rows, radial = {3: (72, 12), 2: (44, 10), 1: (24, 6)}[detail]
    def surface(u, t):
        tangent = unit(sub(curve((t+.0001)%1), curve((t-.0001)%1)))
        normal = unit(sub(outward, mul(tangent, dot(outward, tangent))))
        side = unit(cross(tangent, normal))
        return add(curve(t), add(mul(side, math.cos(u*TAU)*radius), mul(normal, math.sin(u*TAU)*radius)))
    mesh.grid(surface, radial, rows, material, closed=True, reverse=True)


def ellipsoid(mesh, center, size, detail, material=0, exponent=1., twist=0.):
    radial, rows = {3: (36, 20), 2: (24, 12), 1: (12, 6)}[detail]
    def surface(u, v):
        theta, phi = u*TAU, v*math.pi
        x = size[0]*.5*power(math.sin(phi)*math.sin(theta), exponent)
        y = size[1]*.5*power(math.cos(phi), exponent)
        z = -size[2]*.5*power(math.sin(phi)*math.cos(theta), exponent)
        return add(center, (x*math.cos(twist)-y*math.sin(twist), x*math.sin(twist)+y*math.cos(twist), z))
    mesh.grid(surface, radial, rows, material)


def lathe(mesh, rings, detail, material=0):
    radial = {3: 80, 2: 48, 1: 24}[detail]
    # Rings are ordered top-to-bottom for outward triangle winding.
    ids = []
    for y, rx, rz in rings:
        ids.append([mesh.vertex((rx*math.sin(i/radial*TAU), y, -rz*math.cos(i/radial*TAU))) for i in range(radial)])
    for first, second in zip(ids, ids[1:]):
        for i in range(radial):
            j = (i+1) % radial
            mesh.face(first[i], first[j], second[i], material)
            mesh.face(first[j], second[j], second[i], material)
    mesh.cap(ids[0], material, reverse=True)
    mesh.cap(ids[-1], material)
