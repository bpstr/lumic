use crate::{
    ProcessRunner, ProcessSpec,
    atomic_file::write_atomic,
    audit_store::AuditStore,
    event_store::EventStore,
    systemd::{ServiceAction, SystemdServiceManager},
};
use lumic_core::{
    LumicError, OperationContext, ProcessFacts, Result,
    events::{AuditRecord, Event},
    server::{
        BackupSchedule, FirewallDecision, FirewallRule, GroupAccount, HostOperatorSnapshot,
        ListeningPort, MountStatus, MutationResult, ProcessSignal, RemediationAction, TimerStatus,
        UpdateScope, UpdateStatus, UserAccount, validate_account_name, validate_calendar,
    },
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    cmp::Reverse,
    ffi::CString,
    fs,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct ScheduleState {
    version: u32,
    schedules: Vec<BackupSchedule>,
}

#[derive(Debug, Clone)]
pub struct HostOperator {
    state_dir: PathBuf,
    systemd_dir: PathBuf,
    runner: ProcessRunner,
}

impl HostOperator {
    pub fn at_state_dir(state_dir: impl AsRef<Path>) -> Self {
        let state_dir = state_dir.as_ref().to_path_buf();
        let systemd_dir = std::env::var_os("LUMIC_SYSTEMD_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| "/etc/systemd/system".into());
        Self {
            state_dir,
            systemd_dir,
            runner: ProcessRunner,
        }
    }

    pub async fn snapshot(&self) -> Result<HostOperatorSnapshot> {
        Ok(HostOperatorSnapshot {
            users: self.users()?,
            groups: self.groups()?,
            firewall: self.firewall_status().await.unwrap_or_default(),
            listeners: self.listeners().await.unwrap_or_default(),
            mounts: self.mounts()?,
            processes: self.processes(50)?,
            timers: self.timers().await.unwrap_or_default(),
            updates: self.updates().await.unwrap_or_default(),
            backup_schedules: self.backup_schedules()?,
        })
    }

    pub fn users(&self) -> Result<Vec<UserAccount>> {
        parse_passwd(&fs::read_to_string("/etc/passwd").map_err(|error| inspect("users", error))?)
    }
    pub fn groups(&self) -> Result<Vec<GroupAccount>> {
        parse_groups(&fs::read_to_string("/etc/group").map_err(|error| inspect("groups", error))?)
    }

    pub async fn create_user(
        &self,
        name: &str,
        context: &OperationContext,
    ) -> Result<MutationResult> {
        validate_account_name("user", name)?;
        if self.users()?.iter().any(|user| user.name == name) {
            return Ok(unchanged("user already exists"));
        }
        self.run("useradd", ["--create-home", "--shell", "/bin/bash", name])
            .await?;
        self.record(
            "server.user.created",
            "user.create",
            "user",
            name,
            json!({}),
            context,
        )?;
        Ok(changed(format!("created user {name}")))
    }

    pub async fn delete_user(
        &self,
        name: &str,
        context: &OperationContext,
    ) -> Result<MutationResult> {
        validate_account_name("user", name)?;
        if name == "root" {
            return Err(invalid("user", "root cannot be removed"));
        }
        if self.users()?.iter().all(|user| user.name != name) {
            return Ok(unchanged("user is already absent"));
        }
        self.run("userdel", [name]).await?;
        self.record(
            "server.user.deleted",
            "user.delete",
            "user",
            name,
            json!({"home_preserved":true}),
            context,
        )?;
        Ok(changed(format!(
            "deleted user {name}; home directory was preserved"
        )))
    }

    pub async fn create_group(
        &self,
        name: &str,
        context: &OperationContext,
    ) -> Result<MutationResult> {
        validate_account_name("group", name)?;
        if self.groups()?.iter().any(|group| group.name == name) {
            return Ok(unchanged("group already exists"));
        }
        self.run("groupadd", [name]).await?;
        self.record(
            "server.group.created",
            "group.create",
            "group",
            name,
            json!({}),
            context,
        )?;
        Ok(changed(format!("created group {name}")))
    }

    pub async fn delete_group(
        &self,
        name: &str,
        context: &OperationContext,
    ) -> Result<MutationResult> {
        validate_account_name("group", name)?;
        if name == "root" {
            return Err(invalid("group", "root cannot be removed"));
        }
        if self.groups()?.iter().all(|group| group.name != name) {
            return Ok(unchanged("group is already absent"));
        }
        self.run("groupdel", [name]).await?;
        self.record(
            "server.group.deleted",
            "group.delete",
            "group",
            name,
            json!({}),
            context,
        )?;
        Ok(changed(format!("deleted group {name}")))
    }

    pub async fn add_group_member(
        &self,
        group: &str,
        user: &str,
        context: &OperationContext,
    ) -> Result<MutationResult> {
        validate_account_name("group", group)?;
        validate_account_name("user", user)?;
        if self
            .groups()?
            .iter()
            .find(|item| item.name == group)
            .is_some_and(|item| item.members.iter().any(|item| item == user))
        {
            return Ok(unchanged("membership already exists"));
        }
        self.run("usermod", ["--append", "--groups", group, user])
            .await?;
        self.record(
            "server.group.member_added",
            "group.add_member",
            "group",
            group,
            json!({"user":user}),
            context,
        )?;
        Ok(changed(format!("added {user} to {group}")))
    }

    pub async fn set_permissions(
        &self,
        path: &Path,
        owner: &str,
        group: &str,
        mode: u32,
        context: &OperationContext,
    ) -> Result<MutationResult> {
        validate_account_name("owner", owner)?;
        validate_account_name("group", group)?;
        validate_managed_path(path)?;
        let metadata = fs::symlink_metadata(path).map_err(|error| inspect("permissions", error))?;
        if metadata.file_type().is_symlink() {
            return Err(invalid(
                "path",
                "symbolic links are not accepted for permission changes",
            ));
        }
        if fs::canonicalize(path).map_err(|error| inspect("permissions", error))? != path {
            return Err(invalid(
                "path",
                "paths containing symbolic-link components are not accepted",
            ));
        }
        if mode > 0o7777 {
            return Err(invalid(
                "mode",
                "must be an octal permission mode up to 07777",
            ));
        }
        let mode_text = format!("{mode:o}");
        let path_text = path.to_string_lossy().into_owned();
        self.run("chown", [format!("{owner}:{group}"), path_text.clone()])
            .await?;
        self.run("chmod", [mode_text.clone(), path_text.clone()])
            .await?;
        self.record(
            "server.permissions.changed",
            "permissions.set",
            "path",
            &path_text,
            json!({"owner":owner,"group":group,"mode":mode_text}),
            context,
        )?;
        Ok(changed(format!("updated permissions for {path_text}")))
    }

    pub async fn firewall_status(&self) -> Result<Vec<String>> {
        let output = self.run_output("ufw", ["status", "numbered"]).await?;
        Ok(output
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(str::to_owned)
            .collect())
    }

    pub async fn apply_firewall_rule(
        &self,
        rule: &FirewallRule,
        remove: bool,
        context: &OperationContext,
    ) -> Result<MutationResult> {
        validate_firewall_rule(rule)?;
        let decision = match rule.decision {
            FirewallDecision::Allow => "allow",
            FirewallDecision::Deny => "deny",
        };
        let port = rule.port.to_string();
        let mut args = Vec::new();
        if remove {
            args.push("delete".into());
        }
        args.extend([
            decision.into(),
            "from".into(),
            rule.source.clone(),
            "to".into(),
            "any".into(),
            "port".into(),
            port,
            "proto".into(),
            rule.protocol.as_str().into(),
        ]);
        self.run("ufw", args).await?;
        let verb = if remove { "removed" } else { "applied" };
        self.record(
            "server.firewall.changed",
            if remove {
                "firewall.remove"
            } else {
                "firewall.apply"
            },
            "firewall_rule",
            &format!("{}/{}", rule.port, rule.protocol.as_str()),
            json!({"rule":rule,"remove":remove}),
            context,
        )?;
        Ok(changed(format!(
            "{verb} firewall rule for {}/{}",
            rule.port,
            rule.protocol.as_str()
        )))
    }

    pub async fn listeners(&self) -> Result<Vec<ListeningPort>> {
        parse_listeners(
            &self
                .run_output(
                    "ss",
                    [
                        "--listening",
                        "--numeric",
                        "--tcp",
                        "--udp",
                        "--processes",
                        "--no-header",
                    ],
                )
                .await?,
        )
    }

    pub fn mounts(&self) -> Result<Vec<MountStatus>> {
        parse_mounts(&fs::read_to_string("/proc/mounts").map_err(|error| inspect("mounts", error))?)
    }

    pub fn processes(&self, limit: usize) -> Result<Vec<ProcessFacts>> {
        let mut processes = Vec::new();
        for entry in fs::read_dir("/proc").map_err(|error| inspect("processes", error))? {
            let entry = entry.map_err(|error| inspect("processes", error))?;
            let Some(pid) = entry
                .file_name()
                .to_str()
                .and_then(|name| name.parse::<u32>().ok())
            else {
                continue;
            };
            let Ok(status) = fs::read_to_string(entry.path().join("status")) else {
                continue;
            };
            if let Some(process) = parse_process(pid, &status) {
                processes.push(process)
            }
        }
        processes.sort_by_key(|process| Reverse(process.resident_bytes));
        processes.truncate(limit.min(1000));
        Ok(processes)
    }

    pub fn signal_process(
        &self,
        pid: u32,
        signal: ProcessSignal,
        context: &OperationContext,
    ) -> Result<MutationResult> {
        if pid <= 1 || pid == std::process::id() {
            return Err(invalid(
                "pid",
                "PID 0, 1, and the Lumic process cannot be controlled",
            ));
        }
        let native = match signal {
            ProcessSignal::Terminate => libc::SIGTERM,
            ProcessSignal::Kill => libc::SIGKILL,
            ProcessSignal::Hangup => libc::SIGHUP,
        };
        // SAFETY: kill is called with a validated positive PID and a fixed signal constant.
        if unsafe { libc::kill(pid as i32, native) } != 0 {
            return Err(LumicError::Process {
                executable: "kill(2)".into(),
                message: std::io::Error::last_os_error().to_string(),
            });
        }
        self.record(
            "server.process.signalled",
            "process.signal",
            "process",
            &pid.to_string(),
            json!({"signal":signal}),
            context,
        )?;
        Ok(changed(format!("sent {signal:?} to PID {pid}")))
    }

    pub async fn timers(&self) -> Result<Vec<TimerStatus>> {
        parse_timers(
            &self
                .run_output(
                    "systemctl",
                    ["list-timers", "--all", "--no-legend", "--no-pager"],
                )
                .await?,
        )
    }

    pub async fn updates(&self) -> Result<Vec<UpdateStatus>> {
        let output = self.run_output("apt", ["list", "--upgradable"]).await?;
        Ok(parse_updates(&output))
    }

    pub async fn apply_updates(
        &self,
        scope: UpdateScope,
        context: &OperationContext,
    ) -> Result<MutationResult> {
        let before = self.updates().await?;
        if before.is_empty() {
            return Ok(unchanged("no package updates are pending"));
        }
        match scope {
            UpdateScope::Security => self.run("unattended-upgrade", ["--verbose"]).await?,
            UpdateScope::All => self.run("apt-get", ["--yes", "upgrade"]).await?,
        }
        let after = self.updates().await.unwrap_or_default();
        self.record(
            "server.updates.applied",
            "updates.apply",
            "host",
            "local",
            json!({"scope":scope,"pending_before":before.len(),"pending_after":after.len()}),
            context,
        )?;
        Ok(changed(format!(
            "applied {scope:?} updates; {} packages remain pending",
            after.len()
        )))
    }

    pub async fn search_journal(
        &self,
        unit: Option<&str>,
        priority: Option<&str>,
        since: Option<&str>,
        query: Option<&str>,
        lines: usize,
    ) -> Result<String> {
        let mut args = vec![
            "--no-pager".into(),
            "--output=short-iso".into(),
            "--lines".into(),
            lines.min(5000).to_string(),
        ];
        if let Some(unit) = unit {
            validate_unit(unit)?;
            args.extend(["--unit".into(), unit.into()]);
        }
        if let Some(priority) = priority {
            if !matches!(
                priority,
                "emerg" | "alert" | "crit" | "err" | "warning" | "notice" | "info" | "debug"
            ) {
                return Err(invalid("priority", "unknown journal priority"));
            }
            args.extend(["--priority".into(), priority.into()]);
        }
        if let Some(since) = since {
            validate_text("since", since, 128)?;
            args.extend(["--since".into(), since.into()]);
        }
        if let Some(query) = query {
            validate_text("query", query, 256)?;
            args.extend(["--grep".into(), query.into()]);
        }
        self.run_output("journalctl", args).await
    }

    pub fn backup_schedules(&self) -> Result<Vec<BackupSchedule>> {
        Ok(self.load_schedules()?.schedules)
    }

    pub async fn schedule_backup(
        &self,
        schedule: BackupSchedule,
        context: &OperationContext,
    ) -> Result<MutationResult> {
        lumic_core::managed_service::validate_resource_id("schedule", &schedule.id)?;
        lumic_core::managed_service::validate_resource_id("service", &schedule.service_id)?;
        if let Some(database) = &schedule.database {
            lumic_core::managed_service::validate_database_identifier("database", database)?;
        }
        validate_calendar(&schedule.on_calendar)?;
        let service_unit = format!("lumic-backup-{}.service", schedule.id);
        let timer_unit = format!("lumic-backup-{}.timer", schedule.id);
        let database = schedule
            .database
            .as_ref()
            .map(|value| format!(" --database {}", systemd_quote(value)))
            .unwrap_or_default();
        let service = format!(
            "[Unit]\nDescription=Lumic backup {}\n\n[Service]\nType=oneshot\nExecStart=/usr/local/bin/lumic managed-service backup {}{}\n",
            schedule.id,
            systemd_quote(&schedule.service_id),
            database
        );
        let timer = format!(
            "[Unit]\nDescription=Lumic backup schedule {}\n\n[Timer]\nOnCalendar={}\nPersistent=true\nRandomizedDelaySec=5m\n\n[Install]\nWantedBy=timers.target\n",
            schedule.id, schedule.on_calendar
        );
        write_atomic(
            &self.systemd_dir.join(&service_unit),
            service.as_bytes(),
            0o644,
        )?;
        write_atomic(&self.systemd_dir.join(&timer_unit), timer.as_bytes(), 0o644)?;
        self.run("systemctl", ["daemon-reload"]).await?;
        if schedule.enabled {
            self.run("systemctl", ["enable", "--now", &timer_unit])
                .await?;
        } else {
            self.run("systemctl", ["disable", "--now", &timer_unit])
                .await?;
        }
        let mut state = self.load_schedules()?;
        if let Some(existing) = state
            .schedules
            .iter_mut()
            .find(|item| item.id == schedule.id)
        {
            *existing = schedule.clone()
        } else {
            state.schedules.push(schedule.clone())
        }
        self.save_schedules(&state)?;
        self.record(
            "server.backup_schedule.configured",
            "backup.schedule",
            "backup_schedule",
            &schedule.id,
            json!({"schedule":schedule}),
            context,
        )?;
        Ok(changed(format!("configured {timer_unit}")))
    }

    pub async fn remediate(
        &self,
        action: RemediationAction,
        context: &OperationContext,
    ) -> Result<MutationResult> {
        let result = match &action {
            RemediationAction::RestartService { unit } => {
                validate_unit(unit)?;
                SystemdServiceManager::at_state_dir(&self.state_dir)
                    .apply(unit, ServiceAction::Restart, context)
                    .await?;
                changed(format!("restarted {unit} and verified systemd state"))
            }
            RemediationAction::TerminateProcess { pid } => {
                return self.signal_process(*pid, ProcessSignal::Terminate, context);
            }
            RemediationAction::VacuumJournal { older_than_days } => {
                if *older_than_days < 1 || *older_than_days > 3650 {
                    return Err(invalid("older_than_days", "must be between 1 and 3650"));
                }
                self.run(
                    "journalctl",
                    ["--vacuum-time", &format!("{}d", older_than_days)],
                )
                .await?;
                changed(format!(
                    "vacuumed journal entries older than {older_than_days} days"
                ))
            }
        };
        self.record(
            "server.remediation.applied",
            "remediation.apply",
            "host",
            "local",
            json!({"action":action}),
            context,
        )?;
        Ok(result)
    }

    async fn run_output(
        &self,
        executable: &str,
        args: impl IntoIterator<Item = impl Into<String>>,
    ) -> Result<String> {
        let spec = ProcessSpec::new(executable).args(args);
        let output = self.runner.run(&spec).await?;
        if !output.success() {
            return Err(LumicError::Process {
                executable: executable.into(),
                message: String::from_utf8_lossy(&output.stderr).trim().into(),
            });
        }
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }
    async fn run(
        &self,
        executable: &str,
        args: impl IntoIterator<Item = impl Into<String>>,
    ) -> Result<()> {
        self.run_output(executable, args).await.map(|_| ())
    }
    fn schedule_path(&self) -> PathBuf {
        self.state_dir.join("backup-schedules.json")
    }
    fn load_schedules(&self) -> Result<ScheduleState> {
        let path = self.schedule_path();
        if !path.exists() {
            return Ok(ScheduleState {
                version: 1,
                ..Default::default()
            });
        }
        serde_json::from_slice(&fs::read(path).map_err(io)?).map_err(|error| LumicError::Internal {
            message: format!("backup schedule state is invalid: {error}"),
        })
    }
    fn save_schedules(&self, state: &ScheduleState) -> Result<()> {
        let bytes = serde_json::to_vec_pretty(state).map_err(|error| LumicError::Internal {
            message: format!("could not serialize schedules: {error}"),
        })?;
        write_atomic(&self.schedule_path(), &bytes, 0o600).map(|_| ())
    }
    fn record(
        &self,
        event_type: &str,
        capability: &str,
        entity: &str,
        id: &str,
        args: serde_json::Value,
        context: &OperationContext,
    ) -> Result<()> {
        EventStore::at_state_dir(&self.state_dir).append(&Event::now(
            event_type,
            &context.actor,
            context.interface,
            entity,
            id,
            &context.correlation_id,
            args.clone(),
        ))?;
        AuditStore::at_state_dir(&self.state_dir).append(&AuditRecord::now(
            context,
            capability,
            event_type,
            entity,
            id,
            args,
            None,
            None,
            true,
            "typed host operation applied",
        ))
    }
}

fn parse_passwd(input: &str) -> Result<Vec<UserAccount>> {
    input
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let parts = line.split(':').collect::<Vec<_>>();
            if parts.len() < 7 {
                return Err(inspect("users", "invalid passwd entry"));
            }
            let uid = parts[2]
                .parse()
                .map_err(|_| inspect("users", "invalid UID"))?;
            let gid = parts[3]
                .parse()
                .map_err(|_| inspect("users", "invalid GID"))?;
            Ok(UserAccount {
                name: parts[0].into(),
                uid,
                gid,
                home: parts[5].into(),
                shell: parts[6].into(),
                system: uid < 1000,
            })
        })
        .collect()
}
fn parse_groups(input: &str) -> Result<Vec<GroupAccount>> {
    input
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let parts = line.split(':').collect::<Vec<_>>();
            if parts.len() < 4 {
                return Err(inspect("groups", "invalid group entry"));
            }
            Ok(GroupAccount {
                name: parts[0].into(),
                gid: parts[2]
                    .parse()
                    .map_err(|_| inspect("groups", "invalid GID"))?,
                members: parts[3]
                    .split(',')
                    .filter(|item| !item.is_empty())
                    .map(str::to_owned)
                    .collect(),
            })
        })
        .collect()
}
fn parse_listeners(input: &str) -> Result<Vec<ListeningPort>> {
    let mut result = Vec::new();
    for line in input.lines() {
        let parts = line.split_whitespace().collect::<Vec<_>>();
        if parts.len() < 5 {
            continue;
        }
        let protocol = parts[0].into();
        let local = parts.get(4).or_else(|| parts.get(3)).copied().unwrap_or("");
        let Some((address, port)) = split_socket(local) else {
            continue;
        };
        result.push(ListeningPort {
            protocol,
            local_address: address,
            port,
            process: parts
                .iter()
                .find(|value| value.contains("users:("))
                .map(|value| (*value).into()),
        })
    }
    Ok(result)
}
fn split_socket(value: &str) -> Option<(String, u16)> {
    let index = value.rfind(':')?;
    Some((
        value[..index].trim_matches(['[', ']']).into(),
        value[index + 1..].parse().ok()?,
    ))
}
fn parse_mounts(input: &str) -> Result<Vec<MountStatus>> {
    let mut result = Vec::new();
    for line in input.lines() {
        let parts = line.split_whitespace().collect::<Vec<_>>();
        if parts.len() < 4 {
            continue;
        }
        let (total, available) = filesystem_capacity(parts[1]).unwrap_or((0, 0));
        result.push(MountStatus {
            source: unescape_mount(parts[0]),
            mount_point: unescape_mount(parts[1]),
            filesystem: parts[2].into(),
            options: parts[3].split(',').map(str::to_owned).collect(),
            total_bytes: total,
            available_bytes: available,
        })
    }
    Ok(result)
}
fn filesystem_capacity(path: &str) -> Option<(u64, u64)> {
    let path = CString::new(path).ok()?;
    let mut stats = std::mem::MaybeUninit::<libc::statvfs>::uninit(); /* SAFETY: path is NUL terminated and stats points to writable memory. */
    if unsafe { libc::statvfs(path.as_ptr(), stats.as_mut_ptr()) } != 0 {
        return None;
    } /* SAFETY: statvfs initialized stats on success. */
    let stats = unsafe { stats.assume_init() };
    Some((
        u64::from(stats.f_blocks).saturating_mul(stats.f_frsize),
        u64::from(stats.f_bavail).saturating_mul(stats.f_frsize),
    ))
}
fn unescape_mount(value: &str) -> String {
    value
        .replace("\\040", " ")
        .replace("\\011", "\t")
        .replace("\\134", "\\")
}
fn parse_process(pid: u32, input: &str) -> Option<ProcessFacts> {
    let mut name = String::new();
    let mut state = String::new();
    let mut resident = 0;
    for line in input.lines() {
        if let Some(value) = line.strip_prefix("Name:\t") {
            name = value.into()
        }
        if let Some(value) = line.strip_prefix("State:\t") {
            state = value.into()
        }
        if let Some(value) = line.strip_prefix("VmRSS:\t") {
            resident = value
                .split_whitespace()
                .next()?
                .parse::<u64>()
                .ok()?
                .saturating_mul(1024)
        }
    }
    Some(ProcessFacts {
        pid,
        name,
        state,
        resident_bytes: resident,
    })
}
fn parse_timers(input: &str) -> Result<Vec<TimerStatus>> {
    Ok(input
        .lines()
        .filter_map(|line| {
            let parts = line.split_whitespace().collect::<Vec<_>>();
            let timer_index = parts.iter().position(|item| item.ends_with(".timer"))?;
            Some(TimerStatus {
                next: parts[..timer_index.saturating_sub(2)].join(" "),
                last: String::new(),
                unit: (*parts.get(timer_index)?).into(),
                activates: parts.get(timer_index + 1).copied().unwrap_or("").into(),
            })
        })
        .collect())
}
fn parse_updates(input: &str) -> Vec<UpdateStatus> {
    input
        .lines()
        .filter(|line| line.contains("[upgradable from:"))
        .filter_map(|line| {
            let (head, tail) = line.split_once(' ')?;
            let package = head.split('/').next()?.into();
            let candidate = tail.split_whitespace().next()?.into();
            let current = tail
                .split("[upgradable from:")
                .nth(1)?
                .trim_end_matches(']')
                .trim()
                .into();
            Some(UpdateStatus {
                package,
                current_version: current,
                candidate_version: candidate,
                security: line.to_ascii_lowercase().contains("security"),
            })
        })
        .collect()
}
fn validate_firewall_rule(rule: &FirewallRule) -> Result<()> {
    if rule.port == 0 {
        return Err(invalid("port", "must be between 1 and 65535"));
    }
    if rule.source != "any" && !valid_ip_source(&rule.source) {
        return Err(invalid("source", "must be 'any' or an IP address/CIDR"));
    }
    Ok(())
}
fn valid_ip_source(source: &str) -> bool {
    if source.parse::<std::net::IpAddr>().is_ok() {
        return true;
    }
    let Some((address, prefix)) = source.split_once('/') else {
        return false;
    };
    let Ok(address) = address.parse::<std::net::IpAddr>() else {
        return false;
    };
    let Ok(prefix) = prefix.parse::<u8>() else {
        return false;
    };
    prefix <= if address.is_ipv4() { 32 } else { 128 }
}
fn validate_unit(value: &str) -> Result<()> {
    if !value.is_empty()
        && value.len() <= 255
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b'@'))
    {
        Ok(())
    } else {
        Err(invalid("unit", "must be a safe systemd unit name"))
    }
}
fn validate_text(field: &str, value: &str, max: usize) -> Result<()> {
    if !value.is_empty() && value.len() <= max && !value.contains(['\n', '\r', '\0']) {
        Ok(())
    } else {
        Err(invalid(field, "must be bounded single-line text"))
    }
}
fn validate_managed_path(path: &Path) -> Result<()> {
    if path.is_absolute()
        && !path.as_os_str().is_empty()
        && path != Path::new("/")
        && !path
            .components()
            .any(|part| matches!(part, std::path::Component::ParentDir))
    {
        Ok(())
    } else {
        Err(invalid(
            "path",
            "must be an absolute non-root path without traversal",
        ))
    }
}
fn systemd_quote(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}
fn changed(message: impl Into<String>) -> MutationResult {
    MutationResult {
        changed: true,
        message: message.into(),
    }
}
fn unchanged(message: impl Into<String>) -> MutationResult {
    MutationResult {
        changed: false,
        message: message.into(),
    }
}
fn invalid(field: &str, message: &str) -> LumicError {
    LumicError::InvalidInput {
        field: field.into(),
        message: message.into(),
    }
}
fn inspect(fact: &str, error: impl std::fmt::Display) -> LumicError {
    LumicError::Inspection {
        fact: fact.into(),
        message: error.to_string(),
    }
}
fn io(error: std::io::Error) -> LumicError {
    LumicError::Internal {
        message: format!("server state I/O failed: {error}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_native_account_update_and_listener_evidence() {
        let users = parse_passwd(
            "root:x:0:0:root:/root:/bin/bash\ndeploy:x:1000:1000::/home/deploy:/bin/bash\n",
        )
        .unwrap();
        assert_eq!(users[1].name, "deploy");
        let updates = parse_updates("nginx/stable 1.2 amd64 [upgradable from: 1.1]");
        assert_eq!(updates[0].candidate_version, "1.2");
        let listeners = parse_listeners("tcp LISTEN 0 128 127.0.0.1:8080 0.0.0.0:*\n").unwrap();
        assert_eq!(listeners[0].port, 8080)
    }
    #[test]
    fn firewall_and_paths_reject_unsafe_inputs() {
        assert!(validate_unit("nginx.service;id").is_err());
        assert!(validate_managed_path(Path::new("/")).is_err());
        assert!(
            validate_firewall_rule(&FirewallRule {
                decision: FirewallDecision::Allow,
                port: 22,
                protocol: lumic_core::server::NetworkProtocol::Tcp,
                source: "any;id".into()
            })
            .is_err()
        )
    }

    #[tokio::test]
    async fn backup_schedule_rejects_unit_injection_before_writing() {
        let dir = std::env::temp_dir().join(format!("lumic-host-test-{}", std::process::id()));
        let operator = HostOperator::at_state_dir(&dir);
        let result = operator
            .schedule_backup(
                BackupSchedule {
                    id: "daily".into(),
                    service_id: "primary-db".into(),
                    database: Some("app\nExecStart=/bin/sh".into()),
                    on_calendar: "daily".into(),
                    enabled: true,
                },
                &OperationContext {
                    actor: "test".into(),
                    interface: lumic_core::OperationInterface::Cli,
                    correlation_id: "host-test".into(),
                    dry_run: false,
                    approved: true,
                },
            )
            .await;
        assert!(result.is_err());
        assert!(!dir.join("backup-schedules.json").exists());
    }
}
