# Files and the VFS: design decisions

This page covers the `store` Lua API, the virtual file system (VFS), real and virtual roots, what a run may read and write, file identities and conflicts, and persistence. A run's files are in a virtual file system, the VFS, which the crate `runvil-vfs` implements. The Host builds one `VfsRef` handle for each run: a table of mounts, each a backend in memory or over a real directory, which holds the files the run's tools work in. The store is a separate value that the Engine makes for each run, and each `execute` gives the prompt that it runs a store of its own. Lua reads and writes a store through the `store` table. The main walk, each task, and each tool call has an `Access`, an object that performs file operations for one identity: it checks the path and the policy of its handle, and then calls the backend. A `store` call runs on a view of the store, an `Access` with the calling chain's identity that reaches the store alone, under the store's own policy. A view also checks what the other identities have read and written in its store, which an `Access` to the Host's files leaves unchecked.

## 1. The `store` table

### Every Lua block has a `store` table over the files that a prompt's sections and tasks share

The Engine installs a `store` table in the Lua VM of every section entry. The preamble, the Lua before a prompt's first heading, is a section entry like the others ([The prompt language](prompt-language.md)). The table's functions read and write the store, a set of text files that all the sections and tasks of one prompt share while it runs. The run's own prompt has the store that the run starts with, and a prompt that an `execute` runs has a store of its own for that execution. A tool call works in the files of the run's handle, and the files of every store are out of its reach.

The owner's reason: "Store is always present, its a core feature of the Runvil language because its the way to move data between sections and handle fanout."

The store holds files because a prompt is often a pipeline: a stage runs inference over the files of the stage before it and writes new files. The owner's description: "there's files created at every stage of the pipeline". The files stay for the whole run so that a person can follow the data from stage to stage. The owner's reason: "we want those files to persist during the run so that they can be inspected". The owner's convention for a prompt author is to write a new file at each stage and leave the earlier one as it is: "You want to create a new one from an old one, and this way you can inspect the data flow." The store leaves the convention to the author: `write` replaces a file, and `append` and `str_replace` change one. The files are text because, in the owner's words, "Markdown is the language of AI". State has to live outside Lua, because the Engine discards a section's VM when the section ends, and `var` passes values to the next section in a form that converts as JSON. The owner's reason for files beside `var`: "JSON's unsatisfying because it's not Markdown".

The inspection that the owner means happens during the run, at a breakpoint or at any stage. With the evidence in file A and the inference over it in file B, "if we get a breakpoint now we can see A and B", where a B written as a series of edits to A would leave only the event log to show the earlier text. The owner's reason: "Having it in the store means there's a blob there." A debugger is still planned work: one would rebuild each store from the run's records, and today the record is the only view of a store from outside the run.

### The `store` table has a fixed set of functions, and a prompt composes the rest

The functions are `write`, `append`, `read`, `read_numbered`, `str_replace`, `delete`, `glob`, and `exists`. `store.str_replace` replaces an anchor text that must occur exactly once, because an edit by anchor is the pattern that works for models. `store.delete` succeeds on an absent path.

The table stops at these functions because a prompt can write the other VFS operations from them. It leaves out `stat` because timestamps are nondeterministic, and the store's memory backend keeps a file's content alone.

### `store.read` takes 1-based inclusive line bounds, and `store.read_numbered` numbers the lines

`store.read(path, start, end)` returns lines `start` to `end`, and `store.read(path)` returns the whole file. An `end` past the last line means the last line, and a `start` below 1 raises `invalid_range`. `store.read_numbered` takes the same bounds and prefixes each line with its number in the file, as in `12| text`.

The second bound is an end line because a count would force `end - start + 1` arithmetic at every call site, a documented failure mode of small models.

### The store view accepts relative, nested paths and checks each against strict rules

A store path is relative to the store root, as in `drafts/plan.md`, and a write creates the missing parent directories. The store view checks each path first. A path has 1 to 1024 bytes, and the view refuses a leading `/`, a backslash, a colon, a `.` or `..` segment, a name that ends in a dot or a space, and a Windows device name such as `COM0`. A refused path raises `invalid_path`, and `err.rule` names the broken rule.

The backslash, colon, suffix, and device-name rules refuse spellings that some backend would read as a different file.

## 2. The virtual file system

### The file system types are Runvil's own, and a run's tool files and its store are separate values built from them

