# Cubacadabra Tools — Developer Preview 0.4

`cubacadabra` is the native Rust command-line toolbox for Cubacadabra projects.
The project and package builder libraries are shared directly with Studio, so
raw-project builds do not require Python.

## Run

cargo run --release --bin cubacadabra -- --help

cargo run --release --bin cubacadabra -- create-game --title "The Wild West" --path /Users/aa/cubacadabra/examples

cargo run --release --bin cubacadabra -- build-game --source /Users/aa/cubacadabra/examples/the-wild-west --output /tmp/foo --zip ../the-wild-west.zip

## Install

Build the native CLI from this repository:

```sh
cargo install --path crates/cli --locked
```

The command is then available as `cubacadabra`. Maintainer-only commands that
have not migrated yet remain in the legacy Python modules, but they are not
required by Studio or by `build-game` / `create-game`. The Python package
builder no longer expands `maze` declarations; maze projects must use the
native Rust `build-game` command so one source project cannot produce divergent
packages.

## Commands

```text
cubacadabra [--version] COMMAND

Commands:
  create-game Create a new starter game.
  build-game  Build a portable game package from a game project.
  import-roblox-reference Extract a static visual-reference scene from Roblox XML.
  upload-examples Bump, build, and upload both example games. (legacy Python)
  setup-local  Build and install the Morph catalog in local R2/D1. (legacy Python)
  morph ...     Morph release commands. (legacy Python)
```

## Roblox visual-reference import

`import-roblox-reference` is a development tool for source-faithful renderer
comparison. It reads Roblox XML place/model data with `rbx_xml` and writes a
deterministic, tool-owned intermediate JSON scene. It does not emit a playable
Cubacadabra package and does not define a runtime package contract.

```sh
cargo run --release --bin cubacadabra -- import-roblox-reference \
  --place ../other-examples/maze-world/Place.rbxmx \
  --terrain ../other-examples/maze-world/PlaceTerrain.rbxmx \
  --project ../other-examples/maze-world/default.project.json \
  --output /tmp/maze-world-reference-scene.json
```

The intermediate scene preserves source hierarchy paths, transforms, sizes,
colors, material IDs and names, mesh and texture asset references, transparency,
collision/shadow flags, local lights, cameras, text, spawn areas, project
lighting and post-effect settings, computed visible bounds, and source hashes.
Roblox smooth-terrain voxel blobs are recorded by size and SHA-256 but are not
decoded yet; that limitation is explicit in the generated scene.

`import-roblox-scene` can promote selected physical Parts into native authoring
primitives. Repeat `--editable-part-name` for the source display names to
promote; `--editable-part-path-prefix` can limit the match to one source area.
`Part` geometry with a runtime-supported axis-aligned rotation is promoted by
this first primitive slice. Promotion preserves the source `CanCollide` state:
collidable Parts receive a native box collision component, while non-collidable
Parts remain editable primitives without collision. Other source records remain
available in the imported source hierarchy.

Bake a selected imported hierarchy into a compact static GLB for the normal
Cubacadabra package renderer:

```sh
cargo run --release --bin cubacadabra -- export-reference-mesh \
  --scene /tmp/reference-scene.json \
  --output ../examples/maze-101/assets/models/maze_world_reference.glb \
  --path-prefix 'Folder:Place[1]/Folder:Main[1]/Model:MainIsland[1]'
```

The exported GLB bakes source transforms, vertex colors, and one named glTF
primitive/material group per Roblox material. It remains an ordinary package
model, so Studio and player hosts render it through the shared world-mesh path
rather than the development-only reference capture shader.

Repeat `--path-prefix` to include multiple hierarchies. `--exclude-path` omits
paths containing a supplied fragment; `--scale` applies the same positive unit
conversion to the visual mesh and optional `--collision-output scene-collision.json`.
Collision includes invisible source parts with `canCollide`, excludes non-colliding
parts, and is independent of GLB loading. Reference it from an authored world's
`collision: {"source": "reference/scene-collision.json"}`; the builder validates and
inlines versioned world-space triangles into the runtime manifest.

Optional `--mesh-overrides meshes.json` supplies locally available geometry for
source mesh IDs. Version 1 is `{"formatVersion":1,"meshes":{"SOURCE_ID":{
"vertices":[[0,0,0],[1,0,0],[0,1,0]],"triangles":[[0,1,2]]}}}`. Vertices use
normalized source-local coordinates (usually -0.5 to 0.5); the exporter applies
each instance's source size, rotation, position, color, and export scale. Unknown
IDs retain the primitive approximation. Mesh files are not downloaded implicitly.

Create a new game from a title and a parent directory. The command creates a
directory named from the title, with a starter `manifest.json`, `src/main.luau`,
and empty `assets/audio/` and `assets/images/` directories:

```sh
cubacadabra --create-game --title "The Wild West" --path ~/games
# or: cubacadabra create-game --title "The Wild West" --path ~/games
# standalone/offline editor copy: add --vendor-sdk
```

Build any compatible game repository from its project directory:

```sh
cubacadabra build-game ../first-game
cubacadabra build-game ../second-game --output ../second-game/build/package
cubacadabra build-game ../third-game --zip ../third-game/build/third-game.zip
```

The starter created above follows the standard `src/main.luau` and `assets/`
layout. It can be built by pointing `--source` at the project directory; the
CLI selects `src/` and finds its `manifest.json` automatically:

```sh
cubacadabra build-game --source ~/games/the-wild-west --output ../the-wild-west \
  --zip ../the-wild-west.zip
```

By convention, a game project contains `manifest.json`, `src/main.luau`, and
an optional `assets/` directory. The manifest's `id` and SemVer `version`
become package metadata; legacy positive integer versions remain accepted.
Luau files use normal relative modules. The builder follows static string
`require()` calls, gives each module its own scope and cached return value, and
bundles the reachable graph into the package's single `game.luau` artifact:

