-- This fixture intentionally exercises the same lifecycle and network path
-- in native and browser hosts. Keep its output deterministic.
local Game = {}
local tick_count = 0

function Game.on_start(api)
    api.network:publish("conformance", { event = "start", tick = tick_count })
    task.defer(function()
        api.network:publish("conformance", { event = "defer", tick = tick_count })
    end)
    task.delay(0.032, function()
        api.network:publish("conformance", { event = "delay", tick = tick_count })
    end)
end

function Game.on_tick(api, delta_seconds)
    tick_count += 1
    api.network:publish("conformance", {
        event = "tick",
        tick = tick_count,
        delta = delta_seconds,
    })
end

return Game
