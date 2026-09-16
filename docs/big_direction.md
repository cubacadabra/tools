**It sharpens the design, but it does not change the decision to use JSON.** The important separation is **game-object descriptions versus the mesh/image data those objects reference**.

And I need to correct something from my earlier answers: **I suggested several possible binary formats; I had not established that Cubacadabra had selected or implemented one for general models.** I have now checked the relevant committed code.

## What Cubacadabra actually uses today

| Layer                          | Current implementation                                                                                                                                          |
| ------------------------------ | --------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **World/game definition**      | JSON. `GamePackageDefinition::parse()` reads it with `serde_json::from_str`.                                                                                    |
| **Generic entity graph**       | Serializable entities containing an ID, class, name, parent, and JSON-compatible properties.                                                                    |
| **Saved running-engine state** | A versioned JSON `EngineSnapshot`, distinct from the authored world definition.                                                                                 |
| **Compiled Morph assets**      | A custom binary **`.morphpack`** format, currently schema **5**, identified by the **`CUBAMORP`** header.                                                       |
| **Distributable game package** | A directory or ZIP containing `manifest.json`, `package.json`, `game.luau`, and assets. The builder does not convert the world JSON into a binary scene format. |

These are separate, implemented paths—not different names for one format.

**I did not find an implemented, general-purpose binary prefab/world codec equivalent to `.rbxm` in the paths I inspected.** The existing Morph codec is real, but it solves a different problem.

### What is inside our existing binary format?

Your `docs/morph-pack-v5.md` and decoder agree on this structure:

```text
.morphpack

Header
    "CUBAMORP"
    schema version
    reserved flags
    JSON manifest length

JSON metadata
    asset definition
    attachment information

Binary textures
    dimensions
    RGBA8 pixels

Binary geometry for Near / Mid / Far
    positions
    normals
    UV coordinates
    optional skinning data
    triangle indices
    surface information
```

The numeric fields use a defined little-endian layout. **This is a custom format—not MessagePack, FlatBuffers, or a dump of Rust’s in-memory structures.** It deliberately combines JSON metadata with binary bulk data.

That is a reasonable division of responsibilities. There is no requirement that a “binary file” eliminate every piece of JSON.

## The key correction: `.rbxm` is not the binary mesh

Roblox’s `.rbxmx` and `.rbxm` are **XML and binary representations of Roblox models**. They are not respectively “object metadata” and “the actual triangles.” Rojo treats both as model inputs. ([Rojo][1])

A model containing a `MeshPart` can reference mesh and texture assets through its properties, regardless of which model encoding was used. Roblox’s `MeshPart` API exposes those content references separately. ([Creator Hub][2])

So the useful conceptual comparison is:

```text
Roblox object description              Cubacadabra object description
─────────────────────────              ─────────────────────────────
.rbxmx or .rbxm                         Our scene/prefab document
    │                                      │
    └── mesh asset reference               └── mesh asset reference
             │                                      │
             ▼                                      ▼
       Mesh asset bytes                        Mesh asset bytes
```

**Our `.morphpack` belongs primarily on the bottom row, not the top row.**

Similarly, GLB can contain meshes, materials, node hierarchies, skins, and animations—not just triangles—but it does not by itself define Cubacadabra’s game-object behavior and lifecycle. ([Khronos Registry][3])

Also, not every Roblox model came from Blender. The `Wall.rbxmx` we inspected describes an ordinary primitive `Part` with dimensions and properties. Our equivalent should be able to describe that wall without needing a GLB at all.

## What I would keep—and tighten—in our JSON design

**Keep JSON as the authored scene/prefab format. But define the document’s meaning explicitly, rather than assuming that any serializable entity graph is already a complete prefab format.**

Your existing `DataModel` is a useful foundation: it already represents entities, parent relationships, properties, and mutations. Its documentation explicitly calls it a substrate rather than a finished Luau `Instance` API or renderer/physics bridge.

I would tighten four things.

### 1. Separate authored content from saved gameplay state

A **prefab** describes what to instantiate:

> Create a chest, its lid, its collider, its visual asset, and its behavior attachment.

A **runtime snapshot** describes what was happening:

> This chest is open, the timer is at 42 seconds, these players are present, and this simulation has advanced to tick 900.

Your snapshot format already includes simulation clocks, RNG state, player state, pending events, and other running-engine information. That is appropriate for resuming execution, but not automatically appropriate for exporting a reusable chest.

**Reuse the underlying types and validation where useful, but do not make “Save Prefab” dump the entire runtime snapshot.**

### 2. Distinguish object references from asset references

These need different semantics:

```text
Object reference:
    “This hinge connects to the lid inside this chest.”

Asset reference:
    “This renderer uses the shared chest mesh.”
```

When someone duplicates a chest, I would require the engine to create new object identities and remap its internal references. Both chests should still share the same immutable mesh asset.