```luau
local Round = require("./round")
local Document = require("./ui/document")
```

Portable SDK helpers use the `@cubacadabra` alias. The manifest's `sdkVersion`
pins the SDK contract, so module paths do not repeat the version:

```luau
local CubaSharedState = require("@cubacadabra/shared-state")
local CubaDisclosure = require("@cubacadabra/disclosure")
local CubaSurvival = require("@cubacadabra/survival")
local CubaCycle = require("@cubacadabra/cycle")
```

The checked-in workspaces map that alias to the SDK source with `.luaurc`, so
Luau-aware editors can navigate and type-check the same module graph that the
builder bundles. `.luaurc` is editor configuration only; the builder always
uses the canonical SDK shipped with the toolchain. Require paths must be static
strings and must start with `./`, `../`, or `@cubacadabra/`.

Game IDs use the package-wide contract of 3–64 lowercase letters, numbers, and
single dashes. `create-game`, `build-game`, Studio, the browser, and mobile
clients reject IDs outside that contract. Newly created projects do not copy
the SDK; use `--vendor-sdk` only when an offline standalone editor copy is
needed.

Every generated package also contains `package.json`, whose SHA-256 map binds
the manifest, script, and asset files to that package release. It also records
the reachable SDK helper modules and their source hashes. Clients should
validate those hashes before executing or caching a remote package. See
[preview licensing](https://github.com/cubacadabra/docs/blob/main/platform/licensing.md) for the current reuse policy.

`CubaSharedState` v1 owns bounded intent queuing, compare-and-set retries,
conflict rebasing, and reconnect snapshots. Games provide their own initial
state, validator, reducer, and optional change callback, so neither the SDK nor
the backend needs to know game-specific rules.

`CubaSurvival` v1 is a small lifecycle helper for runtime health, damage, death,
and respawn events. World physics, hazards, and safe zones stay in the manifest;
objectives, inventory, and presentation stay in the game script. `CubaCycle`
v1 provides a small reusable day/night or calm/storm clock for games that need
repeating pressure.

See [Shared state SDK v1](https://github.com/cubacadabra/docs/blob/main/contracts/sdk/shared-state.md) for the reducer contract and
lifecycle API. [Disclosure SDK v1](https://github.com/cubacadabra/docs/blob/main/contracts/sdk/disclosure.md) provides a generic
tap-to-reveal controller, and [Survival SDK v1](https://github.com/cubacadabra/docs/blob/main/contracts/sdk/survival.md) provides a
reusable health/death/respawn lifecycle while each game continues to own its
HUD composition. See the game guide for the cycle and safe-zone contracts.

For a checked-out platform workspace, run the cross-repository compatibility
gate after building the browser renderer:

```sh
sh ../rust/scripts/build_web_renderer.sh --debug
PYTHONPATH=src python3 scripts/check_workspace_compatibility.py
```

It builds every supported game with this CLI and loads the resulting packages
through the native Rust client, browser WASM client, and Studio's raw-project
validator. It also runs a deterministic lifecycle/input trace through native
and WASM and compares the serialized game actions and public snapshot bits.

The same reusable workflow is called by the tools, Rust, web, Studio, game,
and examples repositories. For a coordinated change, run the tools workflow
manually and provide explicit refs for the participating repositories.

The complete preview contract is in the
[Cubacadabra creator guide](https://github.com/cubacadabra/docs/blob/main/contracts/creator-guide.md).

## Git hooks

Enable the repository's pre-commit hook once per checkout:

```sh
git config core.hooksPath .githooks
```

When a commit includes Rust source, the hook runs `cargo fmt --all` and
auto-stages formatting changes for Rust files that were already staged. Files
with separate unstaged changes must be staged or discarded before committing.

One-shot game audio is declared under `assets.audio` with an id, path, and
optional volume. The builder validates up to 64 package-local WAV files as
48 kHz, 16-bit PCM with one or two channels and a maximum size of 4 MiB each.

Large game-owned effect libraries may live in a separate source file:

```json
{
  "effects": { "source": "effects.json" }
}
```

The builder validates the relative path and inlines the effect library into the
portable package manifest. Runtime clients still receive one self-contained
manifest and do not need filesystem or include behavior.

Run `cubacadabra build-game --help` for all options.

Build and install the Morph release into local R2 and D1 after starting the
backend with `npm run dev`:

```sh
cubacadabra setup-local
```

The command compiles every source manifest with the pinned authoring compiler,
uploads immutable content-addressed packs, applies local D1 migrations, and
installs an idempotent catalog release. Both commands refresh the starter
thumbnails through the shared GPU renderer and update their content hashes;
use `--skip-thumbnails` for headless builds. Use `cubacadabra morph build` to
only generate the ignored release directory. `--starter-set DIR`, `--endpoint
URL`, and `--dry-run` remain available.

Upload both example games after a change to the engine, web client, or example
projects. The default target is the local backend at `127.0.0.1:8787`:

```sh
PYTHONPATH=src python3 -m cubacadabra upload-examples
PYTHONPATH=src python3 -m cubacadabra upload-examples --target production
```

The command increments each example's patch version, rebuilds the ZIPs in the
parent directory, and uploads them with the review account. Set
`CUBACADABRA_REVIEW_PASSWORD` to override the default testing password. Use
`--no-bump` when retrying an upload for versions that were already built.

## Development

```sh
cargo test --workspace
cargo run --release --bin cubacadabra -- --help
```
### Licensing

Copyright (C) 2026 Andrew Arrow

Licensed under the GNU General Public License v3.0 or later.
See [LICENSE](LICENSE).
