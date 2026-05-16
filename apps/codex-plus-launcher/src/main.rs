use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    println!("Codex++ launcher {}", codex_plus_core::version::VERSION);
    Ok(())
}
