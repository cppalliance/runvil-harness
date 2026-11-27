//! The Packs a Plugin's entry point inserts: the factory the registry
//! holds for each one, one run's instance of it, the functions an
//! RV_PACK_NEW's `RvPackNewArgs` and an RV_PACK_RENDER's `RvPackRenderArgs`
//! carry, and the senders of RV_PACK_NEW, RV_PACK_RENDER, and
//! RV_PACK_DELETE.

use std::ffi::c_void;
use std::mem;
use std::ptr;
use std::sync::Arc;

use runvil_plugin::{
    BoxFuture, Error, ErrorCode, ModelInfo, Pack, PackFactory, PackId, RenderContext, ThinkingMode,
    ToolContext, ToolId, ToolInfo, ToolOutput, make_tool_error,
};
use runvil_plugin_sys::{
    RV_ERR_UNSUPPORTED, RV_MAX_NAME, RV_MAX_TOOL_TEXT, RV_OK, RV_PACK_DELETE, RV_PACK_NEW,
    RV_PACK_RENDER, RV_TOOL_CHANGES_FILES, RvCall, RvPackNewArgs, RvPackRenderArgs, RvStatus,
    RvStr, RvToolInfo,
};
use serde_json::{Value, json};

use crate::call::Steps;
use crate::host::{is_segment, new_call};
use crate::plugin::Plugin;
use crate::session::{
    self, AddedTool, Family, Field, PACK_NEW, PACK_RENDER, Shape, Verb, answer, field, rv_str,
    shape, size_field,
};

#[cfg(test)]
#[path = "pack-tests.rs"]
mod tests;

const TOOL_INFO: Shape = shape!("add_tool", "info", "RvToolInfo");
const TOOL_NAME: Field = field!("the tool's name", 64, RV_MAX_NAME);
const WIRE_NAME: Field = field!("the tool's wire name", 64, RV_MAX_NAME);
const DESCRIPTION: Field = field!("the tool's description", 65536, RV_MAX_TOOL_TEXT);
const PARAMETERS: Field = field!("the tool's parameter schema", 65536, RV_MAX_TOOL_TEXT);

/// Makes each run's instance of one Pack that a Plugin inserted through
/// its entry point. It holds an `Arc` of the Plugin, so the Plugin stays
/// loaded while the registry, or the `Host` built from it, holds the
/// Pack.
pub(crate) struct EntryFactory {
    plugin: Arc<Plugin>,
    /// The Pack's name, as the Plugin gave it to `insert_pack`.
    pack: String,
}

impl EntryFactory {
    /// The factory of the Pack `plugin` inserted under the name `pack`.
    pub(crate) fn new(plugin: Arc<Plugin>, pack: String) -> EntryFactory {
        EntryFactory { plugin, pack }
    }
}

impl PackFactory for EntryFactory {
    /// Sends RV_PACK_NEW and makes the run's instance from the tools the
    /// Plugin added.
    ///
    /// # Errors
    /// Returns the Plugin's refusal with its reason as the message, or a
    /// failure that names how the Plugin broke the contract or the status
    /// RV_PACK_NEW returned.
    fn new_pack(&self, id: &PackId) -> Result<Arc<dyn Pack>, Error> {
        let (instance, added) = send_pack_new(&self.plugin, &self.pack)?;
        // The instance owns `instance` from here, so a tool that cannot be
        // named below still gets its RV_PACK_DELETE.
        let mut pack = EntryPack {
            plugin: Arc::clone(&self.plugin),
            id: id.clone(),
            instance,
            tools: Vec::new(),
        };
        pack.tools = added
            .into_iter()
            .map(|tool| tool_info(id, tool))
            .collect::<Result<_, _>>()?;
        Ok(Arc::new(pack))
    }
}

/// The catalog entry of the tool `added` under the Pack id `id`. The
/// entry says that the tool changes files when the tool's flags did.
fn tool_info(id: &PackId, added: AddedTool) -> Result<ToolInfo, Error> {
    let tool_id = ToolId::parse(&format!("{id}/{}", added.name)).map_err(|error| {
        make_tool_error(
            ErrorCode::ToolOther,
            format!(
                "the tool {} of the Pack {id} could not be named",
                added.name
            ),
        )
        .with_cause(&error)
    })?;
    let info = ToolInfo::new(tool_id, added.description, added.parameters)
        .changes_files(added.changes_files);
    Ok(match added.wire_name {
        Some(wire_name) => info.wire_name(wire_name),
        None => info,
    })
}

