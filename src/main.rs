#[tokio::main(flavor = "multi_thread")]
async fn main() -> anyhow::Result<()> {
    cattail::run().await
}
