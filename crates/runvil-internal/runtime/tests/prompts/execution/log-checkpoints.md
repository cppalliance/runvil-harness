---
name: log_checkpoints
description: Record deterministic checkpoints across the preamble, prologue, and epilog phases
runvil: 0
---

```lua
log("preamble ran")
```

## Prepare

```lua
log("prepare started")
store.write("state.txt", "prepared")
```

```lua
log("prepare finished")
```

## Finish

```lua
log("finish started")
return "logged"
```

This prose must not reach a model.

```lua
log("unreachable epilog")
return "late"
```
