//! The snapshot's renders and its routing by Pack: a render asks each
//! usable Pack once for each set it was offered into, with the section
//! entry and the round's model, and answers every Pack asked in the order
//! asked, a Pack the run cannot use keeping its tools as offered; and a
//! call goes to the Pack its id names when the catalog lacks the tool.

use std::num::NonZeroU32;

use runvil_engine::effect::PackToolSets;
use runvil_plugin::{ModelId, ModelInfo, RenderContext, ThinkingMode};

use super::*;

/// One render a [`Rendering`] Pack performed.
#[derive(Debug, Clone, PartialEq)]
struct Rendered {
    pack: String,
    entry: String,
    model: ModelInfo,
    offered: Vec<String>,
}

/// A [`Listed`] Pack whose render logs what it is asked in `renders`
/// and adds `<pack>/extra`, a tool only the model sees.
struct Rendering {
    name: PackId,
    listed: Arc<dyn Pack>,
    renders: Arc<Mutex<Vec<Rendered>>>,
    /// Whether the render fails, after it logs what it was asked.
    fails: bool,
}

/// The factory of a [`Rendering`] Pack made from a [`listed`]
/// configuration, which logs each render of each instance it makes in
/// `renders`.
fn rendering(config: Value, renders: Arc<Mutex<Vec<Rendered>>>) -> Factory {
    Box::new(move |name: &PackId| -> Result<Arc<dyn Pack>, Error> {
        Ok(Arc::new(Rendering {
            name: name.clone(),
            listed: new_listed(name, &config),
            renders: Arc::clone(&renders),
            fails: config["fails"] == true,
        }))
    })
}

impl Pack for Rendering {
    fn tools(&self) -> Vec<ToolInfo> {
        self.listed.tools()
    }

    fn call<'a>(
        &'a self,
        cx: ToolContext<'a>,
        args: Value,
    ) -> BoxFuture<'a, Result<ToolOutput, Error>> {
        self.listed.call(cx, args)
    }

    fn render<'a>(
        &'a self,
        cx: RenderContext<'a>,
        mut offered: Vec<ToolInfo>,
    ) -> BoxFuture<'a, Result<Vec<ToolInfo>, Error>> {
        self.renders.lock().unwrap().push(Rendered {
            pack: self.name.to_string(),
            entry: cx.entry.to_owned(),
            model: cx.model.clone(),
            offered: offered.iter().map(|tool| tool.id.to_string()).collect(),
        });
        if self.fails {
            return Box::pin(async {
                Err(make_tool_error(
                    ErrorCode::ToolOther,
                    "no tools for this model",
                ))
            });
        }
        offered.push(listed_tool(&format!("{}/extra", self.name)));
        Box::pin(async move { Ok(offered) })
    }
}

/// The model a render here is asked for.
fn model() -> ModelInfo {
    ModelInfo::new(
        ModelId::gateway("m").unwrap(),
        "The round's model",
        NonZeroU32::new(4096).unwrap(),
        ThinkingMode::Never,
    )
}

/// The Pack `pack`'s offered tools, `static_set` then `dynamic_set`.
fn sets(pack: &str, static_set: &[&str], dynamic_set: &[&str]) -> PackToolSets<ToolInfo> {
    let tools = |ids: &[&str]| ids.iter().map(|id| listed_tool(id)).collect();
    PackToolSets::new(name(pack), tools(static_set), tools(dynamic_set))
}

/// Owned copies of `ids`.
fn owned(ids: &[&str]) -> Vec<String> {
    ids.iter().map(|id| (*id).to_owned()).collect()
}

/// Each Pack of `answer` with its tool ids per set, in order.
fn ids(answer: &[PackToolSets<ToolInfo>]) -> Vec<(String, Vec<String>, Vec<String>)> {
    let ids = |tools: &[ToolInfo]| tools.iter().map(|tool| tool.id.to_string()).collect();
    answer
        .iter()
        .map(|sets| {
            (
                sets.pack.to_string(),
                ids(&sets.static_set),
                ids(&sets.dynamic_set),
            )
        })
        .collect()
}

