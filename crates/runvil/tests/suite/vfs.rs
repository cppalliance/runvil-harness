//! Guarding and watching the files a Host hands a run's tool calls: a
//! policy refuses a path off the desk before the operation watcher sees
//! it, and while the run waits on its inbox, the Host's edit of a file a
//! tool call read conflicts and leaves the file as it was. A run's store
//! is no part of those files, so the policy and the watcher never see a
//! store operation.

use std::sync::{Arc, Mutex, PoisonError};

use runvil::harness::{MemoryRecorder, Packs, RecordKind, RunId};
use runvil::packs::PackId;
use runvil::plugin::{Pack, PackInfo, PackRegistry, ToolContext};
use runvil::vfs::{MemoryBackend, Op, Origin, Policy, Verdict, VfsError, VfsPath, VfsRef};
use runvil::{
    BoxFuture, Error, ErrorCode, RunSetup, ToolId, ToolInfo, ToolOutput, make_tool_error,
};
use serde_json::{Value, json};

use crate::support::{Offline, launch, no_packs};

/// Reads the notes through the desk's tool, waits for the operator's
/// message, and returns the notes and the message.
const REVIEW: &str = concat!(
    "---\nname: review\ndescription: Reads the notes, then waits for the operator\nrunvil: 0\n",
    "uses:\n  - user\n  - acme/desk\n",
    "---\n\n## Approve\n\n```lua\n",
    "local notes = tools.call('desk/read')\n",
    "local answer = tasks.receive()\n",
    "return notes .. ' ' .. answer\n",
    "```\n",
);

/// Keeps the notes in the run's store and returns them read back.
const KEEPS: &str = concat!(
    "---\nname: keeps\ndescription: Keeps the notes in the run's store\nrunvil: 0\n",
    "---\n\n## Only\n\n```lua\n",
    "store.write('notes.md', 'Ship on Friday.')\n",
    "return store.read('notes.md')\n",
    "```\n",
);

/// The Pack `acme/desk`, whose one tool, `desk/read`, reads `/notes.md`
/// through the access its call is lent.
const DESK: PackInfo = PackInfo::new("acme/desk").description("Reads the notes on the desk");

/// One run's instance of [`DESK`].
struct Desk {
    read: ToolInfo,
}

impl Pack for Desk {
    fn tools(&self) -> Vec<ToolInfo> {
        vec![self.read.clone()]
    }

    fn call<'a>(
        &'a self,
        cx: ToolContext<'a>,
        _args: Value,
    ) -> BoxFuture<'a, Result<ToolOutput, Error>> {
        Box::pin(async move {
            // The notes are the Host's own text, so the script reads them
            // as trusted output, verbatim.
            cx.access()
                .read_string("/notes.md")
                .map(ToolOutput::trusted)
                .map_err(|error| {
                    let text = format!("{} cannot read /notes.md: {error}", cx.tool());
                    make_tool_error(ErrorCode::ToolOther, text)
                })
        })
    }
}

/// Makes one run's [`Desk`], whose tool sits under the Pack id `id`.
fn new_desk(id: &PackId) -> Result<Arc<dyn Pack>, Error> {
    let read = ToolInfo::new(
        ToolId::parse(&format!("{id}/read"))?,
        "Reads the notes on the desk.",
        json!({ "type": "object", "properties": {} }),
    );
    Ok(Arc::new(Desk { read }))
}

/// A Host's Packs: the built-ins and [`DESK`].
fn desk_packs() -> Packs {
    let mut registry = PackRegistry::new();
    if let Err(error) = registry.insert(DESK, new_desk) {
        panic!("the registry takes the desk Pack: {error:?}");
    }
    Packs::new(registry)
}

/// Each watched operation's origin label and file.
type Seen = Arc<Mutex<Vec<(String, String)>>>;

/// Refuses every path except the notes on the desk.
struct DeskFiles;

impl Policy for DeskFiles {
    fn check(&self, op: Op, path: &VfsPath) -> Verdict {
        let desk_file = path.as_str() == "/notes.md";
        match op {
            Op::Read
            | Op::Write
            | Op::Append
            | Op::Delete
            | Op::Rename
            | Op::Mkdir
            | Op::Copy
            | Op::Exists
            | Op::Glob
            | Op::List
            | Op::Stat
            | Op::Symlink
            | Op::ReadLink
            | Op::Chmod
                if desk_file =>
            {
                Verdict::Allow
            }
            // `Op` is `#[non_exhaustive]`: an operation added later is
            // refused until the desk names it.
            _ => Verdict::Deny(format!("{op:?} on {path} is not a desk file operation")),
        }
    }
}

/// Refuses every operation on every path.
struct Shut;

impl Policy for Shut {
    fn check(&self, op: Op, path: &VfsPath) -> Verdict {
        Verdict::Deny(format!("{op:?} on {path} is refused: the desk is shut"))
    }
}

