//! Tests for the `models` namespace: `use`, `get`, and the section's current model.

use runvil_model_client::model::{ModelBinding, ModelId, ModelInvocation, ModelSet, Temperature};

use crate::error::root_text;
use crate::test_vm::{BuiltVm, TestVm};
use crate::weak::WEAK_TABLE;

/// The role-name list every label rejection ends with.
const ROLE_LIST: &str = "expected default, strong, small, fast or creative";

/// A bound role for the shared set, under `label`, on the model `model`.
fn bound_role(label: &str, model: &str) -> ModelBinding {
    ModelBinding::new(
        label,
        "A general model for tests",
        ModelId::gateway(model).expect("a static model id is valid"),
        ModelInvocation::new(),
        std::num::NonZeroU32::new(8192).expect("8192 is non-zero"),
    )
}

/// A fresh VM with the `models` table installed over a shared set holding
/// the `default` and `strong` roles.
#[track_caller]
fn models_vm() -> BuiltVm {
    let mut models = ModelSet::default();
    models.push(bound_role("default", "m1"));
    models.push(bound_role("strong", "m2"));
    TestVm::new().models(models).build()
}

/// The label of the section's current model, if it has one.
#[track_caller]
fn current_label(vm: &BuiltVm) -> Option<String> {
    vm.current_model()
        .expect("the current model is readable")
        .map(|binding| binding.alias().to_owned())
}

#[test]
fn the_models_namespace_offers_use_and_get_and_leaves_infer_to_the_shims() {
    // `models.infer` suspends, so only the coroutine shim layer defines it.
    let vm = models_vm();
    let (has_use, has_get, infer_is_nil): (bool, bool, bool) = vm.eval(
        "return type(models.use) == 'function', \
                type(models.get) == 'function', \
                models.infer == nil",
    );
    assert!(has_use && has_get);
    assert!(
        infer_is_nil,
        "install_models alone must not define models.infer"
    );
}

#[test]
fn a_handle_exposes_fields_and_no_methods() {
    // A handle is an inspectable value: the fields read, and a name it does
    // not have raises, so `infer` and `loop` are absent however the VM is
    // built.
    let vm = models_vm();
    let (name, infer_failed, loop_failed): (String, bool, bool) = vm.eval(
        "local h = models.get('default')
         local _, infer_err = pcall(function() return h.infer end)
         local _, loop_err = pcall(function() return h.loop end)
         return h.name, infer_err ~= nil, loop_err ~= nil",
    );
    assert_eq!(name, "default");
    assert!(
        infer_failed && loop_failed,
        "a handle has no infer and no loop"
    );
}

#[test]
fn a_new_section_vm_starts_on_the_default_role() {
    assert_eq!(
        current_label(&models_vm()).as_deref(),
        Some("default"),
        "a section starts on the default role's binding"
    );
}

#[test]
fn a_section_vm_with_no_models_has_no_current_model() {
    let vm = TestVm::new().build();
    assert_eq!(
        current_label(&vm),
        None,
        "an empty set leaves the section with no model"
    );
}

#[test]
fn models_use_selects_a_role_by_label() {
    let vm = models_vm();
    let handle: String = vm.eval("local h = models.use('strong'); return h.label .. '|' .. h.name");
    assert_eq!(handle, "strong|strong");
    assert_eq!(current_label(&vm).as_deref(), Some("strong"));
}

#[test]
fn models_use_rejects_a_label_outside_the_five_roles() {
    let vm = models_vm();
    let error = vm.eval_err("models.use('reviewer')");
    assert!(
        error.to_string().contains(&format!(
            "models.use label \"reviewer\" is not a model role: {ROLE_LIST}"
        )),
        "the rejection names the label and the five roles: {error}"
    );
    assert_eq!(
        current_label(&vm).as_deref(),
        Some("default"),
        "a rejected call must not select"
    );
}

#[test]
fn models_get_rejects_a_label_outside_the_five_roles() {
    let error = models_vm().eval_err("models.get('reviewer')");
    assert!(
        error.to_string().contains(&format!(
            "models.get label \"reviewer\" is not a model role: {ROLE_LIST}"
        )),
        "the rejection names the label and the five roles: {error}"
    );
}