/// One run's instance of a Pack that a Plugin's entry point made. It
/// holds an `Arc` of the Plugin, so the Plugin outlives it, and dropping
/// it sends RV_PACK_DELETE once.
pub(crate) struct EntryPack {
    plugin: Arc<Plugin>,
    /// The Pack's id, which the ids of its tools start with.
    id: PackId,
    /// The pointer RV_PACK_NEW set, which each of the instance's calls
    /// passes back and the loader never reads through.
    instance: *mut c_void,
    tools: Vec<ToolInfo>,
}

impl EntryPack {
    /// The Plugin that made this instance, which holds the Host's
    /// resources and the slots of its calls' wakers.
    pub(crate) fn plugin(&self) -> &Plugin {
        &self.plugin
    }

    /// An `RvCall` that sends `verb` to this instance.
    pub(crate) fn new_call(&self, verb: u32) -> RvCall {
        RvCall {
            pack_run: self.instance,
            ..new_call(verb, self.plugin.instance().plugin())
        }
    }

    /// Sends `call` through the Plugin's entry point.
    ///
    /// # Safety
    /// `call` must follow the C contract for its verb, with this
    /// instance's pointers, and its `args` must stay valid for the call.
    pub(crate) unsafe fn send(&self, call: &mut RvCall) -> RvStatus {
        // SAFETY: inserting the Plugin required its entry point to follow
        // the contract and not unwind, and the caller vouches for `call`.
        unsafe { (self.plugin.instance().entry())(call) }
    }
}

impl Drop for EntryPack {
    fn drop(&mut self) {
        let mut call = self.new_call(RV_PACK_DELETE);
        // SAFETY: `instance` is the pointer a successful RV_PACK_NEW set,
        // and this drop is its only delete. Each tool call of the instance
        // borrows it, so every one of them has ended.
        let _status = session::closed(|| unsafe { self.send(&mut call) });
    }
}

// SAFETY: the `# Threads and lifetime` section of `runvil-plugin-sys` lets
// RV_PACK_DELETE, each render, and each tool call come on any thread, and
// the loader never reads through `instance`.
unsafe impl Send for EntryPack {}
// SAFETY: the same section lets tool calls and renders of one Pack instance
// run at the same time, and `&self` only hands out the pointer those calls
// pass back.
unsafe impl Sync for EntryPack {}

impl Pack for EntryPack {
    fn tools(&self) -> Vec<ToolInfo> {
        self.tools.clone()
    }

    fn call<'a>(
        &'a self,
        cx: ToolContext<'a>,
        args: Value,
    ) -> BoxFuture<'a, Result<ToolOutput, Error>> {
        Box::pin(Steps::new(self, cx.tool().name(), &args, cx.files()))
    }

    /// Sends RV_PACK_RENDER with the offered tools and the model as JSON
    /// text, and returns the tools the Plugin lists. The verb comes and
    /// goes in one call, so the future is ready at once.
    fn render<'a>(
        &'a self,
        cx: RenderContext<'a>,
        offered: Vec<ToolInfo>,
    ) -> BoxFuture<'a, Result<Vec<ToolInfo>, Error>> {
        Box::pin(std::future::ready(self.send_render(&cx, offered)))
    }
}

/// An offered tool in the form the contract lends it, with the text of
/// each field owned, so the `RvToolInfo` built from it can point at them.
struct Offered {
    name: String,
    wire_name: Option<String>,
    description: String,
    parameters: String,
    flags: u32,
}

impl Offered {
    /// The offered tool `tool`: its name without the Pack id, its wire
    /// name when it has one, its description, its parameters as compact
    /// JSON, and the one flag the contract carries.
    fn of(tool: &ToolInfo) -> Offered {
        Offered {
            name: tool.id.name().to_owned(),
            wire_name: tool.wire_name.clone(),
            description: tool.description.clone(),
            parameters: tool.parameters_schema.to_string(),
            flags: if tool.changes_files {
                RV_TOOL_CHANGES_FILES
            } else {
                0
            },
        }
    }

    /// The `RvToolInfo` of this tool, valid while the tool is.
    fn raw(&self) -> RvToolInfo {
        RvToolInfo {
            size: size_field::<RvToolInfo>(),
            flags: self.flags,
            name: rv_str(&self.name),
            wire_name: rv_str(self.wire_name.as_deref().unwrap_or("")),
            description: rv_str(&self.description),
            parameters: rv_str(&self.parameters),
        }
    }
}

