use crate::{ProcessOutput, ProcessRunner, ProcessSpec, audit_store::AuditStore};
use lumic_core::{
    LumicError, OperationContext, Result,
    events::{AuditRecord, Event},
    package::{PackageMutation, PackageName, PackagePolicy, PackageRecord},
};
use serde_json::json;
use std::time::Duration;

use crate::event_store::EventStore;

const APT_LOCK_TIMEOUT_SECONDS: u64 = 60;

#[derive(Debug, Clone)]
pub struct AptPackageManager {
    runner: ProcessRunner,
    policy: PackagePolicy,
    events: EventStore,
    audit: AuditStore,
}

impl AptPackageManager {
    pub fn new(policy: PackagePolicy, events: EventStore) -> Self {
        let audit = AuditStore::at_state_dir(events.state_dir());
        Self {
            runner: ProcessRunner,
            policy,
            events,
            audit,
        }
    }

    pub fn system(events: EventStore) -> Self {
        Self::new(PackagePolicy::default_catalog(), events)
    }

    pub fn policy(&self) -> &PackagePolicy {
        &self.policy
    }

    pub async fn search(&self, query: &PackageName) -> Result<Vec<PackageRecord>> {
        let output = self
            .run(ProcessSpec::new("apt-cache").args(["search", "--names-only", query.as_str()]))
            .await?;
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter_map(|line| line.split_once(" - "))
            .take(100)
            .map(|(name, summary)| {
                Ok(PackageRecord {
                    name: PackageName::parse(name)?,
                    installed_version: None,
                    candidate_version: None,
                    summary: Some(summary.to_owned()),
                })
            })
            .collect()
    }

    pub async fn inspect(&self, package: &PackageName) -> Result<PackageRecord> {
        let installed = self.installed_version(package).await?;
        let output = self
            .run(ProcessSpec::new("apt-cache").args(["policy", package.as_str()]))
            .await?;
        let candidate = String::from_utf8_lossy(&output.stdout)
            .lines()
            .find_map(|line| line.trim().strip_prefix("Candidate: "))
            .filter(|version| *version != "(none)")
            .map(str::to_owned);
        Ok(PackageRecord {
            name: package.clone(),
            installed_version: installed,
            candidate_version: candidate,
            summary: None,
        })
    }

    pub async fn update_index(&self, context: &OperationContext) -> Result<PackageMutation> {
        let apt = PackageName::parse("apt")?;
        if context.dry_run {
            return Ok(PackageMutation {
                package: apt,
                action: "update_index".into(),
                changed: false,
                output: "dry run: apt package metadata would be refreshed".into(),
            });
        }
        let mut spec = apt_get(["update"]);
        spec.timeout = Duration::from_secs(300);
        let output = self
            .run_mutation(spec, &apt, "update_index", context, None)
            .await?;
        let mutation = PackageMutation {
            package: apt,
            action: "update_index".into(),
            changed: true,
            output: clean_output(&output),
        };
        self.emit("package.index_updated", &mutation, context)?;
        Ok(mutation)
    }

    pub async fn install(
        &self,
        package: &PackageName,
        context: &OperationContext,
    ) -> Result<PackageMutation> {
        self.install_with_environment(package, context, &[]).await
    }

    pub(crate) async fn install_with_environment(
        &self,
        package: &PackageName,
        context: &OperationContext,
        environment: &[(&str, &str)],
    ) -> Result<PackageMutation> {
        self.policy.authorize(package)?;
        let record = self.inspect(package).await?;
        let installed = record.installed_version;
        if installed.is_some() {
            return Ok(PackageMutation {
                package: package.clone(),
                action: "install".into(),
                changed: false,
                output: "already installed".into(),
            });
        }
        require_candidate(&record.name, record.candidate_version.as_deref())?;
        if context.dry_run {
            return Ok(PackageMutation {
                package: package.clone(),
                action: "install".into(),
                changed: false,
                output: "dry run: package is trusted and would be installed".into(),
            });
        }
        let mut spec = apt_get([
            "install",
            "--yes",
            "--no-install-recommends",
            "--",
            package.as_str(),
        ]);
        spec.environment.extend(
            environment
                .iter()
                .map(|(key, value)| ((*key).into(), (*value).into())),
        );
        spec.timeout = Duration::from_secs(600);
        let output = self
            .run_mutation(spec, package, "install", context, installed)
            .await?;
        let mutation = PackageMutation {
            package: package.clone(),
            action: "install".into(),
            changed: true,
            output: clean_output(&output),
        };
        self.emit("package.installed", &mutation, context)?;
        Ok(mutation)
    }