#[test]
fn a_role_with_no_model_fails_with_the_caller_supplied_none_message() {
    for chunk in ["models.use('default')", "models.get('default')"] {
        let error = TestVm::new().build().eval_err(chunk);
        assert!(
            error.to_string().contains("the caller supplied no model"),
            "{chunk}: the failure says the caller supplied no model: {error}"
        );
    }
}

#[test]
fn a_role_the_set_lacks_fails_with_the_caller_supplied_none_message() {
    let error = models_vm().eval_err("models.use('fast')");
    assert!(
        error.to_string().contains("the caller supplied no model"),
        "a valid role with no binding says the caller supplied no model: {error}"
    );
}

#[test]
fn models_use_options_reach_the_returned_handle() {
    let vm = models_vm();
    let (temperature, max_tokens): (f64, u32) = vm.eval(
        "local h = models.use('default', { temperature = 0.3, max_tokens = 256 }); \
         return h.temperature, h.max_tokens",
    );
    assert!((temperature - 0.3).abs() < f64::EPSILON);
    assert_eq!(max_tokens, 256);
    assert_eq!(current_label(&vm).as_deref(), Some("default"));
    let whole: u32 = vm.eval("return models.use('default', { max_tokens = 512.0 }).max_tokens");
    assert_eq!(whole, 512, "a whole-number float counts as its integer");
}

#[test]
fn models_use_accepts_an_integer_temperature_and_leaves_omitted_fields_nil() {
    let (temperature, max_tokens_is_nil): (f64, bool) = models_vm().eval(
        "local h = models.use('default', { temperature = 0 }); return h.temperature, h.max_tokens == nil",
    );
    assert!(temperature.abs() < f64::EPSILON);
    assert!(
        max_tokens_is_nil,
        "an omitted field keeps the role's default"
    );
}

#[test]
fn models_use_rejects_invalid_options_without_selecting() {
    let cases = [
        (
            "models.use('strong', { temperature = 2.5 })",
            "models.use option temperature 2.5 is outside the supported range [0.0, 2.0]",
        ),
        (
            "models.use('strong', { temperature = -0.1 })",
            "models.use option temperature -0.1 is outside the supported range [0.0, 2.0]",
        ),
        (
            "models.use('strong', { temperature = 0/0 })",
            "models.use option temperature must be finite, got NaN",
        ),
        (
            "models.use('strong', { temperature = 'hot' })",
            "models.use option temperature must be a number, got string",
        ),
        (
            "models.use('strong', { max_tokens = 0 })",
            "models.use option max_tokens must be an integer in [1, 4294967295], got 0",
        ),
        (
            "models.use('strong', { max_tokens = -1 })",
            "models.use option max_tokens must be an integer in [1, 4294967295], got -1",
        ),
        (
            "models.use('strong', { max_tokens = 1.5 })",
            "models.use option max_tokens must be an integer in [1, 4294967295], got 1.5",
        ),
        (
            "models.use('strong', { max_tokens = 'many' })",
            "models.use option max_tokens must be an integer in [1, 4294967295], got string",
        ),
        (
            "models.use('strong', { top_p = 0.9 })",
            "models.use option \"top_p\" is unknown: expected temperature or max_tokens",
        ),
        (
            "models.use('strong', { 0.5 })",
            "models.use option names must be strings, got integer",
        ),
        (
            "models.use('strong', 'fast')",
            "models.use options must be a table, got string",
        ),
        (
            "models.use('strong', {}, 1)",
            "models.use takes at most 2 arguments, got 3",
        ),
    ];
    for (chunk, expected) in cases {
        let vm = models_vm();
        let error = vm.eval_err(chunk);
        assert!(
            error.to_string().contains(expected),
            "{chunk}: expected {expected:?} in {error}"
        );
        assert_eq!(
            current_label(&vm).as_deref(),
            Some("default"),
            "{chunk}: a rejected call must not select"
        );
    }
}

