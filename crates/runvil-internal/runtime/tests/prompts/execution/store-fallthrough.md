---
name: store_fallthrough
description: Write store state in one section and read it in the next
runvil: 0
---

## Write

```lua
log("writing state")
store.write("handoff.txt", args.text)
```

## Read

```lua
log("reading state")
return store.read("handoff.txt")
```
