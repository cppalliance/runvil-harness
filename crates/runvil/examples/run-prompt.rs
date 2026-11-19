//! Runs a prompt through the Host API and prints its result.

use runvil::{Error, Host};

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Error> {
    let prompts = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/prompts");
    let host = Host::new(prompts);
    let greeting = host
        .run("greet.md")
        .args(r#"{"text":"desk"}"#)
        .text()
        .await?;
    assert_eq!(greeting, "Hello, desk.");
    println!("{greeting}");
    Ok(())
}