#[test]
fn models_use_refuses_weak_options_without_selecting() {
    let vm = models_vm();
    let error =
        vm.eval_err("models.use('strong', setmetatable({ temperature = 1 }, { __mode = 'k' }))");
    assert_eq!(
        (root_text(&error), current_label(&vm).as_deref()),
        (
            format!("models.use options must not be {WEAK_TABLE}"),
            Some("default")
        )
    );
}

#[test]
fn models_use_reports_the_first_bad_option_in_key_order_on_every_state() {
    // Each fresh state walks `pairs` under its own hash seed; the rejection
    // must not follow that walk. Every option in a case is bad, so the
    // rejection names the first of its keys in the shared order: booleans,
    // then numbers, then names bytewise.
    for (options, expected) in [
        (
            "{ top_p = 1, max_tokens = 0, temperature = 3 }",
            "models.use option max_tokens must be an integer in [1, 4294967295], got 0",
        ),
        (
            "{ temperature = 3, top_p = 1 }",
            "models.use option temperature 3 is outside the supported range [0.0, 2.0]",
        ),
        (
            "{ 'x', [true] = 1, temperature = 3 }",
            "models.use option names must be strings, got boolean",
        ),
    ] {
        let chunk = format!("models.use('strong', {options})");
        for state in 0..32 {
            let error = models_vm().eval_err(&chunk);
            assert_eq!(root_text(&error), expected, "state {state}: {chunk}");
        }
    }
}

#[test]
fn models_use_reads_an_options_table_too_large_to_hold_a_reference_to_each_entry() {
    // `mlua` panics when Rust holds about a million Lua references, and each
    // entry here is one.
    let error = models_vm().eval_err(
        "local options = {} for i = 1, 1200000 do options[i] = 'x' end \
         models.use('strong', options)",
    );
    assert_eq!(
        root_text(&error),
        "models.use option names must be strings, got integer"
    );
}

/// The section's current model's `(temperature, max_tokens)`, read the way
/// the Engine's Chat-effect sites read it.
#[track_caller]
fn effective_sampling(vm: &BuiltVm) -> (Option<f64>, Option<u32>) {
    let binding = vm
        .current_model()
        .expect("the model state is readable")
        .expect("a selection resolves");
    let invocation = binding.invocation();
    (
        invocation.temperature.map(Temperature::get),
        invocation.max_tokens.map(std::num::NonZeroU32::get),
    )
}

#[test]
fn the_selection_carries_its_options_until_a_later_models_use_replaces_them() {
    let vm = models_vm();
    vm.eval::<()>("models.use('default', { temperature = 0.5, max_tokens = 64 })");
    assert_eq!(effective_sampling(&vm), (Some(0.5), Some(64)));

    let (temperature_is_nil, max_tokens_is_nil): (bool, bool) = vm
        .eval("local h = models.use('default'); return h.temperature == nil, h.max_tokens == nil");
    assert!(temperature_is_nil && max_tokens_is_nil);
    assert_eq!(
        effective_sampling(&vm),
        (None, None),
        "a plain models.use clears the earlier options"
    );
}

#[test]
fn a_models_get_handle_for_the_selected_role_keeps_the_role_defaults() {
    let (temperature_is_nil, max_tokens_is_nil): (bool, bool) = models_vm().eval(
        "models.use('default', { temperature = 0.5, max_tokens = 64 }); \
         local h = models.get('default'); \
         return h.temperature == nil, h.max_tokens == nil",
    );
    assert!(temperature_is_nil && max_tokens_is_nil);
}

#[test]
fn the_handle_exposes_label_model_id_description_context_thinking_and_provider() {
    let mut models = ModelSet::default();
    models.push(
        bound_role("default", "m1")
            .with_invocation(ModelInvocation::new().with_thinking(Some(true)))
            .with_provider("acme"),
    );
    let inspected: String = TestVm::new().models(models).build().eval(
        "local h = models.get('default'); \
         return h.label .. '|' .. h.model_id .. '|' .. h.description .. '|' .. h.context \
         .. '|' .. tostring(h.thinking) .. '|' .. h.provider",
    );
    assert_eq!(
        inspected,
        "default|m1|A general model for tests|8192|true|acme"
    );
}

