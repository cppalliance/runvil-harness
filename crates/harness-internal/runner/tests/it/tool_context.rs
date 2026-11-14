//! The context a tool call lends its Pack: the Pack writes the files of
//! the setup's filesystem through `cx.access()`, where the Host reads
//! them after the run, that access and the run's store share no file,
//! and `cx.origin()` names the section's script as the caller.

use std::sync::Arc;

use harness_runner::RunSetup;
use harness_runner::recorder::MemoryRecorder;
use runvil_engine::make_tool_error;
use runvil_engine::vfs::{Origin, VfsRef};
use runvil_engine::{Error, ErrorCode};
use runvil_plugin::{
    BoxFuture, Pack, PackId, PackInfo, ToolContext, ToolId, ToolInfo, ToolOutput, VfsError,
};
use serde_json::{Value, json};

use crate::prepare::{building, report_of, setup_over};
use crate::support::Make;

/// What the fixture tool writes to `/from-tool.md`.
const WRITTEN: &str = "written through the context";

/// A prompt that writes `from-store.md` to its store, calls the fixture
/// tool by id from the section's script, and returns the tool's answer
/// beside whether its store holds the file the tool wrote.
const CALLS_RECORD: &str = "---\nname: calls-record\ndescription: d\nrunvil: 0\n\
    uses:\n  - tests/context\n---\n\n\
    ## Only\n\n```lua\n\
    store.write('from-store.md', 'kept in the store')\n\
    local answer = tools.call('context/record')\n\
    return answer .. '|' .. tostring(store.exists('from-tool.md'))\n```\n";

/// The fixture Pack `context`: one tool, `context/record`, which writes
/// [`WRITTEN`] to `/from-tool.md` through the access its call lends it
/// and answers with the caller its origin names beside whether that
/// access finds `/from-store.md`.
const CONTEXT: (PackInfo, Make) = (PackInfo::new("tests/context"), new_record);

struct Record {
    tools: Vec<ToolInfo>,
}

#[expect(
    clippy::unnecessary_wraps,
    reason = "the PackFactory signature fixes the return type"
)]
fn new_record(name: &PackId) -> Result<Arc<dyn Pack>, Error> {
    let record = ToolInfo::new(
        ToolId::parse(&format!("{name}/record")).unwrap(),
        "Write a file through the call's access and name the caller.",
        json!({ "type": "object", "properties": {} }),
    );
    Ok(Arc::new(Record {
        tools: vec![record],
    }))
}

impl Pack for Record {
    fn tools(&self) -> Vec<ToolInfo> {
        self.tools.clone()
    }

    fn call<'a>(
        &'a self,
        cx: ToolContext<'a>,
        _args: Value,
    ) -> BoxFuture<'a, Result<ToolOutput, Error>> {
        Box::pin(async move {
            let refused =
                |error: VfsError| make_tool_error(ErrorCode::ToolOther, format!("record: {error}"));
            let files = cx.access();
            files
                .write("/from-tool.md", WRITTEN.as_bytes())
                .map_err(refused)?;
            let stored = files.exists("/from-store.md").map_err(refused)?;
            Ok(ToolOutput::trusted(format!(
                "{:?}|{stored}",
                cx.origin().caller
            )))
        })
    }
}

#[tokio::test]
async fn a_tool_writes_the_setups_filesystem_through_its_context_and_shares_no_file_with_the_store()
{
    let vfs = VfsRef::default();
    let setup = RunSetup {
        vfs: vfs.clone(),
        ..setup_over(&Arc::new(MemoryRecorder::new()))
    };
    let report = report_of(CALLS_RECORD, building(CONTEXT), setup).await;
    assert_eq!(
        report.result,
        Ok("Script|false|false".to_owned()),
        "the tool's origin names the script, the tool finds no file the store wrote, and the \
         store holds no file the tool wrote"
    );

    let files = vfs.acquire(Origin::new("tool context test")).unwrap();
    assert_eq!(
        files.read_string("/from-tool.md"),
        Ok(WRITTEN.to_owned()),
        "the Host reads the tool's file from the setup's filesystem after the run"
    );
}
