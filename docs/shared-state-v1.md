# Shared state SDK v1

Use the shared-state helper when several clients can change one retained piece
of cooperative game state.

```luau
-- @include "@cubacadabra/shared-state-v1.luau"
```

The include defines `CubaSharedState` in the generated game chunk. The SDK is
client-side Luau built on `api.network:compare_set_state`; it does not add game
rules to Rust or the backend.

## Create a store

```luau
local function initial_state()
    return { version = 1, score = 0 }
end

local function validate_state(value)
    if type(value) ~= "table"
        or value.version ~= 1
        or type(value.score) ~= "number"
        or value.score < 0
    then
        return nil
    end
    return { version = 1, score = value.score }
end

local function reduce_state(state, intent)
    if intent.type == "score" then
        return { version = 1, score = state.score + 1 }
    end
    return nil
end

local store = CubaSharedState.create({
    channel = "shared-score",
    initial = initial_state,
    validate = validate_state,
    reduce = reduce_state,
    onChange = function(api, state, previous, event, context)
        -- Update world effects, audio, and UI from accepted state here.
    end,
})
```

Configuration:

- `channel`: the retained network channel, from 1 to 64 bytes.
- `initial()`: returns the state proposed when the channel does not exist.
- `validate(value)`: returns a normalized state or `nil` to reject it.
- `reduce(state, intent)`: returns the proposed next state, or `nil` when
  the intent is invalid or already satisfied.
- `onChange(api, state, previous, event, context)`: optional callback after an
  accepted or reconnected snapshot. `previous` is `nil` for the first valid
  snapshot. `context.initialized` distinguishes that first snapshot and
  `context.completedIntent` contains a queued intent satisfied by the new
  state.
- `retrySeconds`: optional positive retry interval; the default is 0.75.

Reducers must be deterministic, idempotent, and side-effect free. Construct a
new state instead of mutating `state`. Trigger audio, effects, and UI changes
from `onChange`, after the server has ordered the snapshot.

## Lifecycle

```luau
function Game.on_start(api)
    store:start(api)
end

function Game.on_network_message(api, event)
    store:receive(api, event)
end

function Game.on_interaction(api, event)
    store:dispatch(api, { type = "score" })
end

function Game.on_tick(api, delta_seconds)
    store:update(api, delta_seconds)
end
```

`start` is idempotent. `dispatch` queues at most 32 table intents and returns
`false` if that bound is reached. `receive` returns `true` when an event
belongs to the store's channel. `update` advances `ageSeconds` and retries
an unanswered compare-and-set.

Public read-only state used by game presentation:

- `value`: the latest validated snapshot, or `nil` before initialization.
- `sequence`: its server sequence.
- `ageSeconds`: snapshot age derived from server `ageMs` and local ticks.
- `hasSnapshot`: whether a valid snapshot has arrived.

The SDK automatically serializes queued intents, rebases them after conflicts,
drops intents already satisfied by another player, and resumes from retained
state after reconnecting. It remains a cooperative contract: clients still
author proposed state, so it is not a cheat-resistant competitive authority.