#[tokio::test]
async fn a_render_asks_each_usable_pack_once_per_set_it_was_offered_into_and_answers_in_order() {
    let renders = Arc::new(Mutex::new(Vec::new()));
    let host = host_with(vec![
        (
            PackInfo::new("runvil/one"),
            rendering(offering(&["one/a", "one/b"]), Arc::clone(&renders)),
        ),
        (
            PackInfo::new("runvil/two"),
            rendering(offering(&["two/a"]), Arc::clone(&renders)),
        ),
        (PackInfo::new("runvil/backed"), backed(None)),
    ]);
    let run = begin(&host, BTreeMap::new(), &prompt(""));
    let offered = vec![
        sets("two", &["two/a"], &[]),
        sets("one", &["one/a"], &["one/b"]),
        sets("backed", &[], &["backed/a"]),
    ];
    let answer = run
        .render("0.1".to_owned(), model(), offered)
        .await
        .expect("no Pack's render fails");

    let asked = |pack: &str, offered: &[&str]| Rendered {
        pack: pack.to_owned(),
        entry: "0.1".to_owned(),
        model: model(),
        offered: owned(offered),
    };
    assert_eq!(
        *renders.lock().unwrap(),
        [
            asked("two", &["two/a"]),
            asked("one", &["one/a"]),
            asked("one", &["one/b"]),
        ],
        "each usable Pack is asked once per set it was offered into, with the entry and the \
         model, and never for an empty set"
    );
    assert_eq!(
        ids(&answer),
        [
            ("two".to_owned(), owned(&["two/a", "two/extra"]), owned(&[])),
            (
                "one".to_owned(),
                owned(&["one/a", "one/extra"]),
                owned(&["one/b", "one/extra"])
            ),
            ("backed".to_owned(), owned(&[]), owned(&["backed/a"])),
        ],
        "every Pack asked is answered in the order asked, and the one the run cannot use keeps \
         its tools as offered"
    );
}

#[tokio::test]
async fn a_pack_whose_render_fails_fails_the_whole_render_with_an_error_that_names_it() {
    let renders = Arc::new(Mutex::new(Vec::new()));
    let mut failing = offering(&["two/a"]);
    failing["fails"] = Value::Bool(true);
    let host = host_with(vec![
        (
            PackInfo::new("runvil/one"),
            rendering(offering(&["one/a"]), Arc::clone(&renders)),
        ),
        (
            PackInfo::new("runvil/two"),
            rendering(failing, Arc::clone(&renders)),
        ),
    ]);
    let run = begin(&host, BTreeMap::new(), &prompt(""));
    let offered = vec![sets("one", &["one/a"], &[]), sets("two", &["two/a"], &[])];
    let error = run
        .render("0.1".to_owned(), model(), offered)
        .await
        .expect_err("a failing render fails the whole render");
    let shown = crate::display_chain(&error);
    assert!(
        shown.contains("the two Pack could not render its tools"),
        "the error names the Pack: {shown}"
    );
    assert!(
        shown.contains("no tools for this model"),
        "the Pack's own error is the cause: {shown}"
    );
    let asked: Vec<String> = renders
        .lock()
        .unwrap()
        .iter()
        .map(|render| render.pack.clone())
        .collect();
    assert_eq!(
        asked,
        ["one", "two"],
        "the Packs before the failing one were asked, and the failing one was too"
    );
}

#[tokio::test]
async fn a_call_goes_to_the_pack_its_id_names_when_the_catalog_lacks_the_tool() {
    let host = host(&[(info("runvil/one"), offering(&["one/echo"]))]);
    let run = begin(&host, BTreeMap::new(), &prompt(""));
    let extra = ToolId::parse("one/extra").unwrap();
    assert!(
        run.catalog().get(&extra).is_none(),
        "only a render shows one/extra"
    );
    let answer = call(&run, "one/extra")
        .await
        .expect("the call reaches the one Pack");
    assert_eq!(answer.text(), "one:one/extra");
}