    pub async fn remove(
        &self,
        package: &PackageName,
        context: &OperationContext,
    ) -> Result<PackageMutation> {
        self.policy.authorize(package)?;
        let installed = self.installed_version(package).await?;
        if installed.is_none() {
            return Ok(PackageMutation {
                package: package.clone(),
                action: "remove".into(),
                changed: false,
                output: "not installed".into(),
            });
        }
        if context.dry_run {
            return Ok(PackageMutation {
                package: package.clone(),
                action: "remove".into(),
                changed: false,
                output: "dry run: package would be removed".into(),
            });
        }
        let mut spec = apt_get(["remove", "--yes", "--", package.as_str()]);
        spec.timeout = Duration::from_secs(600);
        let output = self
            .run_mutation(spec, package, "remove", context, installed)
            .await?;
        let mutation = PackageMutation {
            package: package.clone(),
            action: "remove".into(),
            changed: true,
            output: clean_output(&output),
        };
        self.emit("package.removed", &mutation, context)?;
        Ok(mutation)
    }

    pub async fn upgrade(
        &self,
        package: &PackageName,
        context: &OperationContext,
    ) -> Result<PackageMutation> {
        self.policy.authorize(package)?;
        let installed = self.installed_version(package).await?;
        if installed.is_none() {
            return Err(LumicError::InvalidInput {
                field: "package".into(),
                message: "package must be installed before it can be upgraded".into(),
            });
        }
        let record = self.inspect(package).await?;
        require_candidate(&record.name, record.candidate_version.as_deref())?;
        if record.candidate_version == installed {
            return Ok(PackageMutation {
                package: package.clone(),
                action: "upgrade".into(),
                changed: false,
                output: "already at candidate version".into(),
            });
        }
        if context.dry_run {
            return Ok(PackageMutation {
                package: package.clone(),
                action: "upgrade".into(),
                changed: false,
                output: "dry run: package would be upgraded".into(),
            });
        }
        let mut spec = apt_get([
            "install",
            "--only-upgrade",
            "--yes",
            "--no-install-recommends",
            "--",
            package.as_str(),
        ]);
        spec.timeout = Duration::from_secs(600);
        let output = self
            .run_mutation(spec, package, "upgrade", context, installed)
            .await?;
        let mutation = PackageMutation {
            package: package.clone(),
            action: "upgrade".into(),
            changed: true,
            output: clean_output(&output),
        };
        self.emit("package.upgraded", &mutation, context)?;
        Ok(mutation)
    }

    async fn installed_version(&self, package: &PackageName) -> Result<Option<String>> {
        let spec = ProcessSpec::new("dpkg-query").args([
            "--show",
            "--showformat=${db:Status-Abbrev}\t${Version}",
            package.as_str(),
        ]);
        let output = self.runner.run(&spec).await?;
        if !output.success() {
            return Ok(None);
        }
        let text = String::from_utf8_lossy(&output.stdout);
        Ok(text.strip_prefix("ii ").and_then(|value| {
            value
                .split_once('\t')
                .map(|(_, version)| version.trim().to_owned())
        }))
    }

    async fn run(&self, spec: ProcessSpec) -> Result<ProcessOutput> {
        let executable = spec.executable.clone();
        let output = self.runner.run(&spec).await?;
        if output.success() {
            Ok(output)
        } else {
            Err(LumicError::Process {
                executable,
                message: clean_output(&output),
            })
        }
    }

    async fn run_mutation(
        &self,
        spec: ProcessSpec,
        package: &PackageName,
        action: &str,
        context: &OperationContext,
        before_version: Option<String>,
    ) -> Result<ProcessOutput> {
        match self.run(spec).await {
            Ok(output) => Ok(output),
            Err(error) => {
                self.audit.append(&AuditRecord::now(
                    context,
                    format!("package.{action}"),
                    action,
                    "package",
                    package.as_str(),
                    json!({"package": package}),
                    before_version.map(|version| json!({"version": version})),
                    None,
                    false,
                    error.to_string(),
                ))?;
                Err(error)
            }
        }
    }

