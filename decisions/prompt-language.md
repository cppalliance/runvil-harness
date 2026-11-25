# The prompt language: design decisions

This page covers the prompt file as its author writes it: front matter, sections, Lua blocks and prose, the order sections run in, `jump`, `call`, `execute`, and `require`, arguments, substitution, the Lua globals, and the limits and errors an author meets. A prompt is one Markdown file. Its front matter names the prompt and declares its arguments and requirements, and each heading in its body starts a section, which is a list of prose blocks and Lua blocks. Lua before the first heading is the preamble, which is the first section. The Engine runs the top-level sections in file order, each in a fresh Lua VM. A Lua block moves the run to another section with `jump` or `call`, runs another prompt file with `execute`, loads shared code with `require`, and sends prose to a model with a call such as `models.infer`. The Engine passes the table `var` from each section to the next.

## 1. The prompt file and its front matter

### A prompt is one Markdown file with front matter and a body, and a body of one Lua block is a whole prompt

The file opens with YAML front matter between two lines of three dashes, and the rest of the file is the body. Headings in the body are optional. A body of one Lua block is a whole prompt, and a body of prose alone parses too and ends its run with the text `done`. A prompt's name is its front matter `name`, and the Engine reports the start and the end of a run under that name.

The owner's rule: "we don't require any headings at all". An author who wants a heading above the sections writes one, and the parser reads it as a section like any other. The owner's rule: "If they want to have the H1, they can have it, but they don't need it." The name is in the front matter because every prompt already states it there. The owner's reason: "the real title for the prompt comes from the front matter".

### One kind of prompt covers pipelines, tool loops, and chats

Every prompt is a Markdown file, and the same Engine code runs a fixed pipeline, a model that calls tools in a loop, and a chat with a user. The Lua in each section decides which of these the section does.

The choice belongs to the section because one prompt can combine them.

### Front matter accepts a fixed set of keys and refuses any other

The keys are `name`, `description`, `title`, `runvil`, `max_tool_iterations`, `uses`, `args`, `models`, `prompts`, and `annotations`. The parser requires `name` and `description`. It refuses every other key, so a misspelled key fails the parse with the code `frontmatter_invalid`.

The section "Arguments and substitution" covers `args:`, and the entries that follow cover `name:`, `title:`, `annotations:`, and `prompts:`. Other pages cover `uses:` ([Tools and Packs](tools-and-packs.md)), and `models:` and `max_tool_iterations` ([Model calls and conversations](models-and-conversations.md)).

### `name:` is a tool name, and `title:` is what people read

A `name` is 1 to 64 characters: an ASCII letter, then ASCII letters, digits, `_`, and `-`. Any other name fails the parse with the code `frontmatter_invalid`, as in ``invalid prompt name `research person`: expected [A-Za-z][A-Za-z0-9_-]{0,63}``, at the line and column of the value. The grammar is the one that argument names and the names of adhoc tools follow. It is stricter than the tool names that MCP accepts and fits the 64-character tool names of model providers, so a Host that publishes a prompt as a tool uses its name unchanged, wherever a client or a model sees it. The parser does not check that names are unique; that check belongs to the Host that lists them.

`title:` is optional. It is a display name, such as `title: Research a Person`, for a Host to show people, while `name` stays the identifier that events and errors use. A title that is empty or all whitespace, that holds a control character or a line break, or that starts or ends with whitespace fails the parse with the code `frontmatter_invalid`, with a message that quotes the title and names the rule it breaks, such as ``invalid title " padded ": expected no leading or trailing whitespace``. The parser refuses padding rather than trimming it, so the title a Host shows is the text the author wrote. MCP shows a tool's `title` to people and keeps its `name` for programs, so a prompt has both. MCP also accepts a title inside a tool's annotations, with a rule for which one wins. `annotations:` takes no title, so a prompt writes its title in one place.

### `annotations:` holds the MCP behavior hints, and the Engine ignores them

`annotations:` is an optional map of the keys `readOnlyHint`, `destructiveHint`, `idempotentHint`, and `openWorldHint`, each a boolean or `null`:

```yaml
annotations:
  readOnlyHint: true
  openWorldHint: true
```

The keys are camelCase where the other front matter keys are snake_case, because they are MCP's own words, as the keywords under `args:` are JSON Schema's own words. A Host that publishes a prompt as a tool copies the hints onto the tool unchanged, and the entry on `args:` in section 5 covers the tool's input schema.

A hint left out or written `null` is unset. An absent `annotations:` key, `annotations: {}`, and `annotations: null` all leave every hint unset. Unset does not mean false: MCP defaults `destructiveHint` and `openWorldHint` to true, so filling in false would change what the author left unsaid.

An unknown key such as `readonlyHint`, a key written twice, and a value other than a boolean or null, such as `yes`, fail the parse with the code `frontmatter_invalid`. A repeated key is reported where its annotations map begins; an unknown key or invalid value is reported at that key or value.

All combinations of hints are accepted and preserved. MCP defines `destructiveHint` and `idempotentHint` as meaningful only when `readOnlyHint` is false, but does not forbid writing them otherwise. They are advisory metadata, so the parser adds no restriction between hints.

The Engine never reads a hint, and the parser does not check a hint against `uses:`. A hint is the author's claim about the prompt as a whole, a section can offer tools of Packs that `uses:` leaves out, and MCP clients treat every hint as untrusted.

### `prompts:` lists the prompts that a prompt may run with `execute`

The value of `prompts:` is a list of names, as in `prompts: [review.md, summarize.md]`. Each name is the one that the caller's `PromptFinder` knows the prompt under, which is the name that `Prompt::file` returns for it ([The Engine](engine.md)). The parser refuses an empty entry and a name that the list has twice, each with the code `frontmatter_invalid`. `Prompt::parse` then finds and parses every listed prompt and every prompt that those list, so a listed prompt that is missing or malformed fails the parse of the prompt that lists it.

A list covers its own prompt alone. If A lists B and B lists C, then B may execute C and A may not, and A's list may leave C out. The parse still reads C when it parses A. The owner's reason: "if prompt A calls prompt B, and prompt B calls prompt C, then A implicitly requires C". A rule that made A name every prompt below it would be the alternative, in the owner's words: "the alternative is that every prompt has to exhaustively enumerate all its descendants which is pointless".

