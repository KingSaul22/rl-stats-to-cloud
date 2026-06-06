use super::transport::{ControlCommand, ControlReply, run_state_bridge_loop, send_control_command};
use super::{BridgeTaskHandle, SharedBridgeTask};
use rl_stats_core::StateSender;
use tokio_util::sync::CancellationToken;

pub(super) fn spawn_ui_bridge_task(
    app_handle: tauri::AppHandle,
    ui_sync_port: u16,
    shutdown: CancellationToken,
    state_sender: StateSender,
) -> BridgeTaskHandle {
    let ws_url = format!("ws://127.0.0.1:{ui_sync_port}");
    tauri::async_runtime::spawn(async move {
        let allow_reply = send_control_command(ControlCommand::AllowUi).await;
        match allow_reply {
            ControlReply::Ok { message } => {
                println!("AllowUi command acknowledged from UI bridge: {message}");
            }
            ControlReply::NotRunning { message } => {
                eprintln!("AllowUi failed (daemon not running): {message}");
            }
            ControlReply::Error { message } => {
                eprintln!("AllowUi command error: {message}");
            }
        }

        run_state_bridge_loop(ws_url, app_handle, state_sender, shutdown).await;
    })
}

pub(super) fn shutdown_ui_bridge_and_disallow(
    shutdown: &CancellationToken,
    bridge_task: &SharedBridgeTask,
    phase: &str,
) {
    shutdown.cancel();

    if let Ok(mut guard) = bridge_task.lock()
        && let Some(handle) = guard.take()
    {
        let abort_handle = handle.inner().abort_handle();
        let join_result = tauri::async_runtime::block_on(async {
            tokio::time::timeout(
                tokio::time::Duration::from_secs(2),
                handle,
            )
            .await
        });

        match join_result {
            Ok(Ok(())) => {}
            Ok(Err(err)) => {
                eprintln!("Bridge task join error on UI {phase}: {err}");
            }
            Err(_) => {
                abort_handle.abort();
                eprintln!(
                    "WARNING: Bridge task did not shut down within 2s on UI {phase}. Forcibly aborted."
                );
            }
        }
    }

    let disallow_reply = tauri::async_runtime::block_on(async {
        send_control_command(ControlCommand::DisallowUi).await
    });
    match disallow_reply {
        ControlReply::Ok { message } => {
            println!("DisallowUi command acknowledged on UI {phase}: {message}");
        }
        ControlReply::NotRunning { message } => {
            eprintln!("DisallowUi on {phase}: daemon not running: {message}");
        }
        ControlReply::Error { message } => {
            eprintln!("DisallowUi on {phase} error: {message}");
        }
    }
}
