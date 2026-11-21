//! The two built-in Packs, which every Host has and the runner makes
//! itself: `runvil/subagents` and `runvil/user`. No Plugin inserts them,
//! so no registry holds them, and a registry refuses a Pack that takes
//! the name or the Pack id of one.

use std::sync::Arc;

use runvil_engine::{BoxFuture, Error, ErrorCode};
use runvil_engine::{ToolInfo, ToolOutput, make_tool_error};
use runvil_plugin::{Pack, PackId, PackInfo, ToolContext};
use serde_json::Value;

/// The Packs every Host has beside the ones its Plugins inserted:
/// `runvil/subagents`, and `runvil/user`, which marks a prompt that needs
/// a human. Neither offers tools or has a prelude. A run holds
/// `runvil/user` only when its setup has a user.
///
/// A built-in Pack's behavior and tools live in the Engine. Its instance
/// exists only so making, readiness, and the run snapshot treat it like
/// any other Pack, so a built-in must never offer tools or have a
/// prelude, and its factory never refuses a run. A built-in can still be
/// unavailable to a run: `runvil/user` is unavailable in a run without a
/// user, which the runner decides from the run's setup instead of making
/// the Pack. Its `call` is unreachable, because the Engine never issues a
/// call under a built-in's id: a built-in lists no tools, so no section
/// offers one of its tools and no render of it adds one. A built-in never
/// takes a [reserved](PackId::is_reserved) Pack id: every run's roster
/// names every Pack of the Host, so a reserved one would refuse every
/// run.
pub(crate) const BUILTIN: [PackInfo; 2] = [
    PackInfo::new("runvil/subagents").description(
        "Reserves the subagents name for the Engine's subagent tools, which are not built yet",
    ),
    PackInfo::new("runvil/user")
        .description("Marks a prompt that needs a human; only a run with a user holds it"),
];

/// The Pack id of the built-in `info`: the second segment of its FQN.
pub(crate) fn builtin_id(info: &PackInfo) -> Option<PackId> {
    let (_vendor, name) = info.fqn.split_once('/')?;
    PackId::parse(name).ok()
}

/// The factory of every built-in: makes each run's [`Builtin`] instance;
/// see [`BUILTIN`].
#[expect(
    clippy::unnecessary_wraps,
    reason = "the PackFactory signature fixes the return type"
)]
pub(crate) fn builtin(_id: &PackId) -> Result<Arc<dyn Pack>, Error> {
    Ok(Arc::new(Builtin))
}

/// The instance of a built-in Pack, which offers no tools; see
/// [`BUILTIN`].
struct Builtin;

impl Pack for Builtin {
    fn tools(&self) -> Vec<ToolInfo> {
        Vec::new()
    }

    fn call<'a>(
        &'a self,
        cx: ToolContext<'a>,
        _args: Value,
    ) -> BoxFuture<'a, Result<ToolOutput, Error>> {
        let message = format!(
            "{} belongs to a built-in Pack, whose calls the Engine answers",
            cx.tool()
        );
        Box::pin(async move { Err(make_tool_error(ErrorCode::ToolOther, message)) })
    }
}

#[cfg(test)]
mod tests {
    //! Tests for the built-in Packs: none takes a reserved Pack id, each
    //! offers no tools and has no prelude, each has a description, and a
    //! registry refuses the name and the Pack id of each.

    use runvil_engine::ErrorFact;
    use runvil_plugin::PackRegistry;
    use serde_json::json;

    use super::*;

    #[test]
    fn no_builtin_takes_a_reserved_pack_id() {
        for info in BUILTIN {
            let id = builtin_id(&info).unwrap();
            assert!(!id.is_reserved(), "{} takes a reserved Pack id", info.fqn);
        }
    }

    #[test]
    fn builtins_offer_no_tools_or_prelude() {
        for info in BUILTIN {
            assert!(info.prelude.is_none(), "{} has a prelude", info.fqn);
            let name = builtin_id(&info).unwrap();
            let pack = builtin(&name).unwrap();
            assert!(pack.tools().is_empty(), "{} offers tools", info.fqn);
        }
    }

    #[test]
    fn each_builtin_has_a_description() {
        for info in BUILTIN {
            assert!(
                !info.description.is_empty(),
                "a Host listing Packs can say what {} does",
                info.fqn
            );
        }
    }

    #[test]
    fn the_registry_refuses_the_name_and_the_pack_id_of_each_builtin() {
        for info in BUILTIN {
            let id = builtin_id(&info).unwrap();
            // The built-in's own name, and its Pack id under another
            // vendor.
            let other_vendor: &'static str = String::leak(format!("acme/{id}"));
            for fqn in [info.fqn, other_vendor] {
                let refusal = PackRegistry::new()
                    .insert(PackInfo::new(fqn), builtin)
                    .expect_err("the registry refuses a built-in's name and its Pack id");
                assert_eq!(
                    refusal.facts.get(&ErrorFact::Reason),
                    Some(&json!("pack_id_taken")),
                    "{fqn}: {refusal:?}"
                );
            }
        }
    }
}