The names are in the front matter, where the parser reads them, because a name that Lua computed would be known only when the `execute` runs. The owner's rule: "I want to parse each of those named prompts so that we don't have to find it later". Prompts may list each other, and a prompt may list itself.

### `runvil: 0` marks a prompt, and a language change leaves the version at 0

The run's first step accepts `runvil: 0`, refuses any other number with the code `unsupported_version`, and refuses a file that lacks the key with the code `structure_invalid` and the message `not a runvil prompt: no runvil version`. It checks the run's prompt and every prompt that the parse read with it. Only the semantics of version 0 exist, and a prompt of another version would otherwise fail in the middle of its run with a confusing error.

A change to the language leaves the number at 0 and deletes the old form, which then fails as a call of nil or as an unknown key. The owner's rule: "as long as we are at 0 you will never suggest to increment it." An alias for the old form would leave two spellings in the language.

## 2. Sections, blocks, and prose

### Every heading starts a section, and the shallowest heading level is the top level

Every heading, from H1 to H6, starts a section. The top level is the shallowest heading level in the file, so in a file whose headings are H2 and H3, each H2 is a top-level section and each H3 is a child of the H2 above it. A heading is exactly one level deeper than its parent. A heading that skips a level, such as an H4 directly under an H2, fails the parse as an orphan heading. So does a first heading that is deeper than a later one: an H2 named `Run` above an H1 fails with ``section `Run` is an orphan H2 heading with no parent H1``. An empty heading fails at every level, with a message such as `an H1 section heading must not be empty`. A heading inside a block quote or a list item is prose of the section around it.

Only headings mark sections, so XML tags remain free for the author and the model to use for emphasis, nonce delimiters, and injection defense.

The top level follows the file so that a plain series of H2 sections runs in file order by fall-through. The owner's reason: "Wouldn't it be better to just let the author start with the H2? And now it's implicit, it's just a series of H2." If every prompt had an H1 as its root section, the H2 sections would be its children, and each would run only when the H1's Lua named it. The owner's reason: "every H1 that wants to chain has to have a jump, and that looks ugly".

### A section's name is its heading's text, and sibling sections have distinct names

A section's name is the text of its heading as the Markdown parser decodes it, so a heading written `## Q&amp;A` has the name `Q&A`. Sibling sections have distinct names, and the parser refuses a duplicate.

With names in raw source form, an author would have to write the entity `&amp;` in Lua to name that section.

### Lua names a section by a path, and a section is found by where it sits

A section path is a Lua string of section names. `name` names a direct child of the section the code is written in. `../name` names a sibling, another section under the same parent, and from the preamble the top-level sections are siblings. `/a/b` starts at the top-level sections and names one section for each level, so it names the same section wherever it is written. Each name matches a section's name case-sensitively, with the whitespace around the name ignored, and a name that holds a slash writes it as `\/`. `call`, `jump`, `tasks.spawn`, and `tasks.fanout` take the relative forms, `name` and `../name`, the `prompt` reads take every form, and `require` takes only a path from the top, as in `require '/helpers'`. A `jump` in a library's function differs from the library's other calls: it moves the walk of the section that is running, so its path starts at that section. A malformed path fails with a Lua error that quotes the path.

A section is found by where it sits, so the relative paths between a section and its neighbors stay the same when they all move to another depth. The calls that run a section take only the relative forms, because each of them reaches only a neighbor of the section it starts from (the entry "A section names its siblings and its direct children"). The reads and `require` leave the walk where it is, so they may name any section, and a path from the top gives a shared section one address at every depth. `require` takes that form alone so that a library has one spelling and one meaning on every line of the prompt, and a line that loads it stays correct when the line moves to another section.

### A section is an ordered list of Lua blocks and prose blocks

The parser splits a section's content into Lua blocks and prose blocks in file order. A section may have any number of Lua blocks, and the Engine runs each in order. The parser compiles every Lua block, so a syntax error fails the parse ([The Engine](engine.md)). An error message names a Lua block that opens a section the prologue and one that ends it the epilog.

### A Lua block has one accepted spelling, and a near miss fails the parse

A Lua block opens with a line of exactly three backticks and `lua` and closes with a line of exactly three backticks. The parser checks the whole body before it compiles a block, and the first violation fails the parse with the code `fence_invalid`. A label that starts with `lua` in any letter case and differs from it, such as `Lua` or `lua extra`, is a violation. So is an opening or closing line of another shape, and so is a line outside every fence that looks like a `lua` opening line. If the parser read a near miss as prose, a model would receive the Lua source as prompt text.

A fence with another label, such as `json`, is prose, because prompts show JSON and code examples to models in fences. The parser skips the lines inside another fence, so an author shows a Lua block as an example inside a fence of four backticks.

### The Markdown above a Lua block is that block's `prose`, and a thematic break discards the text above it

The parser collects the Markdown that follows a heading or a Lua block, and the next Lua block reads it as the global `prose`. A thematic break, a line of three dashes, empties the collected text, so the text above a break is commentary for human readers. Markdown after a section's last Lua block is commentary too.

A break needs a blank line above it, or Markdown reads the line above as a heading. Emptying the collected text is a break's only meaning, and this one rule explains commentary at the start of a section, before every Lua block, and after the last Lua block.

### The Engine renders the placeholders in `prose` at the block's first read

Each Lua block has its own `prose`. The Engine substitutes the `{{ }}` placeholders when the block first reads the global, and it stores the result for later reads. The Engine discards prose that a block leaves unread, bad placeholders included. An assignment to `prose` raises an error.

The Engine sends prose to a model when a Lua block passes it to a call such as `models.infer(prose)` ([Model calls and conversations](models-and-conversations.md)).

### `prompt.section` returns another section's own Markdown, and `prompt.section_or` returns nil for a miss

`prompt.section(path)` returns a section's own raw Markdown, from the line after its heading to the next heading of any level. With a second argument, a selector, the read returns one block of that Markdown as a table: the one block that the selector matches, or the match that a number in a third argument picks. A read raises an error when the section or the block is absent, and `prompt.section_or`, which takes the same arguments, returns nil for that miss.

## 3. The order sections run in

### Lua decides every move between sections

