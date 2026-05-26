use rl_stats_core::daemon::{ControlCommand, execute_control_command, run_daemon};
use rl_stats_core::{AppConfig, ConfigManager, default_config_path};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProcessMode {
    Daemon,
    Command(ControlCommand),
}

fn main() {
    let mode = parse_mode(std::env::args().skip(1));

    if let ProcessMode::Command(command) = mode {
        execute_control_command(command);
        return;
    }

    let config_manager = ConfigManager::new(default_config_path());

    let config = match config_manager.load_or_create() {
        Ok(config) => config,
        Err(err) => {
            let config_path = config_manager.path().display();
            eprintln!("Failed to load config at {config_path}: {err}. Falling back to defaults.");
            AppConfig::default()
        }
    };

    run_daemon(config);
}

fn parse_mode(args: impl Iterator<Item = String>) -> ProcessMode {
    let mut mode = ProcessMode::Daemon;

    for arg in args {
        match arg.as_str() {
            "--allow-ui" => mode = ProcessMode::Command(ControlCommand::AllowUi),
            "--disallow-ui" => mode = ProcessMode::Command(ControlCommand::DisallowUi),
            "--poweroff" => mode = ProcessMode::Command(ControlCommand::Poweroff),
            _ => {}
        }
    }

    mode
}