`runvil-vfs` defines the file system types. A backend implements the traits `Vfs`, for the storage, and `VfsAccess`, for one identity's session with it. `VfsRef` is a cloneable handle to a set of mounted backends, and an `Access` performs operations through it for one identity. A Host builds a `VfsRef` from these types for the files that a run's tools work in. A store is a value of its own, built from the same types: an empty set of files in memory under a claims table of its own, outside every `VfsRef`.

A store is built from the file system types because it keeps their claims rules. The owner's reason: "We need the claim system that the VFS has."

The `vfs` module of each facade crate, `runvil` and `runvil-engine`, exports the public items of `runvil-vfs` except the store type, so a Host can build the file system that its runs' tools work in and can add a backend of its own. `runvil-vfs` alone exports the store type, because holding a store is the whole authority over it.

### A store call is one `Access` call on a view of the calling chain's store

For each `store` call the Engine derives a store view from the calling chain's store and the calling chain's `Access`. The view is the only way into a store. It is a second `Access` with the chain's identity and scope, and its operations reach that store alone. It checks each path against the store's path rules and gives error paths back in the form the caller wrote.

The store follows this rule: it offers a subset of the operations of the VFS. A store is a value of its own, where it could have been a mount of a handle, because one scope has to hold many stores. A child prompt that `execute` runs gets a store of its own, and it runs in its caller's scope under its caller's identity. The owner's rule: "prompts passed to execute or execute_async should get their own store which is completely isolated from the caller's store. they cannot see each others' paths or claims."

### One list of operations serves the `store` functions and a tool's file requests

`runvil-vfs` defines the file operations as values. A `VfsOp` is one operation with its arguments, `Access::perform` runs it on an `Access`, and a `VfsOutcome` is what it returns. The `store` functions use `Write`, `Append`, `Read`, `ReadNumbered`, `StrReplace`, `Delete`, `Glob`, and `Exists`, and each `store` call is one of them. The list also has the operations that a tool asks for through the file functions of the C contract: `Stat`, `List`, `ReadRange`, `Mkdir`, `Remove`, `Rename`, and `Copy`, beside `Write` and `Append`. A Rust Pack's tool may ask for any operation of the list. `Write` and `Append` hold bytes, because a tool writes bytes, and a `store` call passes the bytes of its text. `Delete` succeeds on an absent path, and `Remove` fails on one with `VfsError::NotFound`. They are separate operations, so a run's record shows which one ran. `VfsOp` and `VfsOutcome` are closed enums, so an operation or an outcome that the list gains fails the build at each place that must handle it ([The Engine](engine.md)).

The owner's reason for one list: "in principle, the store is just a virtual directory in the file system that is unique to a prompt." A `store` call and a tool's file request are two views of the same operations, and the Engine and the Harness handle the whole list, so either side gains an operation by adding a function. Each of the two reaches the list as a `Vfs` effect, which says who asked ([The Engine](engine.md)).

### Virtual paths have one POSIX form on every operating system

A virtual path is rooted and uses forward slashes on every operating system. An `Access` canonicalizes each path when it receives it, so the policy, the claims table, and the backend see one spelling. A path without a leading `/` is relative to `/`.

With one canonical form, only `RealBackend` translates a path to the operating system's form. Case is significant. A claims table has an exception: below a rooted real mount on Windows and macOS it compares paths in lowercase, because a false conflict fails safe. That exception is unused while the Host's files are unchecked, because a store is memory (see Identities and conflicts).

### `VfsRefBuilder::build` fixes a handle's mount table, and the longest matching prefix wins

`VfsRefBuilder::mount` installs a backend at a path prefix, and `build` returns the handle. The handle sends each operation to the mount with the longest matching prefix. `VfsRef::overlay` returns another handle with an extra mount above the first, under the first handle's policy and operation sink.

A table fixed at construction makes routing immutable and a handle cheap to clone for each run.

### The backend traits are synchronous and work on bytes

Every method of `Vfs` and `VfsAccess` is a blocking call, file content is bytes, and `Access` adds the text operations. A `VfsRef` locks its backend for each call, so a backend must be `Send` and may leave locking to the handle.

The traits are synchronous because the backends are synchronous by nature: memory and the standard library's file system calls. Lua is synchronous as well, and that is beside the point. The owner's reason: "at the end of the day, the operating system calls are synchronous. So we keep the storage operations synchronous." `Access` is synchronous for the same reason.

