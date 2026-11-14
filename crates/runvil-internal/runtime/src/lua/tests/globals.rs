//! The reserved-name list against the globals a section VM holds after the
//! executor's real setup path, compared in both directions: every global
//! section setup leaves in `_G` is a reserved name, and
//! every reserved global reads non-nil once the VM binds its conditional
//! ones (`ui`, `item`, `prose`) and `require` installs over a resolver and
//! a renderer. No Pack prelude installs here, so what
//! `_G` holds is exactly what the Engine installs. Model roles bind here
//! too, since a role label is never a global, and the run offers tools of
//! a declared Pack and of another one, since a tool is never a global
//! either.

use std::sync::Arc;

use mlua::Value;
use serde_json::json;

use runvil_lua::{RESERVED_NAMES, Reserved, reserved_name};
use runvil_types::packs::PackId;
use runvil_types::tools::{ToolId, ToolInfo};

use super::{VmBinds, set_up_vm};
use crate::execute::section_vm::VmSeed;
use crate::lua::{SectionVm, ToolSet};
use crate::model::ModelSet;
use crate::test_support::scenario::test_binding;

/// The role labels every fully bound VM fills.
const ROLE_LABELS: [&str; 2] = ["default", "strong"];

/// One role bound under each of [`ROLE_LABELS`].
fn roles() -> ModelSet {
    let mut set = ModelSet::default();
    for label in ROLE_LABELS {
        set.push(test_binding(label, "a role", "test-model"));
    }
    set
}

/// The default wire names of the tools in [`offering`]'s catalog.
const OFFERED_WIRE_NAMES: [&str; 2] = ["gh_search", "web_search"];

/// A set declaring the Pack `web`, whose catalog holds `web/search` beside
/// the undeclared Pack's `gh/search`.
fn offering() -> ToolSet {
    let tool = |id: &str| {
        ToolInfo::new(
            ToolId::parse(id).expect("valid id"),
            "a tool",
            json!({ "type": "object" }),
        )
    };
    ToolSet::new(
        vec![PackId::parse("web").expect("valid Pack")],
        std::collections::BTreeMap::new(),
        runvil_types::tools::ToolCatalog::new([tool("gh/search"), tool("web/search")])
            .expect("the ids differ"),
    )
}

/// Builds a section VM through the real setup path with every conditional
/// Engine global bound: a caller-supplied application-state snapshot
/// (`ui`), a collection member (`item`),
/// a block's lazy `prose`, the model roles in [`ROLE_LABELS`], the tools of
/// [`offering`], and a `require` whose resolver reaches no library.
fn fully_bound_vm() -> SectionVm {
    let item = json!("member");
    let ui = Arc::new(json!({}));
    let vm = set_up_vm(&VmBinds {
        section: "Globals",
        tools: &offering(),
        models: &roles(),
        seed: VmSeed {
            var: None,
            item: Some(&item),
        },
        ui: Some(&ui),
    });
    vm.install_lazy_prose(|_| Ok("the block's prose".to_owned()))
        .expect("the prose guard installs");
    vm
}

/// Every key the VM's globals table holds, read raw (no metamethod runs).
fn raw_global_names(vm: &SectionVm) -> Vec<String> {
    vm.lua()
        .globals()
        .pairs::<Value, Value>()
        .map(|pair| match pair.expect("the globals walk").0 {
            Value::String(name) => name.to_string_lossy(),
            other => format!("{other:?}"),
        })
        .collect()
}

#[test]
fn every_global_section_setup_leaves_in_g_is_a_reserved_name() {
    let vm = fully_bound_vm();
    for name in raw_global_names(&vm) {
        assert!(
            matches!(
                reserved_name(&name),
                Some(Reserved::EngineGlobal | Reserved::LuaGlobal)
            ),
            "section setup installs the global `{name}`, \
             which RESERVED_NAMES does not list as an Engine or Lua global; list it there"
        );
    }
}

#[test]
fn every_reserved_global_is_present_after_section_setup() {
    let vm = fully_bound_vm();
    let globals = vm.lua().globals();
    for (name, kind) in RESERVED_NAMES {
        if kind == Reserved::LuaKeyword {
            continue;
        }
        let value: Value = globals.get(name).expect("a global read");
        assert!(
            !value.is_nil(),
            "RESERVED_NAMES lists `{name}` as {kind}, but a fully bound section VM \
             reads it as nil; drop it or install it"
        );
    }
}

#[test]
fn no_role_label_is_a_global_after_section_setup() {
    let vm = fully_bound_vm();
    let (default, strong, call): (String, String, String) = vm
        .lua()
        .load("return type(default), type(strong), type(tools.call)")
        .eval()
        .expect("the globals probe evaluates");
    assert_eq!(
        (default.as_str(), strong.as_str(), call.as_str()),
        ("nil", "nil", "function"),
        "a role label installs no global and leaves the Engine globals in place"
    );
}

#[test]
fn no_offered_tool_is_a_global_after_section_setup() {
    let vm = fully_bound_vm();
    let names = raw_global_names(&vm);
    for wire_name in OFFERED_WIRE_NAMES {
        assert!(
            !names.iter().any(|name| name == wire_name),
            "the offered tool `{wire_name}` installs no global"
        );
    }
    let ids: String = vm
        .lua()
        .load("return tools.get('gh/search').id .. ',' .. tools.get('web/search').id")
        .eval()
        .expect("the tool objects read");
    assert_eq!(
        ids, "gh/search,web/search",
        "the tools read through `tools.get`"
    );
}

#[test]
fn models_get_returns_the_role_handle_after_section_setup() {
    let vm = fully_bound_vm();
    for label in ROLE_LABELS {
        let (kind, name, model_id): (String, String, String) = vm
            .lua()
            .load(format!(
                "local h = models.get({label:?}) return type(h), h.name, h.model_id"
            ))
            .eval()
            .expect("the handle probe evaluates");
        assert_eq!(
            (kind.as_str(), name.as_str(), model_id.as_str()),
            ("userdata", label, "test-model"),
            "models.get({label:?}) hands back the role's handle"
        );
    }
}