    fn emit(
        &self,
        event_type: &str,
        mutation: &PackageMutation,
        context: &OperationContext,
    ) -> Result<()> {
        self.events.append(&Event::now(
            event_type,
            &context.actor,
            context.interface,
            "package",
            mutation.package.as_str(),
            &context.correlation_id,
            json!({"action": mutation.action, "changed": mutation.changed}),
        ))?;
        self.audit.append(&AuditRecord::now(
            context,
            format!("package.{}", mutation.action),
            &mutation.action,
            "package",
            mutation.package.as_str(),
            json!({"package": mutation.package, "changed": mutation.changed}),
            None,
            Some(json!({"changed": mutation.changed})),
            true,
            "package operation completed",
        ))
    }
}

fn apt_get(args: impl IntoIterator<Item = impl Into<String>>) -> ProcessSpec {
    let mut spec = ProcessSpec::new("apt-get").args([
        "-o".into(),
        format!("Dpkg::Lock::Timeout={APT_LOCK_TIMEOUT_SECONDS}"),
        "-o".into(),
        "Dpkg::Options::=--force-confdef".into(),
        "-o".into(),
        "Dpkg::Options::=--force-confold".into(),
    ]);
    spec.args.extend(args.into_iter().map(Into::into));
    spec.environment
        .insert("APT_LISTCHANGES_FRONTEND".into(), "none".into());
    spec.environment
        .insert("DEBIAN_FRONTEND".into(), "noninteractive".into());
    spec.environment
        .insert("NEEDRESTART_MODE".into(), "l".into());
    spec
}

fn require_candidate(package: &PackageName, candidate: Option<&str>) -> Result<()> {
    if candidate.is_some() {
        return Ok(());
    }
    Err(LumicError::InvalidInput {
        field: "apt_sources".into(),
        message: format!(
            "no install candidate for {package}; configure a trusted apt source before setup"
        ),
    })
}

fn clean_output(output: &ProcessOutput) -> String {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let text = if stderr.trim().is_empty() {
        stdout
    } else {
        stderr
    };
    let text = text.trim();
    if text.is_empty() {
        format!("process exited with {:?}", output.exit_code)
    } else {
        text.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumic_core::{ErrorCode, OperationInterface};

    #[tokio::test]
    async fn dry_run_does_not_refresh_apt_metadata() {
        let state_dir = std::env::temp_dir().join(format!(
            "lumic-apt-update-dry-run-test-{}",
            std::process::id()
        ));
        let manager = AptPackageManager::system(EventStore::at_state_dir(state_dir));
        let context = OperationContext {
            actor: "test".into(),
            interface: OperationInterface::Internal,
            correlation_id: "apt-update-dry-run-test".into(),
            dry_run: true,
            approved: true,
        };

        let mutation = manager.update_index(&context).await.unwrap();

        assert!(!mutation.changed);
        assert_eq!(mutation.action, "update_index");
        assert!(mutation.output.contains("would be refreshed"));
    }

    #[test]
    fn require_candidate_rejects_a_package_missing_from_configured_sources() {
        let package = PackageName::parse("example").unwrap();

        let error = require_candidate(&package, None).unwrap_err();

        assert_eq!(error.code(), ErrorCode::InvalidInput);
    }

    #[test]
    fn require_candidate_accepts_an_available_package() {
        let package = PackageName::parse("example").unwrap();

        assert!(require_candidate(&package, Some("1.0")).is_ok());
    }

    #[test]
    fn apt_get_disables_interactive_package_prompts() {
        let spec = apt_get(["update"]);

        assert_eq!(
            spec.environment.get("DEBIAN_FRONTEND").map(String::as_str),
            Some("noninteractive")
        );
    }

    #[test]
    fn apt_get_bounds_package_lock_waits() {
        let spec = apt_get(["update"]);

        assert!(
            spec.args
                .iter()
                .any(|argument| argument == "Dpkg::Lock::Timeout=60")
        );
    }
}
