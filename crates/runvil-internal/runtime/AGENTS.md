# runvil-runtime

- Only the executor decides store access. It makes the run's store, and it derives every store view from that store under the chain's access, at dispatch for a `Vfs` effect. The caller uses the view it was given and never derives, widens, or keeps store scope from it.
