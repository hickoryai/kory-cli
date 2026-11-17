use agent_client_protocol::schema::v1::{
    CreateTerminalRequest, InitializeRequest, InitializeResponse, KillTerminalRequest,
    NewSessionRequest, NewSessionResponse, ReleaseTerminalRequest, TerminalOutputRequest,
    WaitForTerminalExitRequest,
};
use agent_client_protocol::schema::ProtocolVersion;
use agent_client_protocol::{on_receive_request, Agent, ByteStreams};
use goose::acp::{AcpProvider, AcpProviderConfig};
use goose::config::GooseMode;
use goose::providers::base::Provider;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::time::{timeout, Duration};
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

/// Exercise the actual provider's ACP dispatch loop: a pending wait must not
/// prevent the kill request that makes that same wait complete.
#[tokio::test]
async fn terminal_wait_allows_output_and_kill_on_the_same_connection() {
    let (client_read, agent_write) = tokio::io::duplex(64 * 1024);
    let (agent_read, client_write) = tokio::io::duplex(64 * 1024);
    let (connection_tx, connection_rx) = tokio::sync::oneshot::channel();
    let connection_tx = Arc::new(Mutex::new(Some(connection_tx)));
    let agent = tokio::spawn(async move {
        Agent
            .builder()
            .on_receive_request(
                async |_req: InitializeRequest, responder, _cx| {
                    responder.respond(InitializeResponse::new(ProtocolVersion::LATEST))
                },
                on_receive_request!(),
            )
            .on_receive_request(
                async move |_req: NewSessionRequest, responder, cx| {
                    responder.respond(NewSessionResponse::new("terminal-session"))?;
                    connection_tx.lock().unwrap().take().unwrap().send(cx).ok();
                    Ok(())
                },
                on_receive_request!(),
            )
            .connect_to(ByteStreams::new(
                agent_write.compat_write(),
                agent_read.compat(),
            ))
            .await
    });
    let provider = AcpProvider::connect_with_transport(
        "terminal-fixture".into(),
        GooseMode::default(),
        AcpProviderConfig {
            command: "unused".into(),
            args: vec![],
            env: vec![],
            env_remove: vec![],
            work_dir: std::env::temp_dir(),
            mcp_servers: vec![],
            session_mode_id: None,
            session_config_options: vec![],
            model_config_option_id: None,
            mode_mapping: HashMap::new(),
            terminal: true,
            notification_callback: None,
        },
        ByteStreams::new(client_write.compat_write(), client_read.compat()),
    )
    .await
    .unwrap();
    let connection = connection_rx.await.unwrap();
    let terminal = connection
        .send_request(
            CreateTerminalRequest::new("terminal-session", "sleep").args(vec!["3600".into()]),
        )
        .block_task()
        .await
        .unwrap()
        .terminal_id;
    let result = timeout(Duration::from_secs(10), async {
        let waiting = connection
            .send_request(WaitForTerminalExitRequest::new(
                "terminal-session",
                terminal.clone(),
            ))
            .block_task();
        let interrupt = async {
            let output = connection
                .send_request(TerminalOutputRequest::new(
                    "terminal-session",
                    terminal.clone(),
                ))
                .block_task()
                .await
                .unwrap();
            assert!(
                output.exit_status.is_none(),
                "the terminal must still be running before its kill"
            );
            connection
                .send_request(KillTerminalRequest::new(
                    "terminal-session",
                    terminal.clone(),
                ))
                .block_task()
                .await
                .unwrap();
        };
        // Poll the wait first, placing it before output/kill on the ACP wire.
        let (exited, ()) = tokio::join!(biased; waiting, interrupt);
        let exited = exited.unwrap();
        assert_ne!(exited.exit_status.exit_code, Some(0));
        connection
            .send_request(ReleaseTerminalRequest::new(
                "terminal-session",
                terminal.clone(),
            ))
            .block_task()
            .await
            .unwrap();
    })
    .await;
    // Cleanup also works on the broken control: it does not use the blocked ACP actor.
    provider.stop_background(terminal.0.as_ref());
    drop(provider);
    agent.abort();
    result.expect("terminal wait deadlocked output/kill delivery");
}
