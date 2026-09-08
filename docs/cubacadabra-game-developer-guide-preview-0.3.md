# Cubacadabra Game Developer Guide — Developer Preview 0.3

Status: preview contract, version `0.3.0`.

This guide is the single reference for making a game package for Cubacadabra.
It describes the package builder, the game-facing Luau API, the bundled SDK
helpers, the backend connection contract, and the current product boundary.

The examples are real packages:

- [`first-game`](../../first-game/) is a cooperative spell-collection game.
- [`second-game`](../../second-game/) is a sequential relay race.
- [`third-game`](../../third-game/) is the capability-probe package and the
  breadth/conformance example for this preview.

## 1. What ships in the preview

The preview has one portable package format. A game repository contains:

```text
manifest.json
src/main.luau
assets/                 optional package-local assets
effects.json            optional source referenced by the manifest
```

Build it from the repository root or from the game directory:

```sh
PYTHONPATH=tools/src python3 -m cubacadabra build-game first-game \
  --output first-game/build/package \
  --zip first-game/build/first-game-v0.3.0.zip
```

The builder expands local `-- @include "file.luau"` directives, expands the
reserved SDK includes, validates the manifest and assets, inlines an effects
source file, and writes `game.luau`, `manifest.json`, and `package.json`.
Includes must stay below `src/`; cycles, invalid UTF-8, unsafe paths, and
top-level returns in included modules are rejected.

Use SemVer for game and SDK compatibility:

```json
{
  "id": "my-game",
  "version": "0.3.0",
  "sdkVersion": "0.3.0",
  "package": { "formatVersion": 3, "entry": "game.luau" }
}
```

`version` is the package/content version. `sdkVersion` is the preview SDK
contract the source expects. The `version` fields inside effect and game state
payloads are game-owned schemas and may remain at `1`.

When `sdkVersion` is present, the preview builder requires the exact supported
value `0.3.0`; this prevents a package from silently using an unknown SDK.

## 2. Game lifecycle

`src/main.luau` returns a table. Every callback is optional; the capability
probe shows all six supported callbacks:

```luau
local Game = {}

function Game.on_start(api) end
function Game.on_tick(api, delta_seconds) end
function Game.on_interaction(api, event) end
function Game.on_network_message(api, event) end
function Game.on_ui_event(api, event) end
function Game.on_launch(api, launch) end

return Game
```

`on_start` runs once after the package loads. `on_tick` receives elapsed seconds.
Interaction events contain `id`, `phase` (`"enter"` or `"exit"`), and `players`.
UI events contain `node_id`, `action`, and `phase`; sliders and toggles also
contain `value`. Launch events contain `pad_id` and `player_ids`.

## 3. Game-facing Luau API

The API is intentionally small and game-agnostic. Game rules remain in Luau;
Rust supplies bounded simulation, presentation, and transport primitives.

### Lobby and sessions

```luau
api.lobby:set_enabled(false)
api.lobby:set_status("Find the three signal gates")
api.session:start("signal-run", { mode = "cooperative" })
```

`set_enabled` supplies an optional lobby override. `set_status` changes the
shared lobby status. `session:start(name, options)` records the game session
request; the options table is reserved for future session configuration.

### Interactions

Declare generic zones in the world manifest, then read their current state:

```luau
local state = api.interactions:get_state()
local gate = state.zones["gate-a"]
if gate and gate.inside then
    -- gate.nearby, gate.players, gate.kind, gate.label are also available.
end
local event_id = state.event_id
```

The same contract supports pickups, race gates, doors, checkpoints, buttons,
or gathering areas. It does not introduce game-specific Rust types.

### Retained UI

`set_document` accepts a Luau table or JSON string. Supported node kinds are
`panel`, `stack`, `text`, `button`, `menu`, `modal`, `toggle`, `slider`, and
`joystick`.