## 3. Real and virtual roots

### The Host builds the file system that a run's tools work in

A Host sets `RunSetup.vfs` to one `VfsRef` for each run: the file system that the run's tool calls work in ([The Harness and the Host API](harness-and-host.md)). The Harness passes the handle to the Engine with `RunParams::with_vfs`, and the Engine makes an `Access` to it for each tool call. `RunSetup::new` sets a default `VfsRef` that holds an empty in-memory mount at `/`. A Host can fill the handle before a run and read it after the run, through its own clone of the `VfsRef`, and concurrent runs can share one handle. When the handle refuses the run its access, or refuses the fork for a task or a tool call, the failure has the handle's own error code and the message `file access failed: {cause}`.

The Host builds the handle because the mounts are the Host's choice: memory, a real directory, or a backend of its own. The handle holds tool files alone, and the Engine and the Harness write to it only what a tool call asks for.

### The Engine makes each run an empty store, which only the run reaches

The Engine makes a store for each run, and the store starts empty. Every chain that runs the run's own prompt shares the one store: the main walk, a `call` chain, a task, and a fanout arm. The Engine keeps the store to itself: a caller reaches it only through the view in a `Vfs` effect, for that effect's one operation. The store is empty when the run starts and is gone when the run ends: data goes into a run as `args` and comes out as the result ([The Harness and the Host API](harness-and-host.md)). The store and the handle are disjoint: a tool works in the files of the handle alone, and a `store` call works in the files of its store alone.

The owner's reason: "Every prompt starts with a, a memory store that's completely empty. And that's the only type of store there is. And it doesn't map to any file system path in the virtual file system." With the store closed to the outside, a run's result depends only on its prompt, its arguments, and the answers to its effects.

The owner holds the parts of this rule with different confidence. Data enters a run as its arguments and leaves as its result, and the owner is sure of that: "we don't want to use the store as a way of getting data in and out of the prompt from the host", and "Using the argument list and using the return value instead of the input and output files, that was definitely the right move." Keeping the store out of reach of a model's file tools is less certain. The owner made that choice for `execute`: "The problem with execute is that the prompt that we're calling needs to map itself into the virtual file system, and I couldn't think of a place to put it." The owner's view of the choice: "it was a big simplification that was useful for execute. I'm not sure that it was the right one." A prompt can give a model access to its store today with an adhoc tool whose handler calls a `store` function ([Tools and Packs](tools-and-packs.md)). That keeps to the rule that a tool's files are the files of the handle: the handler is the prompt's own Lua and makes an ordinary `store` call on its chain's store, where the file access of a Pack tool reaches the handle alone. The owner does not want that to become the way a model reaches the store, and is against a second set of tools for it: "There's no reason to make a separate set of store tools. That just burdens the model." The owner's rule is that a model that reaches the store uses the file tools it already has, because "We want as few tools as possible" and "Models already know how to use Bash." That would put the store inside the file system that a tool sees, which the design in this section rules out, so it is a change that is still to be designed.

### Each `execute` gives the prompt it runs a new, empty store

The Engine makes a store for the chain of each `execute` ([The prompt language](prompt-language.md)). The store is empty and has a claims table of its own. The executed prompt's `call` chains and tasks share it, and a prompt that the child executes in turn gets another. The caller and the child each see the files of their own store alone: one path in the two stores is two files, and after the `execute` returns, every file in the caller's store is still the caller's own. A second `execute` of the same prompt starts with an empty store again.

A child's store is gone when the child ends: a store is freed once the last chain that shared it has finished or been aborted. The child's tool calls are a different matter: they work in the files of the run's handle under the caller's file access, as the caller's own tool calls do, because the executed prompt is a part of the caller's run.

The rule is the owner's, quoted in the entry "A store call is one `Access` call on a view of the calling chain's store". An executed prompt therefore has the one way in and the one way out that a run has: its arguments and its text. Two fanout arms that execute one prompt can each write the same path, and both writes pass the claims check, because each execution has its own store and its own claims.

### `RealBackend` maps a real directory into the namespace and confines paths and links to it

