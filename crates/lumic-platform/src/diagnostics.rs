use crate::{ProcessRunner, ProcessSpec, inspect_host, server::HostOperator};
use lumic_core::{
    DiagnosticFinding, DiagnosticReport, LoadFacts, LumicError, ProcessFacts, Result,
    server::MountStatus,
};
use std::{cmp::Reverse, fs};

pub async fn diagnose_host() -> Result<DiagnosticReport> {
    let host = inspect_host()?;
    let load = parse_load(
        &fs::read_to_string("/proc/loadavg").map_err(|error| inspection("load", error))?,
        &fs::read_to_string("/proc/uptime").map_err(|error| inspection("uptime", error))?,
    )?;
    let top_processes = inspect_processes(10)?;
    let failed_services = failed_services().await.unwrap_or_default();
    let operator = HostOperator::at_state_dir(
        std::env::var_os("LUMIC_STATE_DIR").unwrap_or_else(|| "/var/lib/lumic".into()),
    );
    let listeners = operator.listeners().await.unwrap_or_default();
    let mounts = operator.mounts().unwrap_or_default();
    let timers = operator.timers().await.unwrap_or_default();
    let updates = operator.updates().await.unwrap_or_default();
    let mut findings = Vec::new();
    let cpu_count = host.cpu_count.max(1) as f64;
    if load.one_minute > cpu_count * 1.5 {
        findings.push(DiagnosticFinding {
            severity: "warning".into(),
            summary: "host load is elevated".into(),
            evidence: format!(
                "1 minute load {:.2} across {} CPUs",
                load.one_minute, host.cpu_count
            ),
            recommendation: "inspect the listed high-memory processes and system journal".into(),
        });
    }
    let available_ratio =
        host.memory.available_bytes as f64 / host.memory.total_bytes.max(1) as f64;
    if available_ratio < 0.1 {
        findings.push(DiagnosticFinding {
            severity: "warning".into(),
            summary: "available memory is low".into(),
            evidence: format!(
                "{} bytes available of {}",
                host.memory.available_bytes, host.memory.total_bytes
            ),
            recommendation: "inspect process memory and recent OOM events".into(),
        });
    }
    for unit in &failed_services {
        findings.push(DiagnosticFinding {
            severity: "error".into(),
            summary: format!("systemd unit {unit} is failed"),
            evidence: "systemctl --failed reports the unit".into(),
            recommendation: format!("inspect journal logs for {unit} before restarting it"),
        });
    }
    for mount in &mounts {
        if let Some(finding) = filesystem_pressure_finding(mount) {
            findings.push(finding);
        }
    }
    let security_updates = updates.iter().filter(|update| update.security).count();
    if security_updates > 0 {
        findings.push(DiagnosticFinding {
            severity: "warning".into(),
            summary: "security updates are pending".into(),
            evidence: format!("{security_updates} security-classified packages can be upgraded"),
            recommendation: "review pending versions, then apply the security update scope during an approved maintenance window".into(),
        });
    }
    Ok(DiagnosticReport {
        host,
        load,
        top_processes,
        failed_services,
        listeners,
        mounts,
        timers,
        updates,
        findings,
    })
}

fn filesystem_pressure_finding(mount: &MountStatus) -> Option<DiagnosticFinding> {
    if !capacity_is_actionable(mount)
        || mount.total_bytes == 0
        || mount.available_bytes.saturating_mul(100) / mount.total_bytes >= 10
    {
        return None;
    }
    Some(DiagnosticFinding {
        severity: "warning".into(),
        summary: format!("filesystem {} is nearly full", mount.mount_point),
        evidence: format!(
            "{} of {} bytes remain available",
            mount.available_bytes, mount.total_bytes
        ),
        recommendation: "inspect application releases, backups, and journal usage before applying a bounded cleanup".into(),
    })
}

fn capacity_is_actionable(mount: &MountStatus) -> bool {
    const VIRTUAL_OR_IMMUTABLE: &[&str] = &[
        "autofs",
        "binfmt_misc",
        "bpf",
        "cgroup",
        "cgroup2",
        "configfs",
        "debugfs",
        "devpts",
        "devtmpfs",
        "efivarfs",
        "fusectl",
        "hugetlbfs",
        "iso9660",
        "mqueue",
        "nsfs",
        "proc",
        "pstore",
        "ramfs",
        "rpc_pipefs",
        "securityfs",
        "squashfs",
        "sysfs",
        "tmpfs",
        "tracefs",
    ];
    !mount.options.iter().any(|option| option == "ro")
        && !VIRTUAL_OR_IMMUTABLE.contains(&mount.filesystem.as_str())
}

