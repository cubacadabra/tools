# Shared state SDK v1

Use the shared-state helper when several clients can change one retained piece
of cooperative game state.

```luau
local CubaSharedState = require("@cubacadabra/shared-state")
```

The module returns `CubaSharedState`; `build-game` bundles it into the generated
game chunk. The SDK is client-side Luau built on
`api.network:compare_set_state`; it does not add game
rules to Rust or the backend.

## Create a store

```luau
local function initial_state()
    return { version = 1, score = 0, operations = {} }
end

local function validate_state(value)
    if type(value) ~= "table"
        or value.version ~= 1
        or type(value.score) ~= "number"
        or value.score < 0
        or type(value.operations) ~= "table"
    then
        return nil
    end
    local operations = {}
    for operation_id, accepted in pairs(value.operations) do
        if type(operation_id) == "string" and accepted == true then
            operations[operation_id] = true
        end
    end
    return { version = 1, score = value.score, operations = operations }
end

local function reduce_state(state, intent)
    if intent.type == "score"
        and type(intent.operationId) == "string"
        and not state.operations[intent.operationId]
    then
        local operations = {}
        for operation_id, accepted in pairs(state.operations) do
            operations[operation_id] = accepted
        end
        operations[intent.operationId] = true
        return {
            version = 1,
            score = state.score + 1,
            operations = operations,
        }
    end
    return nil
end

local function operation_status(state, intent)
    return state.operations[intent.operationId] and "accepted" or "pending"
end

local operation_number = 0

local store = CubaSharedState.create({
    channel = "shared-score",
    initial = initial_state,
    validate = validate_state,
    reduce = reduce_state,
    mode = "distinct",
    operationStatus = operation_status,
    onChange = function(api, state, previous, event, context)
        -- Update world effects, audio, and UI from accepted state here.
    end,
})

store:subscribe(function(api, state, previous, event, context)
    -- Any other consumer can react to the same accepted snapshot.
end)
```

Configuration:

- `channel`: the retained network channel, from 1 to 64 bytes.
- `initial()`: returns the state proposed when the channel does not exist.
- `validate(value)`: returns a normalized state or `nil` to reject it.
- `reduce(state, intent)`: returns the proposed next state, or `nil` when
  the intent is invalid or already satisfied.
- `mode`: optional `"coalescing"` (the default) or `"distinct"`.
- `operationStatus(state, intent)`: required for `"distinct"`; returns
  `"pending"`, `"accepted"`, `"rejected"`, or `"expired"` for the intent's
  stable `operationId`.
- `intentExpired(state, intent)`: optional for coalescing stores; returns
  `true` when a queued intent belongs to an old round or session generation.
- `onChange(api, state, previous, event, context)`: optional callback after an
  accepted or reconnected snapshot. `previous` is `nil` for the first valid
  snapshot. `context.initialized` distinguishes that first snapshot and
  `context.completedIntent` contains a queued intent resolved by the new
  state, and `context.completedStatus` contains its terminal status.
  `context.expiredIntents` contains any additional queued intents discarded
  by the same round or generation change as `{ intent, status = "expired" }`.
- `store:subscribe(listener)`: adds another consumer to the ordered change
 feed. Subscribers receive the same arguments as `onChange`, after the store
 has validated and installed an authoritative snapshot, or primed the initial
 local projection. Use this for HUD, effects, audio,
  or other projections so the game does not refresh them from every lifecycle
 callback or tick.
  `context.source` is `initial` for the local bootstrap projection and
  `network` for an ordered retained snapshot.
- `retrySeconds`: optional positive retry interval; the default is 0.75.

Reducers must be deterministic, idempotent, and side-effect free. Construct a
new state instead of mutating `state`. Trigger audio, effects, and UI changes
from `onChange` or subscribers, after the server has ordered the snapshot.
The initial proposal is also published once so projections can render a
complete local view while the first retained snapshot is in flight. A state
transition should be published once and consumed by each interested system;
consumers should not poll the store to discover changes.

### Coalescing and distinct operations

Use the default `coalescing` mode for “make this shared fact true” intents,
such as discovering a landmark or capturing the next relay node. In this mode,
`reduce` returning `nil` after a newer snapshot means the intent was invalid or
was already satisfied by another client.

Use `distinct` mode for operations where every accepted request matters, such
as counting visits or adding separate contributions. Every intent must carry a
stable, caller-created `operationId` (1-128 bytes). The game state must record
accepted IDs, and `operationStatus` must inspect that record. A distinct
operation is proposed at most once per queued ID, retried with the same ID
after a lost response, and removed only when its status is `accepted`,
`rejected`, or `expired`. This prevents a client from losing a valid additive
operation merely because another client's snapshot arrived first. IDs must be
unique for the lifetime of the retained channel; the SDK does not invent an
identity or provide server-side deduplication.

Coalescing stores that have round or session generations should use
`intentExpired` for stale queued intents. The SDK reports their
`completedStatus` as `expired`, so a UI can stop showing a pending action
without presenting it as accepted. Additional stale intents queued behind the
in-flight one are reported in `context.expiredIntents`.

## Lifecycle

```luau
function Game.on_start(api)
    store:start(api)
end

function Game.on_network_message(api, event)
    store:receive(api, event)
end

function Game.on_interaction(api, event)
    operation_number = operation_number + 1
    store:dispatch(api, {
        type = "score",
        -- Replace this prefix with an identity unique to this client/session.
        operationId = "unique-client-prefix:" .. tostring(operation_number),
    })
end

function Game.on_tick(api, delta_seconds)
    store:update(api, delta_seconds)
end
```

`start` is idempotent. `dispatch` queues at most 32 table intents and returns
`false` if that bound is reached. In distinct mode, dispatching an already
queued `operationId` is idempotent and returns `true`. `receive` returns `true`
when an event belongs to the store's channel. `update` advances `ageSeconds` and retries
an unanswered compare-and-set. Before the first authoritative snapshot,
`update` also repeats the initial sequence-zero proposal. If the first send or
response is lost during a connection transition, a later proposal either
initializes the channel or receives the retained state as a conflict response.

Public read-only state used by game presentation:

- `value`: the latest validated snapshot, or `nil` before initialization.
- `sequence`: its server sequence.
- `ageSeconds`: snapshot age derived from server `ageMs` and local ticks.
- `hasSnapshot`: whether a valid snapshot has arrived.

The SDK automatically serializes queued intents, rebases them after conflicts,
and resumes from retained state after reconnecting. It remains a cooperative
contract: clients still author proposed state, so it is not a cheat-resistant
competitive authority.

## Trust boundary

The server orders and retains snapshots; it does not decide whether a game rule
allowed the transition. A modified client can therefore propose an impossible
score, reward, inventory change, or completion and still win the compare-and-set
race if the game accepts that proposal locally.

Use this helper for cooperative presentation state only. Competitive scores,
valuable inventory, progression, trading, and rewards need a trusted command
handler that validates an action against authoritative state and then emits the
next state and events. The Rust `AuthorityBoundary` provides that generic
command-to-event contract for a trusted host; it does not replace the
game-owned rules or connect itself to the current WebSocket service.