`RealBackend::rooted(dir)` maps the virtual root to one real directory and refuses a path that resolves outside it with `OutsideRoot`. `RealBackend::identity()` maps each virtual path to the same real path. `remove`, `exists`, `stat`, `mkdir`, and `rename` act on a link itself, and the content operations follow a link whose target is inside the root. `with_writability` makes a backend read-only. A failed write leaves the file as it was, because the backend writes a temporary sibling file and renames it.

Those operations act on the link itself because a `remove` that followed the link would delete the link's target. `RealBackend` is part of `runvil-vfs` itself: the file system calls it uses belong to the standard library, so the backend fits the crate's zero-dependency rule.

### A mount is for storage that answers quickly, and a remote system is a tool's job

A Host may mount any backend, a slow one included. Every file operation holds its run until the backend returns, so a run that works on a network drive, or in a folder that a cloud service syncs, waits for that storage, and its stop and its cancel wait with it. A feature that needs a timeout, a retry, a sign-in, or a rule for storage that goes away is a Pack's tool, which the Harness can drop while it waits ([Tools and Packs](tools-and-packs.md)).

The owner's rule for slow storage: "we should allow it, but if the performance suffers, then it suffers." For a remote system: "If you want to access those remote systems, then you need to do it through MCP". The tool then decides, in the owner's words, "what do you do if the volume goes offline, like in the middle of it".

## 4. What a run may read and write

### Each handle has one policy, which answers Allow, Deny, or Ask

An `Access` asks the handle's `Policy` about an operation path by path, reads included: about a path once that path is valid, and before the operation is admitted. The first check that fails and the first question that the policy refuses end the operation, and the questions to the policy end there. `Policy::check` returns `Allow`, `Deny(String)`, or `Ask(String)`. The file system turns `Deny` into `PermissionDenied` and `Ask` into `ApprovalRequired`. The default policy is `AllowAll`. A store is outside the handle and has `AllowAll` as its own policy, so the handle's policy sees the file operations of tool calls, and a `store` call goes to the store's own policy.

A copy and a rename are asked as what they are made of. Each asks about its source and then about its destination under its own kind, `Op::Copy` or `Op::Rename`, and then about its source as `Op::Read` and about its destination as `Op::Write`. The first refusal ends the operation with the source and the destination as they were. A policy that refuses reads under a path is the plain way for a Host to keep a model from a file, and a copy or a rename out of that path would otherwise hand the content over. The same holds for a policy that refuses writes under a path and a copy or a rename into it. The questions under the operation's own kind come first, so a policy that treats a copy or a rename as one kind of change, as `ModePolicy` does, gives its own verdict and its own reason for it. The questions about the source of a rename leave `Op::Delete` out, so a policy that refuses deletes so that every file is kept can still allow a move. The questions name the two paths of the operation alone, whatever is under them: a rename of a directory moves everything in it, so a policy that guards a path must also refuse a rename of each directory above that path. `StrReplace`, the replacement of a text in a file, is asked the same way: about its path as `Op::Write`, and then as `Op::Read`, because it reads the file and its failure tells how many times the text occurs. A policy that refuses reads under a path therefore refuses a replacement there, and the failure is that refusal in place of a count. A policy is asked about the canonical path of each operation, whatever a link on that path leads to, so where a backend follows a link, as `RealBackend` does for a target inside its root, an operation through a link under an allowed path reaches a file under a guarded one.

The model reads the `Deny` text, and a person would read the `Ask` text. A tool of a library Plugin receives both answers as one file error of the C contract, of kind `RV_FILE_ERROR_DENIED`, with the policy's text. One policy for a handle is enough, because a Host that needs a richer system can combine its rules inside a single `Policy` object. A Host's policy ends at its handle, because a policy decides what a run's tools may do to the files of the handle, and the store is the run's own memory, outside every handle.

### `ModePolicy` allows or refuses changes by mode: `Ask`, `Plan`, or `Agent`

`ModePolicy` is a policy that ships beside `AllowAll`. In `Mode::Ask` it answers `Ask` to every change, in `Mode::Plan` it allows changes to paths that end in `.md` and denies the others, and in `Mode::Agent` it allows everything. It allows every read. A Host sets the mode during a run through a `ModeHandle`. A mode decides the changes that tools make through the handle, and a `store` call runs the same in every mode.

Modes are an implementation of the `Policy` trait, separate from the machinery of the VFS.

### An operation sink observes every admitted operation of its handle