A run moves to another section when the walk, the Engine's pass over the sections in file order, falls through, when a Lua block calls `jump` or `call`, or when Lua starts a task ([Tasks and fanout](tasks-and-fanout.md)), and it moves into another prompt when a Lua block calls `execute`. `jump`, `call`, and `execute` are Lua functions, and the Engine leaves them out of every tool list. A model influences the order through data that Lua reads and branches on.

The rule exists so that a reader of the Lua can reconstruct the whole control graph, and so that every branch is a visible Lua condition.

### The code before the first heading is the preamble, and it runs first as an ordinary section

The preamble is the text between the front matter and the first heading. When it has a Lua block, it is the first top-level section: its name is the empty string, its list of children is empty, and the main walk enters it first, in a fresh VM, as it enters any section. When it is prose alone, it is commentary, and the walk starts at the first heading's section. The preamble's blocks follow the rules of every section's blocks, so the Markdown above a Lua block is that block's `prose`. A `return` in the preamble ends the run, and a preamble that runs off its end falls through to the next top-level section.

The preamble and the sections after it are entries of one chain, the main walk. The next section therefore starts with the preamble's `var`, on the model that the preamble left current, and under the limit that the preamble set with `tasks.concurrency`, and it can wait on a task that the preamble spawned. From the preamble the top-level sections are siblings, as in `jump('../Scan')`, and a child path misses with an error that ends `` the preamble has no child `Scan` ``. The preamble reports its section start and its section finish under the empty name, and an error message labels its Lua blocks by their place, as `preamble prologue`, `preamble lua 2`, and `preamble epilog`.

An uncaught Lua error in the preamble, such as a failed `assert`, ends the run with the code `lua_runtime`, as in every section, before the next section runs. An author therefore writes there, as an `assert` call, a hard check that the `args:` declaration leaves to Lua, such as the range of a number. `requirements_unmet` belongs to the run's start checks alone, and `args_invalid` belongs to the start check of the run's arguments and to an `execute` whose arguments the executed prompt refuses ([The Engine](engine.md)).

The Engine treats every section the same way, and the preamble is a section whose name is empty. The owner's rule: "I don't want any special functions for H1. I want everything to be the same." Every section can do the setup that the start of a prompt can do: tools, model calls, `call`, `execute`, and `jump` work in every section, and the main walk starts on the `default` model ([Model calls and conversations](models-and-conversations.md)). The owner's reason: "what can H1 really do in terms of setup? I'm thinking nothing."

The run starts at the first top-level section, whichever it is. The owner's reason: "every prompt has a beginning, and we simply start there". A prompt whose body is a Lua block and then a section under the heading `## Research` runs the block and then `Research`. The owner's goal: "I want this to chain without a jump".

### Top-level sections run in file order, and a nested section runs when Lua names it

The main walk enters the first top-level section, which is the preamble when the prompt has one, runs its blocks, and falls through to the next top-level section, until a block executes a `return` or the last top-level section ends. A child section runs when Lua names it with `jump`, `call`, `require`, or a task call. An H1 above a series of H2 headings makes them its children, so the walk runs the H1's blocks, and each H2 section runs only when Lua names it.

A subroutine section is therefore a child of the section that calls it, or a top-level section placed after the section whose `return` ends the run.

### Each section entry starts a fresh Lua VM, and the Engine copies `var` into it

The Engine builds a fresh Lua VM each time a chain enters a section ([The Engine](engine.md)). A chain is one sequence of section entries: the main walk, a `call`, an `execute`, or a task. A plain Lua global that one section sets reads nil in the next. The Engine copies the previous section's `var` into the fresh VM, and sections also exchange data through a returned value, the store, and the arguments of a `call`.

With these explicit channels, a section behaves the same whether the run enters it by fall-through, by `jump`, by `call`, or as a task. One VM for a whole walk would create hidden globals that depend on the path, state that is impossible to clone, and inconsistent inputs for a section.

### A `return` ends the chain, and a walk past its last section returns `done`

A `return` at the top level of a Lua block ends the chain the block runs in. A returned string, number, or boolean is the chain's result as text, and on the main walk that result is the run's result. A `return` of nil, or a bare one, ends the chain with a nil result, the way a walk past its last section ends. A returned table fails with `cannot return a table as a result`. After a block that ends without a `return`, the Engine runs the next block. The rule makes Lua control flow compact: one `return` both ends the chain and states its result.

A `return` ends the chain whatever its value, because the author wrote it to end the chain. If a nil value moved the run on to the next block, `return x` would end the chain or continue it depending on what `x` holds, and `return call(...)` would depend on what the called section did. A `return` inside a function is that function's, and it leaves the chain running.

A bare `return` and a block that runs off its end give Lua the same result, an empty list of values, so the Engine marks the difference when it compiles a block. A syntax error still reads as it does for the block's source alone, and every line of the block keeps its number.

When the main walk runs past its last section, or a `return` ends it with a nil result, the run's result is the text `done`. A prompt that lacks sections, one whose body is empty or is prose alone, is valid and ends the same way, because its walk ends as soon as it starts.

The output of a prompt is its return value: a prompt that built a file returns the file's text, as in `return store.read('report.md')`. A run has one way in, its arguments, and one way out, its result. The owner's rule: "If a prompt wants to return a file, it should return it as the string of its return value."

### `jump` ends the block and continues the walk at another section

`jump` records the section that it names as its target and ends the current block. It ends the block by raising an error, so when `pcall` catches that error, the block goes on. At the block's end the recorded jump still wins over the value that the block returns and over an error that it raises. The Engine then closes the section, looks the target up, and enters it in a fresh VM that starts with the jumper's `var`. A section may jump to an earlier sibling, so two sections can form a loop.

A section of a called chain may jump as well, so what a section can do is independent of how the run entered it. The prompt's author is responsible for coherent control flow, as in any programming language, and that includes infinite loops and wrong targets.

### A jump to a child section starts a walk at the child's level

When a section jumps to one of its direct children, the Engine walks the children from that target and falls through the target's following siblings. When the last child ends, the walk resumes at the section after the jumper.

The rules are identical at every level: the Engine's section semantics are independent of the heading level. A detour that ran only the one child and then returned would apply different fall-through rules at a child level than at the top level.