/// The JSON text of an object that describes `model`, as the contract
/// lends it to a render: `context`, `description`, `name`, `provider` when
/// the catalog names one, `server`, and `thinking`, with the keys sorted.
/// A model of a mode this loader does not know reads `never`, the claim
/// that asks least of the model.
fn model_text(model: &ModelInfo) -> String {
    let thinking = match model.thinking() {
        ThinkingMode::Always => "always",
        ThinkingMode::Switchable => "switchable",
        _ => "never",
    };
    let mut object = serde_json::Map::new();
    object.insert("context".to_owned(), json!(model.context().get()));
    object.insert("description".to_owned(), json!(model.description()));
    object.insert("name".to_owned(), json!(model.id().name()));
    if let Some(provider) = model.provider() {
        object.insert("provider".to_owned(), json!(provider));
    }
    object.insert("server".to_owned(), json!(model.id().server()));
    object.insert("thinking".to_owned(), json!(thinking));
    Value::Object(object).to_string()
}

impl EntryPack {
    /// Sends RV_PACK_RENDER and reads what the Plugin lists. A Plugin that
    /// answers RV_ERR_UNSUPPORTED does not render, so the model is shown
    /// the offered tools as they are. A refusal, a misuse, and any other
    /// status fail the render with a message that says which.
    fn send_render(
        &self,
        cx: &RenderContext<'_>,
        offered: Vec<ToolInfo>,
    ) -> Result<Vec<ToolInfo>, Error> {
        let model = model_text(cx.model);
        let lent: Vec<Offered> = offered.iter().map(Offered::of).collect();
        let infos: Vec<RvToolInfo> = lent.iter().map(Offered::raw).collect();
        let mut call = self.new_call(RV_PACK_RENDER);
        let (status, mut session) = session::open(Verb::PackRender, |ctx| {
            let args = RvPackRenderArgs {
                size: size_field::<RvPackRenderArgs>(),
                reserved: 0,
                ctx,
                entry: rv_str(cx.entry),
                model: rv_str(&model),
                offered: if infos.is_empty() {
                    ptr::null()
                } else {
                    infos.as_ptr()
                },
                offered_len: infos.len(),
                refuse: render_refuse,
                add_tool: render_add_tool,
            };
            call.args = (&raw const args).cast();
            // SAFETY: inserting the Plugin required its entry point to
            // follow the contract and not unwind, `call` is a live
            // `RvCall` of this instance that nothing else uses, and its
            // `args` and everything they point at are live for the call.
            unsafe { self.send(&mut call) }
        });
        let failed = |message: String| make_tool_error(ErrorCode::ToolOther, message);
        if let Some(reason) = session.refusal.take() {
            return Err(failed(format!(
                "the Plugin {} declined to render the Pack {}: {reason}",
                self.plugin.name(),
                self.id
            )));
        }
        if let Some(misuse) = session.misuse.take() {
            return Err(failed(format!(
                "the Plugin {} misused the render functions for the Pack {}: {misuse}",
                self.plugin.name(),
                self.id
            )));
        }
        match status {
            RV_OK => {}
            RV_ERR_UNSUPPORTED => return Ok(offered),
            other => {
                return Err(failed(format!(
                    "the Plugin {} answered RV_PACK_RENDER for the Pack {} with status {other}",
                    self.plugin.name(),
                    self.id
                )));
            }
        }
        mem::take(&mut session.tools)
            .into_iter()
            .map(|tool| tool_info(&self.id, tool))
            .collect()
    }
}

/// Records the Plugin's refusal to make a Pack instance, validated;
/// returns RV_ERR_REFUSED, or RV_ERR_MISUSE for a `ctx` it doesn't answer
/// or a second refusal.
pub(crate) unsafe extern "C" fn refuse(ctx: *mut c_void, reason: RvStr) -> RvStatus {
    session::refuse(ctx, &PACK_NEW, reason)
}

/// Records one tool the instance adds, validated; returns RV_OK or
/// RV_ERR_MISUSE.
pub(crate) unsafe extern "C" fn add_tool(ctx: *mut c_void, info: *const RvToolInfo) -> RvStatus {
    // SAFETY: the contract makes `info` point at a readable `RvToolInfo`
    // with valid strings during this call.
    unsafe {
        add(
            ctx,
            &PACK_NEW,
            info,
            "two tools of one Pack instance share a name",
        )
    }
}

/// Records the Plugin's refusal to render, validated; returns
/// RV_ERR_REFUSED, or RV_ERR_MISUSE for a `ctx` it doesn't answer or a
/// second refusal.
pub(crate) unsafe extern "C" fn render_refuse(ctx: *mut c_void, reason: RvStr) -> RvStatus {
    session::refuse(ctx, &PACK_RENDER, reason)
}