A Host installs a callback, the operation sink, with `VfsRefBuilder::on_op`. The handle calls it once for each path of an operation that it admitted: one that its own policy allowed in every question, from a run and a tool call that are both still under way. A handle that is mounted inside it can still refuse the operation afterward. Each call has the operation kind, the canonical path, and the `Origin` of the access, a label with a file and line. A copy or a rename has two paths, so two calls under its own kind are one copy or one rename, and the policy may have allowed more questions than the sink shows. A store is outside the handle, so the sink observes the file operations of tool calls, and a `store` call passes it by.

Each store operation is an effect instead: the Harness sees it before it runs, and the run's record holds it.

## 5. Identities and conflicts

### Each operation belongs to an identity, and a conflict in a store ends the run

Every `Access` has an `ExecId`, the identity of one serial thread of execution: the main walk, a task, or a tool call. Each store has a claims table, which records which identity read or wrote which path of that store. When two identities use one store path, at least one of them writes, and the two are unordered, the second operation fails with `VfsError::Conflict`. The Engine then ends the run with the error code `determinism` and abandons the suspended chain, together with any `pcall` around the call.

A lock on the backend makes such a pair of accesses memory-safe, and the claims table refuses the second one all the same. The owner's reason: "It's safe, but it's not correct because that's non-deterministic behavior. That means replay is not going to work right."

### The claims check applies to a store alone, and the Host's files are unchecked

The claims check belongs to a store alone, so an operation on the files of the run's handle runs whatever the other identities have read or written, and whoever asked for it: a model's tool call, or the prompt's own `tools.call`. Two tasks that work on one file both succeed. A writer in one task goes ahead beside a reader in another, and the later write is the content. That holds for every handle that a Host builds, so two runs over one handle, and the Host's own access beside them, share its files in the same way.

The owner's reason: "maybe the determinism error should only apply to store operations? and we should otherwise try to behave like regular harnesses, so that two subagents which conflict on file access can just work it out". On the prompt's own `tools.call`, the owner's words: "I think I am ok with Lua's own tools.call on Host files being unchecked. We can always put it back in later." The store is the prompt's own memory, and every party in it is the prompt's Lua, so the check stays there. The Host's files are the outside world: with a real folder mounted, another program can change a file, and a claims table knows the run's own identities alone.