### `call` runs a contained chain and returns the text that the chain ends with, or nil

`call` names a target section, starts a new chain there, and suspends the calling block. The chain follows the rules of the walk: it falls through the target's following siblings, and its sections may jump. It ends when a block executes a `return` or the last section at its level ends. `call` returns the chain's final text as a string, and it returns nil for a chain that ends with a nil result.

A `call` works like a call of a different prompt whose sections happen to be in the same file. The caller inherits only the chain's answer, because the tokens that the chain spent would otherwise bloat the caller.

`call` returns nil for a chain that ends with a nil result because nil tells the caller how the chain ended, where the empty string would read as a reply. The caller tests the value or gives it a fallback with `or`, and a chain can still return the empty string on purpose. With nil, `return call(...)` also hands on what the called chain did: the caller's chain ends with a nil result too, where the empty string would end it with a text that the Engine supplied. A caller that joins the value to a string with `..` gets a Lua error for nil, at the line of the join. A task and a fanout arm that end with a nil result report the empty string, because their text is one field of a result that also says whether they succeeded ([Tasks and fanout](tasks-and-fanout.md)).

### A call's second argument is the called chain's `args`, and the chain starts with a copy of `var`

The second argument of `call` is optional: a table of named fields or a string. Every section of the called chain reads a table as its `args`, and reads a string as `args.text`. When the second argument is left out, the chain reads the caller's `args`. A table that is a list raises `args must be a table of named fields or a string, got a list`. The Engine passes a call's arguments on as given, unchecked and with defaults left unfilled. The chain's `var` is a deep copy of the caller's `var`, and the Engine discards the chain's writes to it when the chain ends.

The `args:` declaration describes what a caller passes the prompt, so what one section passes another is outside its scope. A string becomes the field `text` so that `args` is a table in every chain, and `text` is also the field of a prompt that omits the declaration.

### `execute` runs another prompt as a child chain of the same run

`execute(name, args)` starts a new chain in the prompt that `name` names and suspends the calling block, as `call` does. The name is one of the names under `prompts:` in the calling prompt's front matter. The chain starts at the first top-level section of the named prompt, which is the preamble when the prompt has one, and it follows the rules of the main walk. `execute` returns the chain's final text as a string, and it returns nil for a chain that ends with a nil result, by the rule of `call`. A prompt that ends with a nil result therefore gives nil to an `execute`, where a run of the same prompt ends with the text `done`.

The executed prompt is a part of the caller's run. It has the caller's scheduler, cancel flag, event log, files, and Packs, and it counts against the same limits. A task that it spawns is admitted under its caller's concurrency limit and counts toward the run's cap on idle tasks. Its chain runs one level deeper than its caller, under the one depth cap of 8 levels. Its tool calls work in the run's files under its caller's file access, and the Harness counts them toward its one cap on tool calls in flight ([The Harness and the Host API](harness-and-host.md)). The owner's reason: "you could have a sub-agent in the main prompt, and then you can have a sub-agent in the child prompt, but they all contribute to the same set of limits".

`execute` works wherever `call` works: in the preamble, in a library while it loads, and in a task. A failed child raises its error in the caller, located in the child's file, and a `pcall` around the `execute` catches it. A name outside the calling prompt's list raises an error with the code `lua_runtime` that names the prompt and what it lists, as in ``prompt `c.md` is not listed: `a.md` lists `b.md` under `prompts:` ``.

### Only the arguments go into an executed prompt, and only its text comes back

`call`, `jump`, `require`, the `prompt` reads, and the task calls look a section up in the prompt that they are written in. One section name in two prompts therefore names two sections, and a prompt reaches another prompt's sections only by executing that prompt. The owner's goal: "I just want it to be as if the other prompt sections were in the main document, but without having their section names conflict". The owner's rule: "prompts can't name sections from other prompts. You can just only execute."

The child starts with an empty `var`, on the `default` model of its own model set, and with a new, empty store ([Files and the VFS](files-and-the-vfs.md)). A `call` differs on the `var` and the model: its chain starts with a copy of the caller's `var` and on the caller's current model. The child's `args` are the arguments of the `execute`. After the `execute` returns, the caller's `var`, `args`, and current model are the same as before it. The owner's rule: "The executed prompt gets a new var, nothing comes across except args/argv".

Each prompt also keeps its own front matter. The Packs under the child's `uses:` are the declared Packs in the child's sections, and their preludes run there. A prelude runs in the sections of a prompt that declares its Pack, and in those alone, so a caller's sections get the prelude of a Pack only when the caller declares that Pack itself ([Tools and Packs](tools-and-packs.md)). A role's thinking switch comes from the `models:` of the prompt that the section belongs to, and the round cap of `models.loop` comes from that prompt's `max_tool_iterations` ([Model calls and conversations](models-and-conversations.md)).

### The executed prompt's `args:` declaration checks the arguments of an `execute`

The second argument of `execute` is optional: a table of named fields or a string, in the forms that `call` takes. A string is the one field `text`, and an `execute` that leaves the second argument out passes the empty object `{}`. The Engine checks the object against the `args:` declaration of the executed prompt and fills the declared defaults, by the rules of a run's first step. A mismatch raises an error with the code `args_invalid` at the line of the `execute`, as in ``argument `query` must be a string, got integer``, before the child starts. A caller that wants the child to read its own arguments passes them, as in `execute('review.md', args)`.

`execute` differs from `call` here: it checks the arguments, and it passes `{}` where `call` passes the caller's `args`. The declaration describes what a caller passes the prompt, and for an executed prompt the caller is the prompt that wrote the `execute`. The check is the one that a run's arguments get, so a prompt starts with arguments of the declared shape, whether a Host started it or another prompt executed it. The error is in the caller because the caller built the arguments.

### A section names its siblings and its direct children

`jump`, `call`, and the task calls look a section up in the visible set of the section that their path starts at: the other sections with the same parent, plus that section's direct children. The parent, a sibling's child, and the section itself are outside the set. A `jump` or a `call` to a section outside the set fails with an error ([The Engine](engine.md)).

With only the children in the set, two sibling sections would be unable to share one worker, and with only the top-level sections in it, a child section would be unaddressable.

## 4. Sharing code with `require`

### `require` loads a section as a library into the calling section's VM