#[test]
fn models_current_returns_the_default_handle_before_any_use() {
    let (label, model_id): (String, String) =
        models_vm().eval("local h = models.current(); return h.label, h.model_id");
    assert_eq!((label.as_str(), model_id.as_str()), ("default", "m1"));
}

#[test]
fn models_current_follows_models_use() {
    let (label, model_id): (String, String) = models_vm()
        .eval("models.use('strong'); local h = models.current(); return h.label, h.model_id");
    assert_eq!((label.as_str(), model_id.as_str()), ("strong", "m2"));
}

#[test]
fn models_current_with_no_models_fails_with_the_caller_supplied_none_message() {
    let error = TestVm::new().build().eval_err("models.current()");
    assert!(
        error.to_string().contains("the caller supplied no model"),
        "the failure says the caller supplied no model: {error}"
    );
}

#[test]
fn models_use_of_a_saved_handle_puts_the_section_back_on_it() {
    let vm = models_vm();
    vm.eval::<()>("local prev = models.current(); models.use('strong'); models.use(prev)");
    assert_eq!(current_label(&vm).as_deref(), Some("default"));
}

#[test]
fn models_use_of_a_handle_returns_that_handle() {
    let (label, model_id): (String, String) =
        models_vm().eval("local h = models.use(models.get('strong')); return h.label, h.model_id");
    assert_eq!((label.as_str(), model_id.as_str()), ("strong", "m2"));
}

#[test]
fn models_use_applies_its_options_on_top_of_a_handle() {
    let vm = models_vm();
    vm.eval::<()>("local prev = models.current(); models.use(prev, { temperature = 0.2 })");
    assert_eq!(current_label(&vm).as_deref(), Some("default"));
    assert_eq!(effective_sampling(&vm), (Some(0.2), None));
}

#[test]
fn models_use_rejects_an_argument_that_is_neither_a_role_nor_a_handle() {
    let vm = models_vm();
    let error = vm.eval_err("models.use(5)");
    assert!(
        error
            .to_string()
            .contains("models.use takes a model role name or a model handle, got number"),
        "the rejection names the argument's Lua type: {error}"
    );
    let error = vm.eval_err("models.use(messages)");
    assert!(
        error
            .to_string()
            .contains("models.use takes a model role name or a model handle, got userdata"),
        "a userdata that is no handle is rejected the same way: {error}"
    );
    assert_eq!(
        current_label(&vm).as_deref(),
        Some("default"),
        "a rejected call must not select"
    );
}

#[test]
fn models_get_rejects_a_label_outside_the_roles_when_the_caller_supplied_no_model() {
    let error = TestVm::new().build().eval_err("models.get('ghost')");
    assert!(
        error.to_string().contains("is not a model role"),
        "the rejection names the role rule, not the missing model: {error}"
    );
}

#[test]
fn a_poisoned_selection_reaches_the_caller_as_an_internal_error() {
    let vm = models_vm();
    let poisoner = std::sync::Arc::clone(&vm.selection);
    let _ = std::thread::spawn(move || {
        let _guard = poisoner.lock();
        panic!("poison the model selection");
    })
    .join();

    let error = vm
        .current_model()
        .expect_err("a poisoned selection cannot be read");
    assert!(
        error.code == runvil_types::error::ErrorCode::Internal
            && error.message == "internal invariant violated: model selection lock was poisoned",
        "the lock failure is Engine misuse, neither an authoring nor a model failure: {error:?}"
    );
}

#[test]
fn a_poisoned_selection_raises_the_internal_failure_inside_lua() {
    let mut models = ModelSet::default();
    models.push(bound_role("default", "m1"));
    let vm = TestVm::new().models(models).shims().build();
    let poisoner = std::sync::Arc::clone(&vm.selection);
    let _ = std::thread::spawn(move || {
        let _guard = poisoner.lock();
        panic!("poison the model selection");
    })
    .join();

    let kinds: (String, String) = vm.eval(
        "local _, current = pcall(models.current)\n\
         local _, chosen = pcall(models.use, 'default')\n\
         return tostring(current.kind), tostring(chosen.kind)",
    );
    assert_eq!(kinds, ("internal".to_owned(), "internal".to_owned()));
}
