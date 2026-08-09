use crate::{
    ProcessRunner, ProcessSpec,
    atomic_file::write_atomic,
    audit_store::AuditStore,
    event_store::EventStore,
    systemd::{ServiceAction, SystemdServiceManager},
};
use lumic_core::{
    LumicError, OperationContext, Result,
    events::{AuditRecord, Event},
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateResult {
    pub version: String,
    pub changed: bool,
    pub destination: String,
    pub recovery_binary: Option<String>,
}

pub struct SelfUpdateManager {
    state_dir: PathBuf,
    unit_dir: PathBuf,
}

impl SelfUpdateManager {
    pub fn system(state_dir: impl Into<PathBuf>) -> Self {
        Self {
            state_dir: state_dir.into(),
            unit_dir: "/etc/systemd/system".into(),
        }
    }

    pub async fn apply_nightly(&self, context: &OperationContext) -> Result<UpdateResult> {
        let destination = std::env::current_exe().map_err(io_error)?;
        if destination
            .symlink_metadata()
            .map_err(io_error)?
            .file_type()
            .is_symlink()
        {
            return Err(LumicError::InvalidInput {
                field: "executable".into(),
                message:
                    "self-update refuses to replace a symlink; install Lumic as a regular file"
                        .into(),
            });
        }
        let target = release_target()?;
        let name = format!("lumic-{target}");
        let url = format!("https://github.com/bpstr/lumic/releases/download/nightly/{name}");
        let parent = destination.parent().ok_or_else(|| LumicError::Internal {
            message: "Lumic executable has no parent directory".into(),
        })?;
        let temporary = parent.join(format!(".lumic-update-{}", std::process::id()));
        let checksum = parent.join(format!(".lumic-update-{}.sha256", std::process::id()));
        let result = self
            .download_verify_replace(&url, &name, &destination, &temporary, &checksum)
            .await;
        let _ = fs::remove_file(&temporary);
        let _ = fs::remove_file(&checksum);
        let result = match result {
            Ok(result) => result,
            Err(error) => {
                self.record_failure(context, &error)?;
                return Err(error);
            }
        };
        self.record(&result, context)?;
        Ok(result)
    }

    async fn download_verify_replace(
        &self,
        url: &str,
        artifact_name: &str,
        destination: &Path,
        temporary: &Path,
        checksum: &Path,
    ) -> Result<UpdateResult> {
        let mut download = ProcessSpec::new("curl").args([
            "-fL",
            "--retry",
            "3",
            url,
            "-o",
            path_text(temporary)?,
        ]);
        download.timeout = Duration::from_secs(300);
        run_ok(download).await?;
        run_ok(ProcessSpec::new("curl").args([
            "-fL",
            "--retry",
            "3",
            &format!("{url}.sha256"),
            "-o",
            path_text(checksum)?,
        ]))
        .await?;
        let expected = fs::read_to_string(checksum)
            .map_err(io_error)?
            .split_whitespace()
            .next()
            .filter(|value| value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit()))
            .ok_or_else(|| LumicError::Inspection {
                fact: "nightly_checksum".into(),
                message: "release checksum file is invalid".into(),
            })?
            .to_ascii_lowercase();
        let output = run_ok(ProcessSpec::new("sha256sum").args([path_text(temporary)?])).await?;
        let actual = String::from_utf8_lossy(&output.stdout)
            .split_whitespace()
            .next()
            .unwrap_or_default()
            .to_ascii_lowercase();
        if actual != expected {
            return Err(LumicError::Inspection {
                fact: "nightly_checksum".into(),
                message: format!("checksum mismatch for {artifact_name}"),
            });
        }
        set_executable(temporary)?;
        let version_output =
            run_ok(ProcessSpec::new(path_text(temporary)?).args(["version"])).await?;
        let version = String::from_utf8_lossy(&version_output.stdout)
            .trim()
            .to_owned();
        if !version.starts_with("lumic ") {
            return Err(LumicError::Inspection {
                fact: "nightly_binary".into(),
                message: "downloaded executable returned an invalid version".into(),
            });
        }
        if files_equal(temporary, destination)? {
            return Ok(UpdateResult {
                version,
                changed: false,
                destination: destination.to_string_lossy().into(),
                recovery_binary: None,
            });
        }
        let recovery = destination.with_extension("lumic-previous");
        if destination.exists() {
            fs::copy(destination, &recovery).map_err(io_error)?;
            set_executable(&recovery)?;
        }
        fs::rename(temporary, destination).map_err(io_error)?;
        if run_ok(ProcessSpec::new(path_text(destination)?).args(["version"]))
            .await
            .is_err()
        {
            if recovery.exists() {
                fs::rename(&recovery, destination).map_err(io_error)?;
            }
            return Err(LumicError::Inspection {
                fact: "nightly_binary".into(),
                message: "post-install verification failed; previous binary restored".into(),
            });
        }
        Ok(UpdateResult {
            version,
            changed: true,
            destination: destination.to_string_lossy().into(),
            recovery_binary: recovery.exists().then(|| recovery.to_string_lossy().into()),
        })
    }

    pub async fn enable_nightly_timer(&self, context: &OperationContext) -> Result<Vec<String>> {
        let executable = std::env::current_exe().map_err(io_error)?;
        let service = format!(
            "# Managed by Lumic\n[Unit]\nDescription=Lumic verified nightly self-update\nAfter=network-online.target\n\n[Service]\nType=oneshot\nExecStart={} self-update apply\n",
            systemd_quote(path_text(&executable)?)
        );
        let timer = "# Managed by Lumic\n[Unit]\nDescription=Run Lumic nightly self-update\n\n[Timer]\nOnCalendar=daily\nRandomizedDelaySec=2h\nPersistent=true\nUnit=lumic-self-update.service\n\n[Install]\nWantedBy=timers.target\n";
        write_atomic(
            &self.unit_dir.join("lumic-self-update.service"),
            service.as_bytes(),
            0o644,
        )?;
        write_atomic(
            &self.unit_dir.join("lumic-self-update.timer"),
            timer.as_bytes(),
            0o644,
        )?;
        let systemd = SystemdServiceManager::at_state_dir(&self.state_dir);
        systemd.daemon_reload().await?;
        systemd
            .apply("lumic-self-update.timer", ServiceAction::Enable, context)
            .await?;
        systemd
            .apply("lumic-self-update.timer", ServiceAction::Start, context)
            .await?;
        Ok(vec![
            "lumic-self-update.service".into(),
            "lumic-self-update.timer".into(),
        ])
    }

    fn record(&self, result: &UpdateResult, context: &OperationContext) -> Result<()> {
        EventStore::at_state_dir(&self.state_dir).append(&Event::now(
            "lumic.updated",
            &context.actor,
            context.interface,
            "lumic",
            "self",
            &context.correlation_id,
            json!({"version": result.version, "changed": result.changed}),
        ))?;
        AuditStore::at_state_dir(&self.state_dir).append(&AuditRecord::now(
            context,
            "lumic.self_update",
            "self_update",
            "lumic",
            "self",
            json!({"channel": "nightly"}),
            None,
            Some(json!({"version": result.version, "changed": result.changed, "recovery_binary": result.recovery_binary})),
            true,
            "verified nightly binary installed",
        ))
    }

    fn record_failure(&self, context: &OperationContext, error: &LumicError) -> Result<()> {
        AuditStore::at_state_dir(&self.state_dir).append(&AuditRecord::now(
            context,
            "lumic.self_update",
            "self_update",
            "lumic",
            "self",
            json!({"channel": "nightly"}),
            None,
            None,
            false,
            error.to_string(),
        ))
    }
}

