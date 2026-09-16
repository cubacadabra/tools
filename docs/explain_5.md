Exactly:

* Roblox text model: **`.rbxmx` = XML**

  * Cubacadabra replacement: **JSON**

* Roblox binary model: **`.rbxm`**

  * Cubacadabra replacement: **we do not have a general equivalent yet**

Right now Cubacadabra is more like:

```text
Roblox                     Cubacadabra

model.rbxmx   XML    →      prefab.json / manifest JSON

model.rbxm    binary →      nothing equivalent yet
```

The actual heavy 3D stuff is separate:

```text
Blender source
   ↓
GLB
   ↓
possibly compiled binary asset formats such as .morphpack
```

And the whole game gets packaged into a **ZIP**.

So imagine a LEGO set:

```text
JSON
= instruction booklet saying
  "put this object here, rotate it this way,
   use this mesh, add this collider"

GLB / morphpack
= the actual plastic LEGO pieces

ZIP
= the box containing everything
```

Roblox's `.rbxm` is basically a **compact binary version of the instruction booklet**.

We haven't needed that yet. If Cubacadabra eventually wants one, my recommendation would probably be:

```text
thing.json       human-readable version
thing.cub / etc. compact MessagePack binary version
```

Both would describe **the exact same objects**.

So the simplest answer is:

> **RBXMX → JSON. RBXM → nothing yet; JSON is currently sufficient. If we later need a binary equivalent, probably MessagePack encoding of the same JSON-style schema.**

And **`.morphpack` is not our RBXM replacement**—it's specifically compiled geometry/assets, a different layer.

