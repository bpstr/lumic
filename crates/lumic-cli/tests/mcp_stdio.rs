use rmcp::{
    ServiceExt,
    transport::{ConfigureCommandExt, TokioChildProcess},
};

#[tokio::test]
async fn installed_cli_serves_mcp_over_stdio() -> anyhow::Result<()> {
    let transport = TokioChildProcess::new(
        tokio::process::Command::new(env!("CARGO_BIN_EXE_lumic")).configure(|command| {
            command.args(["mcp", "serve"]);
        }),
    )?;
    let client = ().serve(transport).await?;
    let resources = client.list_all_resources().await?;
    let tools = client.list_all_tools().await?;
    assert!(
        resources
            .iter()
            .any(|resource| resource.uri == "lumic://server/status")
    );
    assert!(tools.iter().any(|tool| tool.name == "inspect_server"));
    client.cancel().await?;
    Ok(())
}