fn parse_load(loadavg: &str, uptime: &str) -> Result<LoadFacts> {
    let mut values = loadavg.split_whitespace();
    let parse = |value: Option<&str>, fact: &str| {
        value
            .and_then(|value| value.parse::<f64>().ok())
            .ok_or_else(|| inspection(fact, "invalid numeric value"))
    };
    let one_minute = parse(values.next(), "load")?;
    let five_minutes = parse(values.next(), "load")?;
    let fifteen_minutes = parse(values.next(), "load")?;
    let processes = values
        .next()
        .ok_or_else(|| inspection("load", "process counts missing"))?;
    let (running_processes, total_processes) = processes
        .split_once('/')
        .and_then(|(running, total)| Some((running.parse().ok()?, total.parse().ok()?)))
        .ok_or_else(|| inspection("load", "invalid process counts"))?;
    let uptime_seconds = uptime
        .split_whitespace()
        .next()
        .and_then(|value| value.split('.').next())
        .and_then(|value| value.parse().ok())
        .ok_or_else(|| inspection("uptime", "invalid uptime"))?;
    Ok(LoadFacts {
        one_minute,
        five_minutes,
        fifteen_minutes,
        running_processes,
        total_processes,
        uptime_seconds,
    })
}

fn inspect_processes(limit: usize) -> Result<Vec<ProcessFacts>> {
    let mut processes = Vec::new();
    for entry in fs::read_dir("/proc").map_err(|error| inspection("processes", error))? {
        let entry = entry.map_err(|error| inspection("processes", error))?;
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
        let mut name = String::new();
        let mut state = String::new();
        let mut resident_bytes = 0;
        for line in status.lines() {
            if let Some(value) = line.strip_prefix("Name:\t") {
                name = value.into();
            }
            if let Some(value) = line.strip_prefix("State:\t") {
                state = value.into();
            }
            if let Some(value) = line.strip_prefix("VmRSS:\t") {
                resident_bytes = value
                    .split_whitespace()
                    .next()
                    .and_then(|value| value.parse::<u64>().ok())
                    .unwrap_or(0)
                    .saturating_mul(1024);
            }
        }
        processes.push(ProcessFacts {
            pid,
            name,
            state,
            resident_bytes,
        });
    }
    processes.sort_by_key(|process| Reverse(process.resident_bytes));
    processes.truncate(limit);
    Ok(processes)
}

async fn failed_services() -> Result<Vec<String>> {
    let spec = ProcessSpec::new("systemctl").args([
        "--failed",
        "--no-legend",
        "--plain",
        "--type=service",
    ]);
    let output = ProcessRunner.run(&spec).await?;
    if !output.success() {
        return Err(LumicError::Process {
            executable: "systemctl".into(),
            message: String::from_utf8_lossy(&output.stderr).trim().into(),
        });
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| line.split_whitespace().next().map(str::to_owned))
        .collect())
}

fn inspection(fact: &str, error: impl std::fmt::Display) -> LumicError {
    LumicError::Inspection {
        fact: fact.into(),
        message: error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mount(filesystem: &str, options: &[&str]) -> MountStatus {
        MountStatus {
            source: "/dev/test".into(),
            mount_point: "/test".into(),
            filesystem: filesystem.into(),
            options: options.iter().map(|value| (*value).into()).collect(),
            total_bytes: 100,
            available_bytes: 1,
        }
    }

    #[test]
    fn ignores_full_immutable_and_virtual_mounts() {
        assert!(filesystem_pressure_finding(&mount("squashfs", &["ro"])).is_none());
        assert!(filesystem_pressure_finding(&mount("tmpfs", &["rw"])).is_none());
    }

    #[test]
    fn reports_pressure_on_writable_persistent_mounts() {
        let finding = filesystem_pressure_finding(&mount("ext4", &["rw"])).unwrap();
        assert_eq!(finding.summary, "filesystem /test is nearly full");
    }

    #[test]
    fn parses_proc_load_and_uptime() {
        let load = parse_load("0.10 0.20 0.30 2/100 123", "456.78 100.00").unwrap();
        assert_eq!(load.running_processes, 2);
        assert_eq!(load.total_processes, 100);
        assert_eq!(load.uptime_seconds, 456);
    }
}