`require` runs the Lua blocks of the section it names inside the caller's VM. The library reads the caller's globals, `var`, and `args`, and the globals it defines remain for the caller. Runvil's `require` loads only sections of the running prompt, and the Engine leaves Lua's `package` library unloaded.

Shared code exists for reuse, so a library runs only where a section asks for it. With Lua's own `require`, a prompt could load Lua files from disk, and library code would be unable to make a call that waits.

### A library runs once in each section VM, and `require` returns the value of its first `return`

A library's blocks run in order. After a block that ends without a `return`, the next block runs. The first block that executes a `return` ends the load, and `require` returns its value, which may be a table or a function, or `true` when the `return` is bare or gives nil. For a library whose blocks all end without a `return`, `require` returns `true`. Later calls in the same VM return the saved value, and a library that requires itself raises an error that begins `require cycle:`.

`require` returns a value of any type because a library returns its module as a table. A `return` ends the load the way a block's `return` ends a chain, and the result `true` in place of a nil value matches standard Lua. The cycle error exists because standard Lua 5.5 lacks cycle detection and recurses until the stack overflows.

### A library is an ordinary section, and each section requires the libraries it uses

A library section has the same syntax as any section, and the walk enters it like any other. A library that returns a table therefore fails the run there, so the author places it after the section whose `return` ends the run, or nests it under the section that uses it. The placement is the author's responsibility, which the design prefers to a special marker for library sections.

A library that a parent section loaded exists only in the parent's VM, so a called section, a task, and a fanout arm each call `require` themselves. With inheritance, the Engine would replay a parent's libraries implicitly in each child VM.

### A library may call a model while it loads, and `jump` raises during a load

`require` is a Lua function, so a library's blocks run inside the calling block's coroutine ([The Engine](engine.md)) and may call a model, the store, and tools while they load. Library code is ordinary section code that runs in another place. A `jump` during a load raises an error that begins `jump is unavailable while a library loads`, because a transfer of control in the middle of a load lacks a coherent meaning.

The parser leaves `require` calls unchecked, so a `require` of a missing section raises an error when the call runs. A check at parse time would need each block's syntax tree, which the Lua compiler leaves unexposed, and a search of the text would misfire on strings and comments.

## 5. Arguments and substitution

### The arguments of a run are one JSON object, and the run's first step checks them

A caller starts a run with the text of one JSON object, and a caller that leaves every argument out sends `{}`. The run's first step checks that text directly after the version ([The Engine](engine.md)). Malformed JSON, a JSON value other than an object, and an object that differs from the prompt's declaration each end the run there, before any section runs, with the code `args_invalid`. The error names the first mismatch, as in ``argument `query` must be a string, got integer``. The error leaves its location out, because the fault is in the caller's text, outside the lines of the prompt.

The arguments are an object so that a Host can publish any prompt as a tool: the arguments of an MCP tool call are a JSON object. The owner's reason: "we can always put it behind an MCP server". The Engine makes the check, so a run starts with arguments of the declared shape, and a prompt leaves to the Engine every check for a missing field or a wrong type in what its caller passed. The arguments that a `call` or a task spawn gives a chain inside the prompt are unchecked (the entry "A call's second argument is the called chain's `args`, and the chain starts with a copy of `var`").

### `args:` declares the object in a subset of JSON Schema, and the default is one optional string named `text`

The value of `args:` is a schema whose keywords are `type`, `properties`, `required`, `additionalProperties`, `items`, `enum`, `description`, and `default`. A `type` is `string`, `boolean`, `integer`, `number`, `array`, or `object`, and the root is an object whose `properties` name the arguments. The parser refuses any other keyword, a node that lacks a `type`, an array that lacks `items`, and a default that differs from its own node. Each refusal has the code `frontmatter_invalid` and names the YAML path of the node, such as `args.properties.query`. An object is closed: a field outside its `properties` ends the run, unless the object says `additionalProperties: true`. Before the check, the Engine gives each missing field its `default`, at every depth. A `null` for a field outside the `required` list counts as the field left out. A prompt that omits the key takes one optional string named `text`, so a Host with plain text sends `{"text":"hello"}`.

The declaration is JSON Schema because that is the form in which a tool publishes its arguments, and `ArgsSchema::as_json` returns the declaration in that form. It is a subset so that an unknown keyword fails the parse: a misspelled keyword is an error when the prompt loads, where a reader of full JSON Schema would ignore it. An object is closed so that the Engine refuses a misspelled optional field, which an open object would accept and the prompt would leave unread. The Engine fills a default because a caller that reads the schema expects the prompt to use it. A `null` counts as a field left out because models send `null` for a field they omit, and the checker of the `std` Pack reads a `null` the same way ([Tools and Packs](tools-and-packs.md)).

The owner's rule: "there must be no freeform prompts". The default field is named `text` because that is the word for what a chain returns: a chain ends with its final text.

### `args` is the checked object, a read-only table in every section

Every section VM has `args` behind the guard on the globals table. It holds the object that passed the check, with its defaults, so it is a table in every section, and a field that the caller left out and that lacks a default reads nil. Each table inside it is a read-only view. An assignment to the global raises ``args is read-only: assign to `var` or a section global instead``, and a write to a field named `x` raises `args is read-only: cannot set field 'x'`. The preamble, a called chain, an executed prompt, and a task follow the same rule. A null inside an object that takes undeclared fields reads as `json.null`. A prompt that needs a different value builds it in Lua and keeps it in `var`.

Apart from writes, a view behaves like the table it stands for. `#`, `pairs`, `next`, `ipairs`, `table.concat`, and `table.unpack` read the data. A view that Lua hands to `tools.call`, to `tasks.fanout`, to a spawn's `item`, or to `var` arrives there as its data.

`args` always holds what the caller passed, so a reader of any section knows where its value came from. The name is `args` because it can only mean what the caller passed, where "input" could also mean what a user sends during the run. One rule for every section also means one way to install `args` in a VM. The owner's reason: "I value uniformity over everything else." A view is an empty stand-in for its table because Lua calls `__newindex` only for a key that a table lacks, so a table that held its own entries would accept a write to each of them.

### A placeholder names data in `args`, `item`, `var`, `sys`, or a Lua global

