# Prompt loader

This crate loads a Markdown prompt into a ready-to-run tree of frontmatter, sections, and compiled Lua code, and it reports each failure with a classified error. Every Lua fence in the body compiles here, so a loaded prompt cannot fail on a syntax error partway through a run. The frontmatter's `args` key declares the one JSON object a caller passes the prompt, and the crate checks a caller's arguments against that declaration for the executor. It does no execution.