```luau
api.ui:set_document({
    nodes = {
        { id = "score", kind = "text", text = "SCORE 0" },
        {
            id = "objective",
            kind = "button",
            text = "OBJECTIVE",
            action = "objective.toggle",
        },
    },
})

api.ui:set_text("score", "SCORE 1")
api.ui:set_value("volume", 0.75)
api.ui:set_checked("objective", true)
api.ui:set_visible("objective-detail", false)
api.ui:clear()
```

The complete mutation surface is `set_document`, `clear`, `set_text`,
`set_value`, `set_checked`, and `set_visible`. IDs are bounded and must be
unique. UI events are delivered to `on_ui_event`; use `event.action` to route
game-owned controls. The platform shell owns account, payment, permission, and
operating-system UI.

### Network messages and retained state

```luau
api.network:publish("round-events", { type = "captured", id = event.id })
api.network:set_state("last-round-event", { id = event.id })
api.network:compare_set_state("round", expected_sequence, next_state)
```

`publish` is an ephemeral broadcast. `set_state` writes retained state for
channels that have not been claimed by compare-and-set. `compare_set_state`
requires the expected sequence and is the primitive used by
`CubaSharedState`. Channels are 1–64 bytes; serialized messages are limited to
64 KiB. All payloads must be JSON-compatible and bounded.

Incoming events are delivered to `on_network_message`:

```luau
-- event.type is "game_message" or "game_state"
-- event.channel, event.payload, and (for retained state) event.sequence
-- retained game_state events may also contain conflict, authoritative, ageMs.
```

### Audio and effects

Declare package audio under `assets.audio`, then request one-shot playback:

```luau
api.audio:play("gate-hit", { volume = 0.8 })
api.effects:set_state("gate-a", "captured")
api.effects:play("capture-burst", { position = { 2, 0, -4 } })
```

Audio IDs and effect IDs are bounded ASCII identifiers. Audio is package-local
48 kHz, 16-bit PCM WAV, mono or stereo, with at most 64 declared files and 4 MiB
per file. Effect templates are declared in the manifest or in an `effects.json`
source. Effect positions must be finite world coordinates. The runtime bounds
effect and audio queues; a game should react to accepted state rather than
spamming commands every frame.

## 4. Bundled SDK helpers

### Shared state v1

```luau
-- @include "@cubacadabra/shared-state-v1.luau"

local store = CubaSharedState.create({
    channel = "shared-score",
    initial = function() return { version = 1, score = 0 } end,
    validate = function(value)
        if type(value) ~= "table" or value.version ~= 1 then return nil end
        return { version = 1, score = math.max(0, value.score or 0) }
    end,
    reduce = function(state, intent)
        if intent.type == "score" then
            return { version = 1, score = state.score + 1 }
        end
        return nil
    end,
    onChange = function(api, state, previous, event, context) end,
})

function Game.on_start(api) store:start(api) end
function Game.on_interaction(api) store:dispatch(api, { type = "score" }) end
function Game.on_network_message(api, event) store:receive(api, event) end
function Game.on_tick(api, delta_seconds) store:update(api, delta_seconds) end
```

`create`, `start`, `dispatch`, `receive`, and `update` are the public helper
calls. The store owns a bounded intent queue, deterministic reducer retries,
conflict rebasing, duplicate-intent removal, and reconnect snapshots. Reducers
must be deterministic, idempotent, side-effect free, and return a new state.
The helper is cooperative state synchronization, not cheat-resistant authority.
Read-only fields are `value`, `sequence`, `ageSeconds`, and `hasSnapshot`.

### Disclosure v1

```luau
-- @include "@cubacadabra/disclosure-v1.luau"

local disclosure = CubaDisclosure.create({
    action = "objective.toggle",
    triggerId = "objective-button",
    nodeIds = { "objective-detail" },
})

function Game.on_start(api)
    api.ui:set_document(GameUIDocument.create())
    disclosure:sync(api)
    disclosure:set_open(api, false)
end

function Game.on_ui_event(api, event)
    if disclosure:handle(api, event) then return end
end
```