A placeholder is a path between double braces, such as `{{ var.greeting }}`. The first segment selects the source: `args` for the arguments, `item` for the member a task or a fanout arm runs for, `var`, or `sys`. Any other first segment reads the Lua global of that name. The segments after the first are keys, as in `{{ args.query }}`. A string renders as written, a number as text, and a table as compact JSON, so `{{ args }}` renders the whole object.

A missing key, a null value, or a path that ends at a function raises an error at the read of `prose`, and an uncaught one ends the run with the code `substitution`. The owner's rule: "No function calls in {{ }}." Substitution is data only: a placeholder that called a function would let prose invoke tools.

## 6. The Lua environment

### A section VM runs Lua 5.5 with the `string`, `table`, `math`, and `utf8` libraries

The Engine removes base globals, among them `load`, `dofile`, `rawget`, `rawset`, `collectgarbage`, and `print`. The names `io`, `os`, `package`, `debug`, and `coroutine` read nil.

The libraries include `utf8` because it is pure, and an author needs it to cut a string at a character boundary.

### `math.random` draws from the run's seed and the section entry's id

The Engine seeds the generator of each section VM from the run's seed and `sys.id`.
The draws of `math.random` therefore differ from one run to the next and from one section entry to the next, and they repeat in a run with the same seed.
`math.randomseed` with integer arguments seeds the generator as stock Lua does, and with no argument it seeds the generator from its own next draws.
A Pack prelude and a library that `require` loads draw from the generator of the VM they run in.
The generator is Lua's own, which is not cryptographic, so a draw is not a secret.

Stock Lua seeds the generator from the clock and a memory address, when the `math` library loads and at each `math.randomseed` with no argument.
A draw would then differ between a run and a second run with the same prompt, arguments, params, and answers, which the Engine requires to produce the same result ([The Engine](engine.md)).
The section entry's id is a part of the seed because every VM of a run would otherwise draw the same numbers.
The Engine derives the seed of a generator with SHA-256, as it derives the nonce, so the draws give back neither the run's seed nor the nonce ([Tools and Packs](tools-and-packs.md)).

### `table.sort` is stable, and its result depends only on the list and the comparison

The Engine supplies its own `table.sort`, a merge sort.
Elements that compare equal keep the order they had, so two runs sort a list the same way.
The list must be a table, where stock Lua also takes a value that has the `__index`, `__newindex`, and `__len` metamethods.
The default order is by `<`, as in stock Lua, and no comparison function raises `invalid order function for sorting`.
The sort reads every element once, compares, and then writes every element once, so a sort whose comparison fails leaves the list as it was.
The sort is Lua code, so it is slower than stock Lua's C function, and it holds more arrays of the list's length while it runs.

Stock Lua's `table.sort` is a quicksort that takes a pivot from the clock and a memory address after an unbalanced partition.
Elements that compare equal then end in an order that differs between a run and a second run with the same inputs, which the Engine requires to produce the same result ([The Engine](engine.md)).
A merge sort chooses nothing, and the stable order is the one order that the list and the comparison define by themselves.

### `tostring` and `string.format` print a table, function, thread, or userdata by its name

`tostring` gives a table, a function, a thread, or a userdata that has no `__tostring` as its name alone: `table`, `function`, `thread`, `userdata`, or the `__name` of its metatable.
A `__name` that holds a zero byte counts up to that byte, as in stock Lua.
`%s` in `string.format` prints such a value the same way, and `%p` raises an error.
A `__close` handler receives a failure of an Engine function before `pcall` returns it as a table, and `tostring` and `%s` print it there as the message of that table.
A failure made from such a raised value reads `error object is a table value`, with the value's name in place of `table`.
That covers a block that raises the value and does not catch it, and a metamethod that raises it while an Engine function such as `pairs` runs it.
The Engine recognizes the value by the text Lua made of it, which ends in a name, `: `, and an address.
A `__name` that is empty, or that holds a `: ` or a line break, keeps its text, and `(address)` takes the place of the address.
An error string that ends the same way changes too: on Linux and macOS, `error('size: 0x1f', 0)` reads `error object is a size value`, and `error('bad byte: 0x1f')` ends in `bad byte: (address)`.
A hex number with more digits than a memory address has, 16 on a 64-bit machine, is no address and stays.
Windows prints an address as uppercase hex digits at the full width with no `0x` in front, so there those two strings stay, and `error('size: 000000000000001F', 0)` reads `error object is a size value`.
The Engine cannot tell such a string from a raised value, and it changes both so that no address stays in a failure.
Every other value prints as in stock Lua.
Two tables therefore print the same text, and code that must tell them apart compares them with `==`.

Stock Lua prints such a value with its address, as in `table: 0x55d0c0a1e2f0`, and `%p` prints nothing else.
The address is where the allocator put the value, so it differs between a run and a second run with the same inputs, and so does every text that holds it.
The Lua binding's own text for a failure holds the address of a table that a metamethod raised inside an Engine function, so the failure prints as its message.

### The Engine installs a fixed set of globals and reserves their names

The Engine globals that are the data of the run are `args`, `item`, `messages`, `prose`, `sys`, and `var`. The tables of operations are `compactors`, `json`, `markdown`, `models`, `prompt`, `store`, `tasks`, and `tools`. The functions are `call`, `execute`, `jump`, `log`, `require`, and `untrusted`. One global exists only in some VMs: `item`, in the first section of a task whose spawn supplied one. The reserved names are these globals, the Lua globals that the sandbox leaves in place, and the keywords of Lua 5.5. A Pack prelude, the Lua that the Engine runs for a Pack in each section VM, fails to install when it defines a reserved name.

Other pages cover `models`, `messages`, and `compactors` ([Model calls and conversations](models-and-conversations.md)), `tools` and `untrusted` ([Tools and Packs](tools-and-packs.md)), `tasks` and `item` ([Tasks and fanout](tasks-and-fanout.md)), and `store` ([Files and the VFS](files-and-the-vfs.md)).

The functions of `markdown` work over Markdown text from outside the prompt's own sections. `markdown.parse` reads text into a list of blocks, which are plain tables. `markdown.section` returns the one section that a path names, or one block of it, and raises when it is absent, and `markdown.section_or` returns nil for that miss. `markdown.select` collects every block that a selector matches, and `markdown.render` writes the blocks back as one text.

