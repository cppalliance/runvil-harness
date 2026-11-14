---
name: greet
description: Greet a name using a Lua-computed value
runvil: 0
args:
  type: object
  properties:
    name: { type: string, description: The name to greet }
  required: [name]
---

Computes a greeting for its `name` argument in Lua, substitutes it into the prose, and has the model echo it.

## Main

```lua
var.greeting = "Hello, " .. args.name .. "!"
```

Repeat exactly, with no extra words: {{ var.greeting }}

```lua
return models.infer(prose)
```