The public calls are `create`, `sync`, `set_open`, and `handle`. The helper owns
only open/closed state and trigger handling; the game owns the document, copy,
styles, and placement. It accepts 1–32 node IDs and keeps an optional trigger’s
checked state synchronized.

## 5. Manifest essentials

The manifest owns content, not engine code. At minimum, define `id`, `version`,
`package`, `scene`, a `world`, and `worlds` when the game has named destinations.
World content can include palettes, blocks, signs, clouds, generic interaction
zones, and launch pads. `launch.destinationWorld` selects the destination when
the shared lobby is disabled. The `effects` object may contain an inline
library or `{ "source": "effects.json" }`.

Keep state schemas and interaction IDs stable within a package version. Use
small, semantic IDs such as `node-1`, `objective`, and `round-state`; they are
the bridge between manifest, Luau, and retained presentation.

## 6. Backend API available to clients

Base URL: `https://api.cubacadabra.com` in production. Local development uses
`http://127.0.0.1:8787`. The live OpenAPI document is available at
`/openapi.json`; interactive docs are at `/docs` and `/redocs`. Cookie sessions
use `cubacadabra_session`; native clients use `Authorization: Bearer <access_token>`.

The following is the complete HTTP surface in the preview OpenAPI contract.
Request bodies use JSON and responses use JSON unless noted.

| Method and path | Auth | Request body/query | Purpose |
| --- | --- | --- | --- |
| `GET /health` | no | — | Service health; returns `{ ok }`. |
| `GET /admin/status` | operator/internal | — | World-instance status and capacity diagnostics. |
| `POST /auth/google` | no | `{ credential }` | Web Google sign-in. |
| `POST /auth/email` | no | `{ email, password }` | Web email sign-in. |
| `POST /auth/app/google` | no | `{ credential }` | Native Google sign-in and tokens. |
| `POST /auth/app/email` | no | `{ email, password }` | Native email sign-in and tokens. |
| `GET /auth/browser/consume` | no | `code`, optional `returnTo` | Consume a browser handoff and redirect. |
| `POST /auth/browser/authorize` | session | — | Create a browser handoff code. |
| `POST /auth/app/authorize` | session | `{ redirect_uri }` | Create native authorization code. |
| `GET /auth/app/redirect` | session | `redirect_uri`, optional `state` | Authorize native redirect. |
| `POST /auth/app/exchange` | no | `{ code, redirect_uri }` | Exchange native code for tokens. |
| `POST /auth/app/refresh` | no | `{ refresh_token }` | Refresh native tokens. |
| `POST /auth/age-gate` | no | `{ dob: "YYYY-MM-DD" }` | Submit age gate and return user. |
| `GET /auth/me` | session | — | Return current user. |
| `POST /auth/birthday` | session | `{ dob: "YYYY-MM-DD" }` | Set a user birthday once. |
| `POST /auth/username` | session | `{ username }` | Set an eligible username. |
| `POST /auth/avatar` | session | `{ body_id }` | Set an eligible avatar body. |
| `POST /auth/logout` | session | — | End the current cookie session. |
| `GET /moderation/blocks` | session | — | List blocked user IDs. |
| `POST /moderation/blocks` | session | `{ user_id }` | Block a user. |
| `DELETE /moderation/blocks/{userId}` | session | path `userId` | Unblock a user. |
| `POST /moderation/reports` | session | `{ user_id, reason, details?, world_id }` | Report a user. |
| `GET /subscription` | session | — | Read subscription state. |
| `POST /subscription/checkout-session` | session | — | Create embedded Stripe checkout. |
| `POST /subscription/checkout-session/complete` | session | `{ checkout_session_id }` | Complete checkout. |
| `POST /subscription/cancel` | session | `{ subscription_id }` | Cancel current subscription. |
| `GET /world/{worldId}` | optional/forwarded | WebSocket upgrade | Join a live world session. |

Public user bodies expose `id`, `email`, `name`, `created_at`, `dob`,
`username`, and one of `cuba:person.v1`, `cuba:person-girl.v1`, or
`cuba:person-nb.v1` as `body_id`. Native token responses contain access and
refresh tokens plus their expiry values. Authentication, age eligibility,
subscription, moderation, and rate-limit errors are part of the contract; a
game should surface a useful state instead of leaking raw error text.

