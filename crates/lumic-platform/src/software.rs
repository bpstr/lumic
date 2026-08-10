use crate::{
    ProcessOutput, ProcessRunner, ProcessSpec, apt::AptPackageManager, audit_store::AuditStore,
    event_store::EventStore,
};
use lumic_core::{
    LumicError, OperationContext, Plan, Result,
    events::{AuditRecord, Event},
    package::{PackageMutation, PackageName, PackageRecord},
    server::validate_account_name,
    software::{
        SoftwareDefinition, SoftwarePackageSource, SoftwareSetupScope, setup_plan, software,
    },
};
use serde::Serialize;
use serde_json::json;
use std::{
    fs,
    path::{Path, PathBuf},
};

const NVM_VERSION: &str = "v0.40.6";
const NVM_REPOSITORY: &str = "https://github.com/nvm-sh/nvm.git";
const NVM_PROFILE_BLOCK: &str = "\n# Lumic: NVM\nexport NVM_DIR=\"$HOME/.nvm\"\n[ -s \"$NVM_DIR/nvm.sh\" ] && \\. \"$NVM_DIR/nvm.sh\"\n[ -s \"$NVM_DIR/bash_completion\" ] && \\. \"$NVM_DIR/bash_completion\"\n";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SoftwareStatus {
    pub software: SoftwareDefinition,
    pub installed: bool,
    pub available: bool,
    pub requires_index_refresh: bool,
    pub requires_repository: bool,
    pub update_available: bool,
    pub installed_version: Option<String>,
    pub candidate_version: Option<String>,
    pub target_user: Option<String>,
    pub unavailable_packages: Vec<String>,
    pub packages: Vec<PackageRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SoftwareSetupResult {
    pub software: SoftwareDefinition,
    pub changed: bool,
    pub installed_version: Option<String>,
    pub target_user: Option<String>,
    pub packages: Vec<PackageMutation>,
}

#[derive(Debug, Clone)]
pub struct SoftwareManager {
    packages: AptPackageManager,
    runner: ProcessRunner,
    events: EventStore,
    audit: AuditStore,
}

impl SoftwareManager {
    pub fn at_state_dir(state_dir: impl AsRef<Path>) -> Self {
        Self {
            packages: AptPackageManager::system(EventStore::at_state_dir(&state_dir)),
            runner: ProcessRunner,
            events: EventStore::at_state_dir(&state_dir),
            audit: AuditStore::at_state_dir(state_dir),
        }
    }

    pub async fn status(&self, id: &str) -> Result<SoftwareStatus> {
        self.status_for_user(id, None).await
    }

    pub async fn status_for_user(
        &self,
        id: &str,
        target_user: Option<&str>,
    ) -> Result<SoftwareStatus> {
        let definition = *software(id)?;
        let mut packages = Vec::with_capacity(definition.packages.len());
        for name in definition.packages {
            packages.push(self.packages.inspect(&PackageName::parse(*name)?).await?);
        }
        let unavailable_packages = unavailable_package_names(&packages);
        let available = unavailable_packages.is_empty();
        let requires_index_refresh =
            !available && definition.package_source == SoftwarePackageSource::Distribution;
        let requires_repository =
            !available && definition.package_source == SoftwarePackageSource::ExternalRepository;

        if definition.setup_scope == SoftwareSetupScope::User {
            let version = if let Some(user) = target_user {
                validate_account_name("user", user)?;
                let home = self.user_home(user).await?;
                self.nvm_version(user, &home).await?
            } else {
                None
            };
            return Ok(SoftwareStatus {
                software: definition,
                installed: version.is_some(),
                available,
                requires_index_refresh,
                requires_repository,
                update_available: version.as_deref().is_some_and(|value| value != NVM_VERSION),
                installed_version: version,
                candidate_version: Some(NVM_VERSION.into()),
                target_user: target_user.map(str::to_owned),
                unavailable_packages,
                packages,
            });
        }

        let installed = packages.iter().all(|item| item.installed_version.is_some());
        let update_available = packages.iter().any(|item| {
            matches!((&item.installed_version, &item.candidate_version), (Some(installed), Some(candidate)) if installed != candidate)
        });
        Ok(SoftwareStatus {
            software: definition,
            installed,
            available,
            requires_index_refresh,
            requires_repository,
            update_available,
            installed_version: None,
            candidate_version: None,
            target_user: None,
            unavailable_packages,
            packages,
        })
    }

    pub async fn plan_setup(&self, id: &str) -> Result<Plan> {
        self.plan_setup_for_user(id, None).await
    }

    pub async fn plan_setup_for_user(&self, id: &str, target_user: Option<&str>) -> Result<Plan> {
        let status = self.status_for_user(id, target_user).await?;
        let mut plan = setup_plan(&status.software, status.installed);
        if status.requires_repository {
            plan.preconditions.push(format!(
                "Configure a trusted apt source that provides: {}",
                status.unavailable_packages.join(", ")
            ));
        } else if status.requires_index_refresh {
            if let Some(change) = plan.changes.first_mut() {
                change.summary = format!(
                    "Refresh apt package metadata, then {}",
                    change.summary.to_lowercase()
                );
            }
            plan.validation.push(
                "Re-check every required package candidate after refreshing apt metadata".into(),
            );
        }
        Ok(plan)
    }

    pub async fn setup(&self, id: &str, context: &OperationContext) -> Result<SoftwareSetupResult> {
        self.setup_for_user(id, None, context).await
    }

    pub async fn setup_for_user(
        &self,
        id: &str,
        target_user: Option<&str>,
        context: &OperationContext,
    ) -> Result<SoftwareSetupResult> {
        let definition = *software(id)?;
        let required_user = if definition.setup_scope == SoftwareSetupScope::User {
            Some(target_user.ok_or_else(|| LumicError::InvalidInput {
                field: "user".into(),
                message: "is required for a per-user installer".into(),
            })?)
        } else {
            None
        };
        let mut status = self.status_for_user(id, target_user).await?;
        if status.requires_index_refresh {
            if context.dry_run {
                return Err(LumicError::InvalidInput {
                    field: "package_index".into(),
                    message: "apt package metadata must be refreshed before a setup dry run; review the setup plan or run an approved setup".into(),
                });
            }
            self.packages.update_index(context).await?;
            status = self.status_for_user(id, target_user).await?;
        }
        ensure_packages_available(&status)?;
        if let Some(user) = required_user {
            return self.setup_nvm(definition, user, context).await;
        }

        let packages = self.install_packages(&definition, context).await?;
        let changed = packages.iter().any(|item| item.changed);
        Ok(SoftwareSetupResult {
            software: definition,
            changed,
            installed_version: None,
            target_user: None,
            packages,
        })
    }

    async fn install_packages(
        &self,
        definition: &SoftwareDefinition,
        context: &OperationContext,
    ) -> Result<Vec<PackageMutation>> {
        let mut packages = Vec::with_capacity(definition.packages.len());
        for name in definition.packages {
            packages.push(
                self.packages
                    .install(&PackageName::parse(*name)?, context)
                    .await?,
            );
        }
        Ok(packages)
    }

    async fn setup_nvm(
        &self,
        definition: SoftwareDefinition,
        user: &str,
        context: &OperationContext,
    ) -> Result<SoftwareSetupResult> {
        validate_account_name("user", user)?;
        let home = self.user_home(user).await?;
        let before = self.nvm_version(user, &home).await?;
        let packages = self.install_packages(&definition, context).await?;
        let mut changed = packages.iter().any(|item| item.changed);
        let nvm_dir = home.join(".nvm");

        if nvm_dir.join(".git").is_dir() {
            self.run_checked(ProcessSpec::new("runuser").args([
                "--user",
                user,
                "--",
                "git",
                "-C",
                &nvm_dir.to_string_lossy(),
                "fetch",
                "--force",
                "--depth",
                "1",
                "origin",
                "tag",
                NVM_VERSION,
            ]))
            .await?;
            self.run_checked(ProcessSpec::new("runuser").args([
                "--user",
                user,
                "--",
                "git",
                "-C",
                &nvm_dir.to_string_lossy(),
                "checkout",
                "--detach",
                NVM_VERSION,
            ]))
            .await?;
            changed |= before.as_deref() != Some(NVM_VERSION);
        } else if nvm_dir.exists() {
            return Err(LumicError::InvalidInput {
                field: "user".into(),
                message: format!(
                    "{} exists but is not an NVM Git checkout",
                    nvm_dir.display()
                ),
            });
        } else {
            self.run_checked(ProcessSpec::new("runuser").args([
                "--user",
                user,
                "--",
                "git",
                "clone",
                "--branch",
                NVM_VERSION,
                "--depth",
                "1",
                NVM_REPOSITORY,
                &nvm_dir.to_string_lossy(),
            ]))
            .await?;
            changed = true;
        }

        let profile = home.join(".profile");
        let profile_text = match fs::read_to_string(&profile) {
            Ok(contents) => contents,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(error) => {
                return Err(LumicError::Inspection {
                    fact: "nvm_profile".into(),
                    message: error.to_string(),
                });
            }
        };
        if !profile_text.contains("# Lumic: NVM") {
            self.run_checked(
                ProcessSpec::new("runuser")
                    .args([
                        "--user",
                        user,
                        "--",
                        "tee",
                        "-a",
                        &profile.to_string_lossy(),
                    ])
                    .stdin(NVM_PROFILE_BLOCK.as_bytes()),
            )
            .await?;
            changed = true;
        }

        let after = self.nvm_version(user, &home).await?;
        self.record_nvm(user, before.as_deref(), after.as_deref(), changed, context)?;
        Ok(SoftwareSetupResult {
            software: definition,
            changed,
            installed_version: after,
            target_user: Some(user.into()),
            packages,
        })
    }

    async fn user_home(&self, user: &str) -> Result<PathBuf> {
        let output = self
            .run_checked(ProcessSpec::new("getent").args(["passwd", user]))
            .await?;
        let record = String::from_utf8_lossy(&output.stdout);
        let home = record.split(':').nth(5).map(str::trim).unwrap_or_default();
        let home = PathBuf::from(home);
        if !home.is_absolute() || !home.is_dir() {
            return Err(LumicError::InvalidInput {
                field: "user".into(),
                message: "must reference an existing account with an absolute home directory"
                    .into(),
            });
        }
        Ok(home)
    }

    async fn nvm_version(&self, user: &str, home: &Path) -> Result<Option<String>> {
        let directory = home.join(".nvm");
        if !directory.join("nvm.sh").is_file() {
            return Ok(None);
        }
        if !directory.join(".git").is_dir() {
            return Ok(Some("unknown".into()));
        }
        let output = self
            .runner
            .run(&ProcessSpec::new("runuser").args([
                "--user",
                user,
                "--",
                "git",
                "-C",
                &directory.to_string_lossy(),
                "describe",
                "--tags",
                "--exact-match",
            ]))
            .await?;
        if output.success() {
            Ok(Some(String::from_utf8_lossy(&output.stdout).trim().into()))
        } else {
            Ok(Some("unversioned".into()))
        }
    }

    async fn run_checked(&self, spec: ProcessSpec) -> Result<ProcessOutput> {
        let executable = spec.executable.clone();
        let output = self.runner.run(&spec).await?;
        if output.success() {
            Ok(output)
        } else {
            Err(LumicError::Process {
                executable,
                message: String::from_utf8_lossy(&output.stderr).trim().into(),
            })
        }
    }

    fn record_nvm(
        &self,
        user: &str,
        before: Option<&str>,
        after: Option<&str>,
        changed: bool,
        context: &OperationContext,
    ) -> Result<()> {
        let args = json!({"user": user, "version": NVM_VERSION});
        self.events.append(&Event::now(
            "software.nvm.setup",
            &context.actor,
            context.interface,
            "software",
            "nvm",
            &context.correlation_id,
            args.clone(),
        ))?;
        self.audit.append(&AuditRecord::now(
            context,
            "software.setup",
            "setup",
            "software",
            "nvm",
            args,
            before.map(|version| json!({"version": version, "user": user})),
            after.map(|version| json!({"version": version, "user": user})),
            true,
            if changed {
                "NVM setup changed"
            } else {
                "NVM setup already current"
            },
        ))
    }
}

fn unavailable_package_names(packages: &[PackageRecord]) -> Vec<String> {
    packages
        .iter()
        .filter(|package| {
            package.installed_version.is_none() && package.candidate_version.is_none()
        })
        .map(|package| package.name.as_str().to_owned())
        .collect()
}

fn ensure_packages_available(status: &SoftwareStatus) -> Result<()> {
    if status.available {
        return Ok(());
    }
    let (field, guidance) = if status.requires_repository {
        (
            "apt_sources",
            "configure a trusted external apt source before setup",
        )
    } else {
        (
            "package_index",
            "verify the supported Debian or Ubuntu sources after refreshing apt metadata",
        )
    };
    Err(LumicError::InvalidInput {
        field: field.into(),
        message: format!(
            "no install candidate for {}; {guidance}",
            status.unavailable_packages.join(", ")
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumic_core::{ErrorCode, OperationInterface};

    #[tokio::test]
    async fn nvm_setup_requires_an_explicit_target_user() {
        let state_dir =
            std::env::temp_dir().join(format!("lumic-nvm-target-test-{}", std::process::id()));
        let context = OperationContext {
            actor: "test".into(),
            interface: OperationInterface::Internal,
            correlation_id: "nvm-target-test".into(),
            dry_run: false,
            approved: true,
        };

        let error = SoftwareManager::at_state_dir(state_dir)
            .setup_for_user("nvm", None, &context)
            .await
            .unwrap_err();

        assert_eq!(error.code(), ErrorCode::InvalidInput);
    }

    #[test]
    fn unavailable_package_names_ignores_installed_packages_without_a_candidate() {
        let packages = vec![PackageRecord {
            name: PackageName::parse("example").unwrap(),
            installed_version: Some("1.0".into()),
            candidate_version: None,
            summary: None,
        }];

        assert!(unavailable_package_names(&packages).is_empty());
    }

    #[test]
    fn unavailable_package_names_reports_missing_candidates() {
        let packages = vec![PackageRecord {
            name: PackageName::parse("example").unwrap(),
            installed_version: None,
            candidate_version: None,
            summary: None,
        }];

        assert_eq!(unavailable_package_names(&packages), vec!["example"]);
    }
}
