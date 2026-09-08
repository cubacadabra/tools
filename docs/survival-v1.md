# Survival SDK v1

The runtime owns gravity, fall detection, hazards, health, and respawn. A game
manifest declares those rules; the Luau helper only keeps lifecycle state for a
game-owned HUD or objective.

```luau
-- @include "@cubacadabra/survival-v1.luau"

local survival = CubaSurvival.create({})

function Game.on_player_event(api, event)
    survival:handle(api, event)
    local status = survival:status()
end
```

The helper begins in `waiting` state and becomes authoritative when it receives
the runtime `spawn` event. `status()` returns a named table with `state`,
`health`, `maxHealth`, `deaths`, and `source`. `damage` events expose `source`,
`amount`, `health`, and `maxHealth`; death and respawn events carry the same
authoritative health fields. Shelter timers, healing, inventory, weather,
crafting, and victory conditions remain specific to the game.
