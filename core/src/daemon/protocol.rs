use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ControlCommand {
	AllowUi,
	DisallowUi,
	Poweroff,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "kebab-case")]
pub(super) enum ControlReply {
	Ok { message: String },
	NotRunning { message: String },
	Error { message: String },
}