Your current `EntityId` is explicitly documented as stable **during an engine lifetime**. That is not yet the same promise as a stable authored-file identity. A prefab format needs document-local identifiers and a clear mapping to runtime entity IDs.

This is more consequential than whether the file uses XML or JSON.

### 3. Give properties a schema, not just a place to live

A generic JSON property bag is useful, but I would not let it be the entire contract.

For engine-understood properties, define what values mean: the transform convention, coordinate units, quaternion order, collider settings, asset-reference structure, and behavior references. Define what happens when a document contains an unsupported component or schema version.

For example, an importer must not silently turn:

```text
unsupported hinge constraint
```

into:

```text
nothing
```

and report that the model imported successfully.

I would allow explicitly namespaced game-owned data, while requiring validation of engine-owned components.

### 4. Make asset dependencies explicit and resolvable

The lesson from Maze World is not that remote assets are wrong. It is that **a model file is not necessarily a self-contained asset bundle**.

For Cubacadabra, I would require a build to resolve each asset reference to either packaged bytes or a pinned external dependency. In development that can be a project-relative file; a published release can resolve to an immutable content-hashed object.

Your builder already records a SHA-256 map for packaged files, and the Morph publishing workflow already uses immutable content-addressed packs. Those are foundations to extend, not replace.

For example games, my preference would be: **ordinary game-owned props should build and run from the checkout without depending on someone else’s Roblox asset account.** Shared catalog assets can remain separate, provided their dependency and version are explicit.

## What should we use for a binary prefab counterpart?

**For now, I would keep the world/prefab metadata as JSON inside the existing package, and keep large media data in binary asset files.**

That is not an incomplete architecture. It is also consistent with your existing Morph format’s use of JSON metadata alongside binary geometry.

I would not introduce a second scene encoding merely because Roblox has two.

However, **when we actually need a binary encoding of the same prefab document, my default choice would be MessagePack with string-keyed maps**, wrapped in a small versioned file envelope. MessagePack provides binary representations of maps, arrays, strings, numbers, and other values with cross-language implementations. ([MessagePack][4])

The intended relationship would be:

```text
                    One scene/prefab schema
                         /          \
                        /            \
               JSON encoding     MessagePack encoding
                        \            /
                         \          /
                    Same validated document
                              │
                              ▼
                       Instantiate objects
```

That is a **recommendation, not something currently implemented**.

I would require both encodings to preserve the same supported document semantics. Neither would become a special mesh format, and neither would replace `.morphpack` or the ZIP package. I would measure actual load time and size before assuming the additional codec is worthwhile.

And I would **not force ordinary world props into `.morphpack` unchanged**. Its current contract is specifically built around Morph assets, rigid/skinned attachments, and three fixed LODs. A generic world-mesh path can reuse suitable geometry code without pretending every rock is a character attachment.

## The tests that would convince me it is done right

Before adding a binary counterpart, I would want these tests passing for JSON:

| Test                                           | Required result                                                                                        |
| ---------------------------------------------- | ------------------------------------------------------------------------------------------------------ |
| Save and reload a prefab                       | Supported objects, transforms, properties, and references retain their meaning.                        |
| Instantiate the same articulated prefab twice  | Each copy’s internal references point into that copy; both share the same mesh asset.                  |
| Build with a missing mesh or texture           | An explicit dependency error, not an apparently successful but incomplete model.                       |
| Load an unsupported schema/component           | A defined rejection or migration path, not silent data loss.                                           |
| Open a packaged example on a clean machine     | No hidden dependency on the original author’s filesystem or Roblox asset IDs.                          |
| Parse a prefab containing behavior attachments | Parsing does not itself execute scripts; execution begins only through the intended runtime lifecycle. |

When a binary scene encoding is added, add the invariant:

> **JSON and binary versions of the same document instantiate equivalent supported content.**

If Roblox import becomes part of the workflow, I would route `.rbxmx` and `.rbxm` through the same conversion layer, producing our document plus a report of unresolved assets and unsupported Roblox features—not two independent import implementations with different behavior.

**Bottom line: keep JSON. We already have custom binary Morph assets and ZIP game packages; we do not yet have a verified general binary prefab counterpart. The work that matters next is a precise prefab schema, correct reference remapping, and explicit asset dependencies—not choosing a more elaborate binary format.**

[1]: https://rojo.space/docs/v7/sync-details/ "Sync Details | Rojo"
[2]: https://create.roblox.com/docs/reference/engine/classes/MeshPart "MeshPart | Documentation - Roblox Creator Hub"
[3]: https://registry.khronos.org/glTF/specs/2.0/glTF-2.0.html "glTF™ 2.0 Specification"
[4]: https://msgpack.org/ "MessagePack: It's like JSON. but fast and small."

