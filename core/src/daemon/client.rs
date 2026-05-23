use super::protocol::{ControlCommand, ControlReply};
use super::{control_endpoint_display, CONTROL_BIND_ADDR};
use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream as StdTcpStream;

pub fn execute_control_command(command: ControlCommand) {
	let reply = send_control_command(command);
	print_control_reply(command, &reply);
}

pub(super) fn send_control_command(command: ControlCommand) -> ControlReply {
	let endpoint_display = control_endpoint_display();
	let mut stream = match StdTcpStream::connect(CONTROL_BIND_ADDR) {
		Ok(stream) => stream,
		Err(err) => {
			return ControlReply::NotRunning {
				message: format!("Failed to connect to daemon at {endpoint_display}: {err}"),
			};
		}
	};

	let payload = match serde_json::to_string(&command) {
		Ok(payload) => payload,
		Err(err) => {
			return ControlReply::Error {
				message: format!("Failed to serialize control command: {err}"),
			};
		}
	};

	if let Err(err) = stream.write_all(payload.as_bytes()) {
		return ControlReply::Error {
			message: format!("Failed to send control command payload: {err}"),
		};
	}
	if let Err(err) = stream.write_all(b"\n") {
		return ControlReply::Error {
			message: format!("Failed to send control command frame delimiter: {err}"),
		};
	}
	if let Err(err) = stream.flush() {
		return ControlReply::Error {
			message: format!("Failed to flush control command stream: {err}"),
		};
	}

	let mut reader = BufReader::new(stream);
	let mut response_line = String::new();
	match reader.read_line(&mut response_line) {
		Ok(0) => ControlReply::Error {
			message: "Daemon closed the control socket without sending a reply.".to_string(),
		},
		Ok(_) => {
			let frame = response_line.trim_end();
			match serde_json::from_str::<ControlReply>(frame) {
				Ok(reply) => reply,
				Err(err) => ControlReply::Error {
					message: format!("Failed to decode daemon reply '{frame}': {err}"),
				},
			}
		}
		Err(err) => ControlReply::Error {
			message: format!("Failed to read control reply from daemon: {err}"),
		},
	}
}

pub(super) fn print_control_reply(command: ControlCommand, reply: &ControlReply) {
	let command_name = match command {
		ControlCommand::AllowUi => "allow-ui",
		ControlCommand::DisallowUi => "disallow-ui",
		ControlCommand::Poweroff => "poweroff",
	};

	match reply {
		ControlReply::Ok { message } => {
			println!("Command '{command_name}' acknowledged: {message}");
		}
		ControlReply::NotRunning { message } => {
			eprintln!("Command '{command_name}' failed: {message}");
		}
		ControlReply::Error { message } => {
			eprintln!("Command '{command_name}' error: {message}");
		}
	}
}