The record of a run stays whole all the same, because each operation that a tool asks for through the files of its call is a recorded effect that the Harness performs in order (see the entry on a tool's file request). A gap is known. A tool that edits a file as a read followed by a write can lose an edit that another task makes between the two ([runvil-plugin-std issue 4](https://github.com/cppalliance/runvil-plugin-std/issues/4)). The list of operations has `StrReplace`, which replaces a text in one step, and a Rust Pack can ask for it. The C contract lacks a function for it, so it is out of reach of the tools of `std` and `bash`.

The claims code stays in `runvil-vfs` with its tests, and only that crate can make a handle check claims, which a store does. A check on the Host's files can therefore be added in that crate.

### Happens-before order decides whether two accesses to a store conflict

Each identity has a vector clock. Fork and join are methods of `ScopeHandle`, which `VfsRef::acquire_scope` returns and whose methods are `spawn`, `join`, `end_access`, and `end`. `ScopeHandle::spawn` forks a child identity, and the fork orders everything the parent did before it ahead of the child's first operation. `ScopeHandle::join` merges a child's clock into its owner's, and the owner may then read what the child wrote to the store. The Engine forks at each task spawn and joins at each delivery of a task's result ([Tasks and fanout](tasks-and-fanout.md)).

The ordering test is the one that the FastTrack race detector uses, and with it a verdict depends only on the structure of the program. A claim that lasted for the life of its identity would make the verdict depend on which model call returns first.

### A read claims the whole region it observes

Where claims are checked, `read` and `exists` claim a path, `glob` claims its pattern, and `list` claims a directory's children. A write claims its path and each parent directory it may create. A `glob` therefore conflicts with an unordered write of a matching file, whichever runs second. Two unordered writes below one new directory pass.

A `glob` or an `exists` that runs before a sibling task creates a matching file has to conflict as well, because its result would otherwise depend on timing. The claims are stricter than some cases need. That extra strictness is deterministic, and a missed conflict would be nondeterministic.

### A scope is one acquire and its forks, and the run closes its scope at `Done`

`VfsRef::acquire` starts a scope: a new identity plus every identity later forked from it. Accesses in two live scopes are always unordered, so in one store a write in one scope conflicts with an overlapping access in the other. A backend that wraps another backend passes the `AcquireContext` of `Vfs::acquire` on as it received it, so a wrapped `VfsRef` joins the caller's scope.

A run is one scope, which the Engine starts with `VfsRef::acquire_scope` on the run's handle. A store view joins the scope of the `Access` it comes from and records its claims in the store's own claims table, so the forks, joins, and ends of the scope order store operations. Each store's claims are its own, even under one identity, and every claim of a run is in a store. The Engine closes the run's scope when the run ends: each store's table then ignores the scope's claims, and an `Access` of the scope, a store view included, refuses each operation with `ScopeEnded`.

The scope bounds what a run can do: once it is closed, every access of the run refuses to change a file of the handle or of a store. A store view joins its caller's scope so that the identities of the run's chains order its `store` calls.

### A tool call works under its own identity, which the Engine ends with the call

The Engine forks an identity for each Pack tool call. The identity's `Access` is rooted at `/` of the run's handle and reaches the files of that handle alone, and every file operation of the call runs on it ([Tools and Packs](tools-and-packs.md)). The Engine holds the `Access` itself and passes it only in the `Vfs` effect that it issues for each request of the call, and the Harness performs the operation on it. A tool asks for each operation, through the Rust API or the C contract, and the `Access` stays with the Engine and the Harness. When the Harness answers the call or the Engine aborts it, `ScopeHandle::end_access` joins the identity into the calling chain, and every later operation through that `Access` fails with `ScopeEnded`. When the run's result is decided, the Engine abandons every task that is still live ([Tasks and fanout](tasks-and-fanout.md)), so a call that a task, or a `call` or `execute` chain under that task, still awaits ends as any aborted call does, with its identity ended at once. The Engine leaves the identity of every other open call to the close of the run's scope, as `Run::step` returns `Step::Done`: such a call is one that the main walk, or a `call` or `execute` chain under the main walk, still awaits. The Engine withdraws both kinds of call with every other outstanding effect, and the Harness skips or aborts each withdrawn effect and answers it `Dropped`. The Engine ends the identity because a file operation of a call that it aborted could otherwise still run.

The claims check belongs to a store alone, so each operation of a tool call runs whatever another access has read or written (see the entry on the claims check). A failure of one, such as a path that is absent or a policy's denial, goes to the tool alone, and its Pack decides how to answer the call. The Engine ends a run for a conflict only in the answer to a `store` call.

### A tool's file request is a `Vfs` effect, which the Harness performs and records

A tool asks for a file operation and waits for the outcome. A Rust Pack's tool asks through the files of its call, and a library Pack's tool asks through a file function of the C contract, which the loader passes on through the files of the call ([Tools and Packs](tools-and-packs.md)). The Harness reports the request to the Engine, the Engine issues a `Vfs` effect under the call's identity, the Harness performs and records it, and the tool goes on with the answer ([The Engine](engine.md)). A request is a tool's one way to the run's files, so the record holds every file operation of a tool. The owner's rule: "every VFS operation that's performed by a tool has to turn into an effect in the engine."

The reason is the record. The owner's questions about a tool that performs its file operations itself: "how's replay gonna work? How are we gonna do replay? How are we gonna handle debugging?" One ordered record of a run exists only if the Harness sees every effect and every answer ([The Harness and the Host API](harness-and-host.md)).

## 6. Errors

### `VfsError` is the one error type, and each variant has a stable tag

A VFS operation that fails returns a `VfsError`, an enum whose details are public fields. `VfsError::tag()` returns one fixed string for each variant, such as `not_found`. Each kind of refusal has its own variant, such as `ReadOnly` and `OutsideRoot`.

### A failed `store` call raises a Lua error value of kind `store`

In that value, `err.kind` is `store`, `err.reason` is the variant's tag, and `err.path` is the path as the prompt wrote it. An anchor failure adds `anchor` and `count`, and an invalid path adds `rule`. A `pcall` catches the value, and an uncaught failure ends the run with the error code of its reason, such as `not_found`. A wrong argument type raises an error of kind `lua`.

An error has a form for each reader: Rust code matches the variant, Lua code branches on `err.reason` and the fields, and the model reads the message.

## 7. Running a store call, and persistence

### Each `store` call is one `Vfs` effect, which the Harness performs inline

A `store` function yields its operation, and the Engine issues one `Effect::Vfs` for it ([The Engine](engine.md)). The Harness performs the effect at once, before it waits for any other effect. The Engine reports an event for each call, such as `VfsWriteSucceeded`, which names the section and omits the path and the store. The run's record holds every store operation: each `Vfs` effect record with the id of its store and its operation, and each answer record with its outcome, in order. The record is the only view of a store outside its run, and the records of one store are enough to rebuild it.

The Harness performs the call inline because file operations are synchronous by design: the storage underneath is synchronous, so a `Vfs` effect is one blocking call, and the run waits for the operation. The owner's reason: "all file I/O synchronizes the, the harness, and that's in line with our design." The Harness performs the operation through the effect's view and lets the view go once the effect is answered, so its way to the store lasts for that one effect. It answers every `Vfs` effect of a step this way and then steps the run once. VFS operations run one at a time, in the order the Engine issues them, whether a `store` call or a tool's request asked for each.

Store operations are effects for the owner's reason: "We want to be able to debug prompts and replay them and see how the contents of the store evolves". The record holds every store call with its arguments and its outcome, the Harness sees each call before it happens, and a replay, which is planned work, would issue the same effects again.

The `store` functions stay in the Engine. Serving them from a tool would only restructure the code to make the Engine smaller, and in the owner's words, "This has nothing to do with capabilities." The owner's verdict on serving them from a tool: "That turned out to be not a good idea". Turning the store into a tool was more work, in the owner's words "to turn it into a tool and to create all the rigmarole", and the store is part of the language: "It just needs to be a language feature." The owner's rule is that a feature that is deeply part of the language stays in the Engine.

### A `Vfs` effect and its record name a store by a chain id, and a tool call by its effect id

The `Vfs` effect of a `store` call names its store in its `asker`, by a `ChainId`, and the effect's record holds that id beside the operation, as in `{"Vfs":{"store":"0","op":{...}}}`. The id `0`, the root chain's, names the store that the run starts with. A store that an `execute` made has the id of the executed prompt's chain, such as `0.0`, and every operation on that store has this id, whichever chain of the executed prompt made the call.

The record names the store because an executed prompt reports under its caller's task, and two stores can each have a file at one path. The id is what tells a reader of the log the caller's `a.md` from the child's. With it, the records of each store rebuild that store. The id is a chain id because a chain id is the same in every run with the same inputs and answers ([Tasks and fanout](tasks-and-fanout.md)), which a replay needs.

The `Vfs` effect of a tool's request names the tool call, by the id of its `ToolCall` effect, and the record holds that id beside the operation, as in `{"Vfs":{"call":7,"op":{...}}}`. Its files are the files of the run's handle, which the record leaves unnamed because a run has one handle. The effect record holds the operation with the bytes of a write, and the answer record holds a receipt in place of content, such as the number of bytes of a read ([The Engine](engine.md)).

### A run's store ends with the run, and the files of its tools last as long as the Host's mounts

A run's store is memory: it is empty when the run starts and is gone when the run ends, so a run's store holds only the files that the run itself wrote. The store of an executed prompt is gone sooner, when that prompt ends. A prompt that built a file returns the file's text, as in `return store.read('report.md')`. The files of a run's tools are in the mounts of the run's handle: a `MemoryBackend` drops with its handle, and the files below a `RealBackend::rooted` mount are in a real directory, which outlasts the run. Workshop builds a default `VfsRef` for each run, so the files of a Workshop run's tools exist in memory until the run ends, and Workshop's workspace document is a separate file.

A run has one way in, its arguments, and one way out, its result. The owner's reason: "If a prompt wants to return a file, it should return it as the string of its return value."

## 8. Planned work

- **A database mount.** A mount would show an agent's run database as files. Its backend is synchronous like every other, so a backend over a database client with an async interface drives that client on a thread of its own. The backends that ship are `MemoryBackend` and `RealBackend`.
- **Replay.** Replay would read a run's records back into a run, and a debugger would rebuild each store of a run from the same records, at any step. Today the Harness records each `Vfs` effect and its answer.
- **Glob across mounts.** A glob would also search the backends mounted below the mount that matches its pattern. Today a glob runs inside that mount and skips a backend mounted below it.
