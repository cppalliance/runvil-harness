---
name: analyst_example
description: Demonstrate frontmatter model roles and models.use for careful model selection.
runvil: 0
models:
  strong:
    min_context: 40000
---

Demonstrates model selection by role. The frontmatter `models:` key sets a context minimum for the `strong` role, and the Host decides which model fills that role. A prompt whose `strong` model is too small is refused before it runs. A run starts on the `default` role's model, and a section that calls `models.use("strong")` runs the completions after that call on the `strong` role's model.

## Analyze

```lua
models.use("strong")
```

Analyze the following input carefully and return a short factual summary with no preamble:

{{ args }}

```lua
return models.infer(prose)
```
