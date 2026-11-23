//! The `annotations` frontmatter key: MCP behavior hints for publishing
//! a prompt as a tool.

use serde::Deserialize;

/// The behavior hints a prompt declares under `annotations:`, for
/// publishing the prompt as a tool. The Engine never reads them.
///
/// Deserialization accepts a map of the four camelCase hint keys, each
/// a boolean or `null`, and rejects unknown or duplicate keys. All hint
/// combinations are preserved; write hints have no meaning when
/// `readOnlyHint` is true.
///
/// No default is filled in, because MCP's defaults differ between hints
/// (`destructiveHint` and `openWorldHint` default to true): a hint the
/// prompt left out stays unset, so the published tool carries only what
/// the prompt said.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, expecting = "a map of behavior hints")]
pub struct Annotations {
    /// The `readOnlyHint` the prompt wrote.
    #[serde(rename = "readOnlyHint")]
    read_only: Option<bool>,
    /// The `destructiveHint` the prompt wrote.
    #[serde(rename = "destructiveHint")]
    destructive: Option<bool>,
    /// The `idempotentHint` the prompt wrote.
    #[serde(rename = "idempotentHint")]
    idempotent: Option<bool>,
    /// The `openWorldHint` the prompt wrote.
    #[serde(rename = "openWorldHint")]
    open_world: Option<bool>,
}

impl Annotations {
    /// Returns `readOnlyHint`: when true, the prompt does not modify its
    /// environment. `None` means the prompt left it unset.
    #[must_use]
    pub fn read_only_hint(&self) -> Option<bool> {
        self.read_only
    }

    /// Returns `destructiveHint`: when true, the prompt may perform
    /// destructive updates, and when false, it only adds. It applies only
    /// to a prompt that is not read-only. `None` means the prompt left it
    /// unset.
    #[must_use]
    pub fn destructive_hint(&self) -> Option<bool> {
        self.destructive
    }

    /// Returns `idempotentHint`: when true, running the prompt again with
    /// the same arguments has no further effect on its environment. It
    /// applies only to a prompt that is not read-only. `None` means the
    /// prompt left it unset.
    #[must_use]
    pub fn idempotent_hint(&self) -> Option<bool> {
        self.idempotent
    }

    /// Returns `openWorldHint`: when true, the prompt may interact with an
    /// open world of external entities, and when false, its domain is
    /// closed. `None` means the prompt left it unset.
    #[must_use]
    pub fn open_world_hint(&self) -> Option<bool> {
        self.open_world
    }
}