async fn run_ok(spec: ProcessSpec) -> Result<crate::ProcessOutput> {
    let executable = spec.executable.clone();
    let output = ProcessRunner.run(&spec).await?;
    if output.success() {
        Ok(output)
    } else {
        Err(LumicError::Process {
            executable,
            message: String::from_utf8_lossy(&output.stderr).trim().into(),
        })
    }
}

fn release_target() -> Result<&'static str> {
    match std::env::consts::ARCH {
        "x86_64" => Ok("x86_64-unknown-linux-musl"),
        architecture => Err(LumicError::UnsupportedPlatform {
            platform: format!("nightly release artifact for architecture {architecture}"),
        }),
    }
}

fn files_equal(left: &Path, right: &Path) -> Result<bool> {
    if !right.exists() {
        return Ok(false);
    }
    Ok(fs::read(left).map_err(io_error)? == fs::read(right).map_err(io_error)?)
}

#[cfg(unix)]
fn set_executable(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).map_err(io_error)
}

#[cfg(not(unix))]
fn set_executable(_path: &Path) -> Result<()> {
    Err(LumicError::UnsupportedPlatform {
        platform: "self-update requires Unix executable permissions".into(),
    })
}

fn path_text(path: &Path) -> Result<&str> {
    path.to_str().ok_or_else(|| LumicError::InvalidInput {
        field: "path".into(),
        message: "must be valid UTF-8".into(),
    })
}

fn systemd_quote(value: &str) -> String {
    format!(
        "\"{}\"",
        value
            .replace('%', "%%")
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
    )
}

fn io_error(error: std::io::Error) -> LumicError {
    LumicError::Internal {
        message: format!("self-update I/O failed: {error}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_and_systemd_command_are_explicit() {
        if std::env::consts::ARCH == "x86_64" {
            assert_eq!(release_target().unwrap(), "x86_64-unknown-linux-musl");
        } else {
            assert!(release_target().is_err());
        }
        let quoted = systemd_quote("/usr/local/bin/lumic");
        assert_eq!(quoted, "\"/usr/local/bin/lumic\"");
        assert!(!quoted.contains("sh -c"));
    }
}