/// Records one tool the render lists, validated; returns RV_OK or
/// RV_ERR_MISUSE.
pub(crate) unsafe extern "C" fn render_add_tool(
    ctx: *mut c_void,
    info: *const RvToolInfo,
) -> RvStatus {
    // SAFETY: the contract makes `info` point at a readable `RvToolInfo`
    // with valid strings during this call.
    unsafe {
        add(
            ctx,
            &PACK_RENDER,
            info,
            "two tools of one render share a name",
        )
    }
}

/// Records one tool that the open call of `family` is given, validated:
/// the rules of a tool's fields are the same for RV_PACK_NEW and
/// RV_PACK_RENDER. `twice` names a repeated name. Returns RV_OK or
/// RV_ERR_MISUSE.
///
/// # Safety
/// `info` must be NULL, or point at a readable `RvToolInfo` whose strings
/// are valid for their lengths, for the duration of the call.
unsafe fn add(
    ctx: *mut c_void,
    family: &Family,
    info: *const RvToolInfo,
    twice: &'static str,
) -> RvStatus {
    answer(ctx, family, |session| {
        // SAFETY: the caller makes `info` readable if it is not NULL.
        let info = unsafe { TOOL_INFO.read(info) }?;
        if info.flags & !RV_TOOL_CHANGES_FILES != 0 {
            return Err("the tool's flags set a bit that is not one of the RV_TOOL constants");
        }
        // SAFETY: the contract makes each string valid for its length.
        let name = unsafe { TOOL_NAME.read(info.name) }?;
        if !is_segment(&name) {
            return Err("the tool's name is not one name segment, such as `fetch`");
        }
        if session.tools.iter().any(|tool| tool.name == name) {
            return Err(twice);
        }
        // SAFETY: as above.
        let wire_name = unsafe { WIRE_NAME.read(info.wire_name) }?;
        if !wire_name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
        {
            return Err("the tool's wire name is not ASCII letters, digits, `_`, and `-`");
        }
        // SAFETY: as above.
        let description = unsafe { DESCRIPTION.read(info.description) }?;
        if description.is_empty() {
            return Err("the tool's description is empty");
        }
        // SAFETY: as above.
        let parameters = unsafe { PARAMETERS.read(info.parameters) }?;
        let parameters = serde_json::from_str::<Value>(&parameters)
            .ok()
            .filter(Value::is_object)
            .ok_or("the tool's parameter schema is not the JSON text of an object")?;
        session.tools.push(AddedTool {
            name,
            wire_name: (!wire_name.is_empty()).then_some(wire_name),
            description,
            parameters,
            changes_files: info.flags & RV_TOOL_CHANGES_FILES != 0,
        });
        Ok(RV_OK)
    })
}

/// Sends RV_PACK_NEW for the Pack `plugin` inserted under the name `pack`,
/// and returns the instance pointer the Plugin set and the tools it
/// added. A call this rejects gets no RV_PACK_DELETE.
///
/// The message of each failure is the reason the run goes without the
/// Pack: the Plugin's own reason for a refusal, how the Plugin broke the
/// contract, or the status it returned.
fn send_pack_new(plugin: &Plugin, pack: &str) -> Result<(*mut c_void, Vec<AddedTool>), Error> {
    let mut call = new_call(RV_PACK_NEW, plugin.instance().plugin());
    let (status, mut session) = session::open(Verb::PackNew, |ctx| {
        let args = RvPackNewArgs {
            size: size_field::<RvPackNewArgs>(),
            reserved: 0,
            ctx,
            pack: rv_str(pack),
            refuse,
            add_tool,
        };
        call.args = (&raw const args).cast();
        // SAFETY: inserting the Plugin required its entry point to follow
        // the contract and not unwind, `call` is a live `RvCall` that
        // nothing else uses, and its `args` is live for the call.
        unsafe { (plugin.instance().entry())(&raw mut call) }
    });
    let unavailable = |message: String| make_tool_error(ErrorCode::ToolOther, message);
    if let Some(reason) = session.refusal.take() {
        return Err(unavailable(reason));
    }
    if let Some(misuse) = session.misuse.take() {
        return Err(unavailable(format!(
            "the Plugin {} misused the Pack functions: {misuse}",
            plugin.name()
        )));
    }
    if status != RV_OK {
        return Err(unavailable(format!(
            "the Plugin {} answered RV_PACK_NEW with status {status}",
            plugin.name()
        )));
    }
    Ok((call.pack_run, mem::take(&mut session.tools)))
}