/// An empty memory filesystem behind `policy`, beside the origins its
/// watcher saw.
fn watched(policy: impl Policy + Sync + 'static) -> (VfsRef, Seen) {
    let seen = Seen::default();
    let sink = Arc::clone(&seen);
    let vfs = VfsRef::builder()
        .mount("/", MemoryBackend::new())
        .policy(policy)
        .on_op(move |event| {
            sink.lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push((event.origin().label.clone(), event.origin().file.clone()));
        })
        .build();
    (vfs, seen)
}

/// Yields until the run the recorder began first waits on its inbox,
/// failing if it never does within a few thousand polls.
async fn until_the_run_waits_on_its_inbox(recorder: &MemoryRecorder) {
    for _ in 0..10_000 {
        let waits = recorder.records(RunId::from_raw(1)).iter().any(|record| {
            record.kind == RecordKind::Event && record.payload["kind"] == "inbox_waiting"
        });
        if waits {
            return;
        }
        tokio::task::yield_now().await;
    }
    panic!("the run never waited on its inbox");
}

#[test]
fn a_policy_refuses_a_path_off_the_desk_before_the_watcher_sees_it() {
    let (vfs, seen) = watched(DeskFiles);
    vfs.acquire(Origin::new("desk notes"))
        .expect("a memory filesystem acquires")
        .write("/notes.md", b"Ship on Friday.")
        .expect("the policy allows a desk file");
    let stray = vfs
        .acquire(Origin::new("desk stray"))
        .expect("a memory filesystem acquires")
        .write("/secret.md", b"x");
    assert!(matches!(stray, Err(VfsError::PermissionDenied { .. })));
    assert_eq!(
        seen.lock().unwrap()[..],
        [("desk notes".to_owned(), file!().to_owned())]
    );
}

#[tokio::test]
async fn a_hosts_edit_of_a_file_the_run_read_conflicts_while_the_run_waits_on_its_inbox() {
    let (vfs, seen) = watched(DeskFiles);
    vfs.acquire(Origin::new("desk notes"))
        .expect("a memory filesystem acquires")
        .write("/notes.md", b"Ship on Friday.")
        .expect("the policy allows a desk file");

    let recorder = Arc::new(MemoryRecorder::new());
    let setup = RunSetup {
        has_user: true,
        vfs: vfs.clone(),
        ..RunSetup::new(recorder.clone(), Arc::new(Offline))
    };
    let harness = launch(desk_packs(), REVIEW, setup);
    let control = harness.control();
    // While the run waits on its inbox, the Host edits the notes the run's
    // tool call already read, then sends the approval.
    let (report, edited) = tokio::join!(harness.run(), async {
        until_the_run_waits_on_its_inbox(&recorder).await;
        let edit = vfs
            .acquire(Origin::new("desk edit"))
            .and_then(|access| access.write("/notes.md", b"Ship on Monday."));
        assert!(control.send("Approved.".to_owned()));
        edit
    });
    let report = report.expect("the run is reported");

    assert!(matches!(edited, Err(VfsError::Conflict { .. })));
    let notes = vfs
        .acquire(Origin::new("desk check"))
        .expect("a memory filesystem acquires")
        .read_string("/notes.md")
        .expect("the notes stay readable");
    assert_eq!(notes, "Ship on Friday.");
    assert_eq!(
        seen.lock().unwrap()[..],
        [
            ("desk notes".to_owned(), file!().to_owned()),
            ("desk/read".to_owned(), "review".to_owned()),
            ("desk check".to_owned(), file!().to_owned()),
        ],
        "the tool call's read names the prompt by its frontmatter `name`, and the refused \
         edit never reaches the watcher"
    );
    assert_eq!(
        report.result.expect("the run completed"),
        "Ship on Friday. Approved."
    );
}

#[tokio::test]
async fn a_hosts_policy_and_watcher_never_see_a_store_operation() {
    let (vfs, seen) = watched(Shut);
    let setup = RunSetup {
        vfs: vfs.clone(),
        ..RunSetup::new(Arc::new(MemoryRecorder::new()), Arc::new(Offline))
    };
    let report = launch(no_packs(), KEEPS, setup)
        .run()
        .await
        .expect("the run is reported");

    assert_eq!(
        report.result,
        Ok("Ship on Friday.".to_owned()),
        "a policy that refuses every path refuses no store operation"
    );
    let look = vfs
        .acquire(Origin::new("desk look"))
        .and_then(|access| access.exists("/notes.md"));
    assert!(
        matches!(look, Err(VfsError::PermissionDenied { .. })),
        "the same policy refuses the Host's own look at that path: {look:?}"
    );
    let seen = seen.lock().unwrap();
    assert!(seen.is_empty(), "the watcher saw {seen:?}");
}
