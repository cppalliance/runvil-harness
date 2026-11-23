---
name: echo
title: Echo
description: Return the text argument unchanged
runvil: 0
annotations:
  readOnlyHint: true
  openWorldHint: false
---

Returns its `text` argument verbatim, entirely in Lua, with no model call.

## Main

```lua
return args.text
```
