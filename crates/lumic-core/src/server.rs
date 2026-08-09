use crate::{LumicError, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserAccount {
    pub name: String,
    pub uid: u32,
    pub gid: u32,
    pub home: String,
    pub shell: String,
    pub system: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupAccount {
    pub name: String,
    pub gid: u32,
    pub members: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FirewallDecision {
    Allow,
    Deny,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NetworkProtocol {
    Tcp,
    Udp,
}

impl NetworkProtocol {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Tcp => "tcp",
            Self::Udp => "udp",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FirewallRule {
    pub decision: FirewallDecision,
    pub port: u16,
    pub protocol: NetworkProtocol,
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListeningPort {
    pub protocol: String,
    pub local_address: String,
    pub port: u16,
    pub process: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MountStatus {
    pub source: String,
    pub mount_point: String,
    pub filesystem: String,
    pub options: Vec<String>,
    pub total_bytes: u64,
    pub available_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimerStatus {
    pub unit: String,
    pub next: String,
    pub last: String,
    pub activates: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateStatus {
    pub package: String,
    pub current_version: String,
    pub candidate_version: String,
    pub security: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackupSchedule {
    pub id: String,
    pub service_id: String,
    pub database: Option<String>,
    pub on_calendar: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProcessSignal {
    Terminate,
    Kill,
    Hangup,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UpdateScope {
    Security,
    All,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum RemediationAction {
    RestartService { unit: String },
    TerminateProcess { pid: u32 },
    VacuumJournal { older_than_days: u16 },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MutationResult {
    pub changed: bool,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostOperatorSnapshot {
    pub users: Vec<UserAccount>,
    pub groups: Vec<GroupAccount>,
    pub firewall: Vec<String>,
    pub listeners: Vec<ListeningPort>,
    pub mounts: Vec<MountStatus>,
    pub processes: Vec<crate::ProcessFacts>,
    pub timers: Vec<TimerStatus>,
    pub updates: Vec<UpdateStatus>,
    pub backup_schedules: Vec<BackupSchedule>,
}

pub fn validate_account_name(field: &str, value: &str) -> Result<()> {
    if !value.is_empty()
        && value.len() <= 32
        && value.as_bytes()[0].is_ascii_lowercase()
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'_' | b'-')
        })
    {
        Ok(())
    } else {
        Err(LumicError::InvalidInput {
            field: field.into(),
            message: "must be a lowercase Linux account name".into(),
        })
    }
}

pub fn validate_calendar(value: &str) -> Result<()> {
    if !value.is_empty() && value.len() <= 128 && !value.contains(['\n', '\r', '\0']) {
        Ok(())
    } else {
        Err(LumicError::InvalidInput {
            field: "calendar".into(),
            message: "must be a bounded systemd OnCalendar expression".into(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn account_and_calendar_inputs_are_data_only() {
        assert!(validate_account_name("user", "deploy-1").is_ok());
        assert!(validate_account_name("user", "root;id").is_err());
        assert!(validate_calendar("daily\nExecStart=/bin/sh").is_err());
    }
}