### A handle is a plain value, and its operations are functions of a namespace table

A model handle, a tool object, and a task handle are values with fields. Each operation on one is a function of a namespace table that takes the handle as an argument, as in `models.infer(handle, text)` and `tools.call(tool, arguments)`. The message list is the one exception: its builders are colon methods, as in `messages:user(text)`.

The owner's reason: "Simpler implementation. Simpler mental model for authors. This is best for AI because only one way to do things." [Model calls and conversations](models-and-conversations.md) covers model handles.

### `var` is a guarded table of JSON data

`var` is an empty proxy table over a hidden data table, and each table that an author reads from it is a view of the same kind. The proxy checks each write at the assigning line, by the rules of `json.encode`: a function, a userdata, or a thread raises an error that says the value `must be JSON data`, and a table that mixes list items and named keys raises an error that names it. A write that would break its table's shape as a list or an object raises too, such as a boolean key, a named field in a list, or an index past the end of a list. A view measures and iterates like the table it stands for: `#` gives the length of the data, `pairs` and `next` visit its entries, and `var.items[#var.items + 1] = x` appends to a list.

The proxy is empty because Lua calls `__newindex` only for a key that a table lacks, so only an empty table sees every write. The guard refuses a write that `var` cannot hold as JSON data, where the write is made, because the read-back of `var` at the end of the block would otherwise fail on it or silently drop a part of it. The Engine sets `__len` on each view, and its own `pairs` and `next` read through one, so that an author treats a `var` table as a plain table.

### `sys` is a sealed table of facts about the run and the section entry

`sys.when` is the run's start time, `sys.id` is the section entry's id, such as `0.0` for the first section that the main walk enters ([Tasks and fanout](tasks-and-fanout.md)), `sys.taskid` is the enclosing task, `sys.section_name` is the name of the running section, which is the empty string in the preamble, and `sys.section_count` is the number of top-level sections of the section's own prompt, the preamble included. `sys.index` exists in a task's first section when the spawn supplied an index, and `sys.model` exists after the entry's first tool call. A read of any other field raises an error that begins `unknown sys field`, and a write raises one that begins `sys is read-only`.

The table refuses writes because `sys` is runtime metadata: the Engine writes it, and an author only reads it. The Engine reads the start time from its caller ([The Engine](engine.md)).

### `log` records one line of at most 256 characters, and the Engine removes `print`

`log(message)` takes exactly one string: a single line of at most 256 characters. A newline or a control character in it raises an error. The Engine reports the string as an `Event::Lua` under the current section. A section VM accepts 1024 calls by default and 262,144 bytes of text across them, and a call past either limit raises an error with the code `lua_quota`.

The limits loosen only after examples from real prompt debugging establish a need for multiline or structured values.

### `pairs` and `next` visit keys in a fixed order

The Engine supplies its own versions of `pairs` and `next`.
Each visits the keys that are whole numbers from 1 up in rising order, then the other keys sorted: booleans, then the other numbers, then strings.
So a list comes out in index order, before any named field.
The place of a key depends on the key alone, so two runs walk a table the same way, and a table with a gap in its indices too.
A loop of `next` may clear keys as it goes: every other key keeps its place, and the loop visits each key that is left once.
Each raises an error for a table that has a table, a function, a userdata, or a thread as a key.
The error names the type of such a key, and of several types the first by name.
Such a table still reads and writes by key, so a set of objects works as a lookup, and a block that must walk the objects keeps them in a list as well.
Each also raises an error for a weak table, one whose metatable has a `__mode` with `k` or `v` in it, and a weak table still reads and writes by key.
Each reads through a view, so a walk of a `var` table or of a table inside `args` visits the entries of the data in that order, and the value it yields for a nested table is a view too. A `__pairs` metamethod still applies, on a table and on a userdata.

Lua leaves the order of hash keys open, and the Lua binding sets the hash seed itself. The sort makes the order independent of that internal hash state.
The order does not use the length of the table, because the length of a table with a gap differs between two runs, as the next entry says.
A key that is a table, a function, a userdata, or a thread has nothing to sort by except its memory address, which differs between a run and a second run with the same inputs.
A walk in the order of the addresses would give two such runs different results, and a walk that left those keys out would lose data without an error.
Lua's collector takes an entry out of a weak table at a moment that a walk does not choose, and it keeps the entry that the walk reads at that moment.
Which entry that is follows the order of Lua's own walk, so two such runs would find different entries.

### The length of a table with a gap in its list differs between runs

`#t` is the number of items of a list, a table that holds a value at every index from 1 up to its last.
For a table with a gap in those indices, `#t` is an index that holds a value and whose next index holds none, and two runs with the same inputs can get different ones.
`table.insert`, `table.remove`, `table.concat`, `table.unpack`, and `table.sort` measure a table by `#t`.
The Engine measures such a table by `#t` too when it converts the table to data, such as the arguments of `tools.call`, the `item` of a spawn, or a global that prose names, and the items past the length are left out.
A block that needs the same result in every run keeps its lists without a gap.
`pairs`, `next`, and `ipairs` do not use the length, `json.encode` raises an error that names the missing item, and `tasks.fanout` ends the list at the first gap.
`tools.offer` refuses such a table for the same key in every run, with one exception: a table with no `__len` whose `__index` gives a value at the gap is measured by `#t`, so one run can accept it and another refuse it.

`#` is an operator of Lua and not a function, so the Engine cannot supply its own version of it as it does for `pairs`.
Lua finds the end of such a list with a search that starts from the hash seed, and the Lua binding sets that seed itself.
A list without a gap has one end, so its length is the same in every run.

### An error for one entry of a table names the entry whose key comes first

Some functions check every entry of a table that an author passes: the options of `models.use`, the short form of the parameters of `tools.adhoc`, the array of `tools.offer`, and the keyed part of a `tasks.fanout` collection.
The Engine checks the globals that a Pack's prelude defines the same way.
When several entries fail the check, the error is the one for the entry whose key comes first: booleans, then numbers, then strings in byte order, then any other key by the name of its type.

The Engine reads such a table in the order of Lua's own walk, which differs between a run and a second run with the same inputs.
A check that stopped at the first entry it read would give the two runs different error texts, and a block that catches the error can act on its text.

