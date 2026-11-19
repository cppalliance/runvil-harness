---
name: messages
description: Receive two messages from the Host
runvil: 0
uses:
  - user
---

## Listen

```lua
local name = tasks.receive()
local goal = tasks.receive()
return name .. ' wants to ' .. goal .. '.'
```