### World WebSocket

Connect to `wss://api.cubacadabra.com/world/{worldId}` (or the local `ws://`
equivalent). Client messages include:

```json
{ "type": "move", "x": 0, "y": 0, "z": 0, "yaw": 0, "sprinting": false }
{ "type": "set_username", "username": "PLAYER" }
{ "type": "set_hidden", "hidden": false }
{ "type": "set_appearance", "appearance": {} }
{ "type": "game_message", "channel": "round-events", "payload": {} }
{ "type": "game_state_set", "channel": "lobby-note", "payload": {} }
{ "type": "game_state_compare_set", "channel": "round", "expectedSequence": 0, "payload": {} }
```

The server sends session identity, player join/leave, authoritative move
corrections, `game_message`, retained `game_state`, and structured `error`
events. Movement is rate-limited and distance-canonicalized server-side.
`CubaSharedState` should be used for game state rather than reimplementing the
retained-state protocol in each game.

## 7. Making an amazing game here

Start with one readable cooperative verb: collect, connect, build, race, or
reveal. Put the rules in a small Luau module, keep the manifest generic, and
make every state transition visible through a label, effect, or sound. A strong
preview game should have:

1. A playable first minute with no account or tutorial wall.
2. A solo-completable loop that becomes better with two or more players.
3. One authoritative retained state for the shared outcome.
4. Clear feedback for enter, progress, success, reconnect, and reset.
5. A calm, legible mobile HUD with touch targets that remain usable at 390px.
6. Bounded assets, effects, queues, and state payloads.

The first two games demonstrate content depth. The third game demonstrates API
breadth. Keep both standards: a technically complete package still needs a
clear reason to play.

## 8. Cubacadabra vs. Roblox

Roblox is a much larger, mature creation ecosystem with a full Studio editor,
client/server scripting, broad platform services, publishing, discovery, and a
creator economy. See the [Roblox experiences overview](https://create.roblox.com/docs/experiences),
[scripting documentation](https://create.roblox.com/docs/scripting),
[remote events](https://create.roblox.com/docs/scripting/events/remote), and
[DataStore documentation](https://create.roblox.com/docs/cloud-services/data-stores).

Cubacadabra’s current advantage is focus: a tiny package format, one source
artifact consumed by web/native clients, generic multiplayer primitives, a
portable bounded SDK, and a low-friction path from a Luau idea to a playable
world. It is a good fit for small cooperative games and experiments where
predictable cross-client behavior matters.

It is not Roblox parity yet. Preview 0.3 does not provide a visual editor,
arbitrary server-side game code, a game-owned durable database API beyond the
bounded world state primitive, creator publishing/discovery, or an economy.
Do not promise those capabilities to creators. The honest pitch is “small,
portable cooperative games with a clear runtime contract,” not “Roblox with
fewer menus.”

## 9. Preview exit criteria

Preview 0.3 is ready to share as a Developer Preview, not to label `1.0.0`.
Promote only when all of these remain true in a clean checkout:

- the Rust runtime tests pass;
- first-game, second-game, and third-game build and load successfully;
- the guide’s API matrix matches the generated SDK and runtime;
- version, package, and SDK compatibility rules are documented and enforced;
- a real multiplayer session verifies reconnect, retained state, and rejection
  of malformed or conflicting state;
- the platform has an explicit answer for publishing, moderation, and package
  compatibility.

For this preview, run:

```sh
PYTHONPATH=tools/src python3 -m unittest discover -s tools/tests -v
(cd rust && cargo test --lib)
for game in first-game second-game third-game; do
  PYTHONPATH=tools/src python3 -m cubacadabra build-game "$game" \
    --output "/tmp/cubacadabra-$game-package"
done
```

The preview is ready for creator feedback after those checks. It is not yet a
stable `v1.0.0` platform contract.
