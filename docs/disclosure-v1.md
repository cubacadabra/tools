# Disclosure SDK v1

Use this helper when a compact game-owned control should reveal and hide one
or more optional UI nodes. The game still owns the document, copy, styles, and
placement; the helper only owns the open state and tap handling.

```luau
-- @include "@cubacadabra/disclosure-v1.luau"

local objective = CubaDisclosure.create({
    action = "objective.toggle",
    triggerId = "objective-button",
    nodeIds = { "objective-detail" },
})
```

After installing the UI document, synchronize its authored visibility:

```luau
api.ui:set_document(GameUIDocument.create())
objective:sync(api)
```

Pass UI events to `handle`. It returns `true` when it consumed the configured
activate action:

```luau
function game.on_ui_event(api, event)
    if objective:handle(api, event) then
        return
    end
end
```

`set_open(api, boolean)` allows game rules to open or close the same nodes
directly. `triggerId` is optional; when present, its checked state tracks the
disclosure state. A disclosure supports 1-32 node ids. Names are limited to 64
bytes to match the retained UI's bounded identifiers.
