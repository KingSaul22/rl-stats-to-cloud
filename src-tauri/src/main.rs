// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use rl_stats_core::{
    connector_factory, default_config_path, AppConfig, AppState, ConfigManager,
    RocketLeagueWorker,
};
use tokio_util::sync::CancellationToken;

fn main() {
    let cli_headless = std::env::args().any(|arg| arg == "--headless");
    let config_manager = ConfigManager::new(default_config_path());

    let mut config = match config_manager.load_or_create() {
        Ok(config) => config,
        Err(err) => {
            eprintln!(
                "Failed to load config at {}: {}. Falling back to defaults.",
                config_manager.path().display(),
                err
            );
            AppConfig::default()
        }
    };

    if cli_headless {
        config.is_headless = true;
    }

    if config.is_headless {
        run_headless(config);
    } else if let Err(err) = rl_stats_to_cloud_lib::run_tauri(config) {
        eprintln!("Tauri app terminated with error: {err}");
    }
}

fn run_headless(config: AppConfig) {
    println!("Starting in headless mode...");

    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(err) => {
            eprintln!("Failed to initialize tokio runtime: {err}");
            return;
        }
    };

    runtime.block_on(async move {
        let (state_sender, _state_receiver) =
            tokio::sync::watch::channel(AppState::default());
        let initial_sink = connector_factory(&config.connector);
        let (_sink_sender, sink_receiver) = tokio::sync::watch::channel(initial_sink);
        let worker = RocketLeagueWorker::from_config(&config, state_sender, sink_receiver);
        let shutdown = CancellationToken::new();
        let worker_shutdown = shutdown.clone();

        let worker_task = tokio::spawn(async move {
            worker.run_until_cancelled(worker_shutdown).await;
        });

        match tokio::signal::ctrl_c().await {
            Ok(()) => {
                println!("Ctrl+C received. Signaling shutdown...");
                shutdown.cancel();
            }
            Err(err) => {
                eprintln!("Failed to listen for Ctrl+C: {err}");
                shutdown.cancel();
            }
        }

        if let Err(err) = worker_task.await {
            eprintln!("Worker task join error: {err}");
        }
    });
}