### One guard on `_G` answers reads of `args`, `prose`, and `messages`, and `setmetatable` refuses finalizers

The Engine puts one metatable on the globals table. It answers reads of `args`, `prose`, and `messages` and raises on an assignment to any of them. An author's `setmetatable(_G, mt)` records `mt` behind the guard, so the documented pattern of default values for globals works. Author code can overwrite every other Engine global for the rest of that VM.

`setmetatable` raises for a metatable with a `__gc` field. Lua runs finalizers with hooks disabled, so a finalizer that loops would continue to run after a cancel.

### `json` is a global table with `json.null` for null and `json.array` for lists

The members of `json` are `encode`, `encode_lines`, `decode`, `decode_lines`, `null`, and `array`. `json.decode` returns fresh writable tables and returns `json.null` for each JSON null. `json.null` is truthy, and code tests for it with `v == json.null`. An unmarked empty table encodes as `{}`, and `json.array()` returns an empty table that encodes as `[]`. The `_lines` functions read and write JSON Lines.

The table is always present because `require` loads only sections of the running prompt. Null is a sentinel value so that a key whose value is null remains in its table. A sequence encodes as an array, so only the empty array needs the `json.array()` marker.

### `json.encode` raises for a value outside JSON and names its path

`json.encode` raises for a function, a thread, a userdata other than `json.null`, NaN, an infinity, a table that mixes list items and named keys, and a list with a gap. The message names the place, as in `value.items[3]`. The encoder writes keys sorted, so the same value produces the same text and the same error on every run. The option `{ debug = true }` writes a placeholder string such as `"[function]"` for each refused value.

The Engine walks the value itself, because the Lua binding's own conversion silently drops keys from mixed tables and leaves the path out of its errors. The walk reads through sealed tables such as `var`, because `json.encode(var.items)` would otherwise produce `{}` silently.

## 7. Errors and limits

### Lua catches every Engine failure as a table with `kind`, `code`, and `message`

`pcall` returns a failure as a table whose `tostring` is its message. `kind` is a string that an author branches on, such as `lua`, `store`, `tool`, `model_failure`, and `canceled`. `code` is the tag of the exact failure, such as `lua_quota`, and an uncaught error ends the run with it ([The Engine](engine.md)). `message` starts with the prompt's file and line when the failure has a location.

The table prints as its message, so code that prints a caught failure shows the text, and code that branches reads `kind`.

### A cancel passes through `pcall`, and a stop raises an error that `pcall` catches

When the Host cancels a run, the Engine's `pcall` and `xpcall` raise a caught failure again, and `xpcall` skips the author's message handler. A Lua block may run any number of instructions, and a hook reads the cancel flag every 10,000 instructions ([The Engine](engine.md)). A cancel passes through `pcall` because cancellation is the only way to stop a runaway loop.

A stop drops the model round or the tool call in flight and leaves the cancel flag clear. The waiting Lua call raises an error of kind `canceled`, and a `pcall` around it catches the error. The built-in chat prompt wraps `models.loop` in `pcall`, so a stop ends one reply and the chat continues ([The Harness and the Host API](harness-and-host.md)).

### A call that waits raises an error inside a callback of a C function

A call that waits, such as `tools.call`, yields the block's coroutine, and Lua refuses a yield that would cross a C function.
Lua's `string.gsub` is a C function, so a waiting call inside its replacement function raises Lua's own error, `attempt to yield across a C-call boundary`.
`table.sort` is the Engine's own Lua function, so a waiting call inside its comparison function works.

### `call`, `execute`, tasks, and fanout arms nest at most 8 levels

A `call`, an `execute`, a task, and a fanout arm each run one level deeper than the chain that started them, and a `jump` runs at the jumper's level. A start at level 9 fails with the code `call_depth_exceeded` ([Tasks and fanout](tasks-and-fanout.md)). The levels of one run count together, whichever prompt a level runs in. Two prompts that execute each other, and a prompt that lists itself and executes itself, therefore stop at the cap. The owner's answer on recursion between prompts: "cap covers it, yes".

The nesting cap and the round cap of `models.loop` are backstops: a prompt ends through its explicit Lua conditions. `jump` calls are unlimited in number, so a loop of `jump` calls ends by a Lua condition or by a cancel.

### One `json` or `var` conversion has a nesting limit and a size budget

The limits are fixed numbers:

- `json.encode` and `json.decode` accept 127 levels of nesting.
- `json.decode` accepts 8,388,608 bytes of input.
- One `json.encode` call or one write to `var` may visit as many values as one eighth of the VM's memory limit in bytes, which is 8,388,608 values at the default of 64 MiB.
- One path of an encoded value may have 100,000 keys that are strings or tables.

The nesting limit is 127 because `serde_json`, which parses the input of `json.decode`, refuses the 128th nested array or object, and the encoder writes only a depth that the decoder accepts. The size budget exists for tables that are small in Lua and huge once converted: a conversion runs inside one Rust call, outside the instruction hook and outside the limit on the Lua heap.

## 8. Planned work

- **An `execute` that returns at once.** A Lua function, planned as `execute_async`, starts a listed prompt as a task and returns the task's handle. Today `execute` suspends its caller until the executed prompt ends.
- **Lua module files and Markdown includes.** A `require` of a Lua file by its path, as in `require("./path.lua")`, and an `include` for partial Markdown. Today `require` loads only sections of the running prompt.
- **Front matter in Lua.** A sealed table with the prompt's own `name`, `args`, and `models` declarations. Today the functions of the `prompt` table read sections.
- **A live clock.** A way for Lua to read the current time during a run. Today `sys.when` is the run's start time.
- **More of `json`.** Encoding of message records, pretty printing, and a decode that builds its values inside the VM's memory limit. Today `json.encode` raises for a message record, as for every userdata other than `json.null`, and `json.decode` accepts 8,388,608 bytes of input.
- **A structured result.** An `output:` key that declares the JSON object a prompt returns, for a Host that publishes the prompt as a tool with an output schema. Today a prompt's result is text.
- **An acyclic section order.** A rule that `jump` names only a child or a later sibling. Today a section may jump to an earlier sibling, so two sections can form a loop, and the guide teaches that loop as a pattern. The rule would turn the pattern into an error, so it stays an idea for now.
