---
name: receipt
description: Calculate a receipt using a typed Rust tool
runvil: 0
uses:
  - examples/pricing
---

## Calculate

```lua
local total = tools.call('pricing/total', {unit_price_cents = 1250, quantity = 3})
return string.format('3 items: %d cents', total.total_cents)
```
