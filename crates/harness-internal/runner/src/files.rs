//! The Harness's half of a prompt's declared output file: the `output:`
//! path read after a run.
//!
//! The read goes through the handle's store view
//! ([`VfsRef::acquire_store`]), so the store's strict path rules apply to
//! the declared path, which the parser takes as written, and the handle's
//! policy and op sink see the read like any other operation. It is
//! synchronous, like the VFS, and the Harness runs it inline in the run's
//! future.

use runvil_engine::vfs::{
    Origin, VfsError, VfsOp, VfsOutcome, VfsRef, make_store_error, perform_vfs_op,
};
use runvil_engine::{Error, ErrorCode, ErrorFact};

/// Reads the prompt's declared output file at `path` from `vfs`'s store.
///
/// # Errors
/// Returns the store's failure, [`VfsError::NotFound`] when the run never
/// wrote the file.
pub fn read_output(vfs: &VfsRef, path: &str) -> Result<String, VfsError> {
    let view = vfs.acquire_store(Origin::new(format!("output: {path}")))?;
    let read = VfsOp::Read {
        path: path.to_owned(),
        start: None,
        end: None,
    };
    match perform_vfs_op(&view, read)? {
        VfsOutcome::Text(text) => Ok(text),
        other => Err(VfsError::Backend {
            message: format!("a whole-file store read answered {other:?} instead of text"),
        }),
    }
}

/// The output a run's report carries: what a completed run left at its
/// declared `output:` path in `vfs`'s store, or why the report omits it.
///
/// A run whose `result` is an error did not complete and fails with
/// [`make_not_completed_error`], and a prompt with no `output:` file with `NoOutput`
/// and the `Reason` fact `undeclared`. A completed run that left no file
/// fails with `NotFound`, and a refused read with the store failure's
/// code. Both carry the declared path as the `Path` fact. The prose is the
/// code's default.
pub(crate) fn report_output(
    vfs: &VfsRef,
    path: Option<&str>,
    result: &Result<String, Error>,
) -> Result<String, Error> {
    if result.is_err() {
        return Err(make_not_completed_error());
    }
    let path = path.ok_or_else(|| {
        make_reasoned_error(
            ErrorCode::NoOutput,
            "undeclared",
            "the prompt declares no `output:` file",
        )
    })?;
    read_output(vfs, path).map_err(|error| {
        let mut converted = make_store_error(&error);
        converted.message = if matches!(error, VfsError::NotFound { .. }) {
            format!("the run completed without writing its declared output file `{path}`")
        } else {
            format!("the output file `{path}` could not be read: {error}")
        };
        converted.facts.insert(ErrorFact::Path, path.into());
        converted
    })
}

/// The output failure of a run that failed or was canceled, possibly
/// before it began: `NoOutput` with the `Reason` fact `not_completed`.
pub(crate) fn make_not_completed_error() -> Error {
    make_reasoned_error(
        ErrorCode::NoOutput,
        "not_completed",
        "the run did not complete",
    )
}

/// A failure under `code` with `message`, the `Reason` fact `reason`, and
/// the code's default prose.
fn make_reasoned_error(code: ErrorCode, reason: &str, message: impl Into<String>) -> Error {
    let mut error = Error::new(code, message, code.prose());
    error.facts.insert(ErrorFact::Reason, reason.into());
    error
}

#[cfg(test)]
mod tests {
    //! The report's output: a completed run's declared file, and the failure
    //! that replaces it when the run did not complete, the prompt declares no
    //! output, the run left no file, or the store refuses the read.

    use runvil_engine::vfs::{Origin, VfsRef};
    use runvil_engine::{Error, ErrorCode, ErrorFact};
    use serde_json::json;

    use super::report_output;

    /// The result of a completed run.
    const COMPLETED: Result<String, Error> = Ok(String::new());

    /// A default handle whose store holds `contents` at `path`.
    fn seeded(path: &str, contents: &str) -> VfsRef {
        let vfs = VfsRef::default();
        vfs.acquire_store(Origin::new("files test"))
            .expect("the default handle declares a store")
            .write(path, contents.as_bytes())
            .expect("the seed is written");
        vfs
    }

    #[test]
    fn a_completed_run_reports_its_declared_output_file() {
        let vfs = seeded("report.md", "done");
        assert_eq!(
            report_output(&vfs, Some("report.md"), &COMPLETED),
            Ok("done".to_owned())
        );
    }

    #[test]
    fn a_run_that_did_not_complete_reports_no_output() {
        let failed = Error::new(ErrorCode::Internal, "", ErrorCode::Internal.prose());
        for result in [Err(Error::canceled()), Err(failed)] {
            let error = report_output(&VfsRef::default(), Some("report.md"), &result)
                .expect_err("a run that did not complete has no output");
            assert_eq!(error.code, ErrorCode::NoOutput);
            assert_eq!(error.message, "the run did not complete");
            assert_eq!(
                error.facts.get(&ErrorFact::Reason),
                Some(&json!("not_completed"))
            );
        }
    }

    #[test]
    fn a_prompt_without_an_output_declaration_reports_no_output() {
        let error = report_output(&VfsRef::default(), None, &COMPLETED)
            .expect_err("an undeclared output has no text");
        assert_eq!(error.code, ErrorCode::NoOutput);
        assert_eq!(error.message, "the prompt declares no `output:` file");
        assert_eq!(error.prose, ErrorCode::NoOutput.prose());
        assert_eq!(
            error.facts.get(&ErrorFact::Reason),
            Some(&json!("undeclared"))
        );
    }

    #[test]
    fn a_completed_run_without_its_output_file_reports_not_found() {
        let error = report_output(&VfsRef::default(), Some("report.md"), &COMPLETED)
            .expect_err("the run wrote no output file");
        assert_eq!(error.code, ErrorCode::NotFound);
        assert_eq!(
            error.message,
            "the run completed without writing its declared output file `report.md`"
        );
        assert_eq!(error.facts.get(&ErrorFact::Path), Some(&json!("report.md")));
    }

    #[test]
    fn a_refused_read_reports_the_stores_failure_with_its_cause_in_the_message() {
        let vfs = seeded("out/report.md", "done");
        let error = report_output(&vfs, Some("out"), &COMPLETED)
            .expect_err("a directory is not an output file");
        assert_eq!(error.code, ErrorCode::IsADirectory);
        assert!(
            error
                .message
                .starts_with("the output file `out` could not be read: "),
            "{error:?}"
        );
        assert_eq!(error.prose, ErrorCode::IsADirectory.prose());
        assert_eq!(error.facts.get(&ErrorFact::Path), Some(&json!("out")));
    }
}
