use crate::{
    apt::AptPackageManager, event_store::EventStore, framework_state::FrameworkStateStore,
};
use lumic_core::{
    LumicError, OperationContext, Result,
    application::ApplicationRuntime,
    package::{PackageMutation, PackageName},
    resource::{ResourceKind, ResourceOutput, ResourceOutputs, ResourceRecord, ResourceRef},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    time::{SystemTime, UNIX_EPOCH},
};

const SUPPORTED_PHP_VERSIONS: &[&str] = &["8.1", "8.2", "8.3", "8.4"];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeInstallResult {
    pub runtime: ApplicationRuntime,
    pub runtime_version: Option<String>,
    pub runtime_resource_id: Option<String>,
    pub components: Vec<String>,
    pub packages: Vec<PackageMutation>,
    pub fpm_socket: Option<String>,
}

#[derive(Debug, Clone)]
pub struct RuntimeManager {
    packages: AptPackageManager,
    state: FrameworkStateStore,
}

impl RuntimeManager {
    pub fn at_state_dir(state_dir: impl AsRef<std::path::Path>) -> Self {
        Self {
            packages: AptPackageManager::system(EventStore::at_state_dir(&state_dir)),
            state: FrameworkStateStore::at_state_dir(state_dir),
        }
    }

    pub async fn install(
        &self,
        runtime: ApplicationRuntime,
        components: &[String],
        context: &OperationContext,
    ) -> Result<RuntimeInstallResult> {
        self.install_versioned(runtime, None, components, context)
            .await
    }

    pub async fn install_versioned(
        &self,
        runtime: ApplicationRuntime,
        requested_version: Option<&str>,
        components: &[String],
        context: &OperationContext,
    ) -> Result<RuntimeInstallResult> {
        let version = self.validate_request(runtime, requested_version, components)?;
        let mut names = match runtime {
            ApplicationRuntime::Static => Vec::new(),
            ApplicationRuntime::Php => {
                let version = version.as_deref().expect("PHP always has a version");
                vec![
                    format!("php{version}-fpm"),
                    format!("php{version}-cli"),
                    "composer".into(),
                ]
            }
            ApplicationRuntime::Node => vec!["nodejs".into()],
        };
        for component in components {
            names.push(component_package(runtime, version.as_deref(), component)?);
        }
        names.sort_unstable();
        names.dedup();
        let mut packages = Vec::new();
        for name in names {
            packages.push(
                self.packages
                    .install(&PackageName::parse(name)?, context)
                    .await?,
            );
        }

        let now = unix_time_ms()?;
        let runtime_resource_id = version.as_deref().map(|version| format!("php.{version}"));
        let fpm_socket = version
            .as_deref()
            .map(|version| format!("/run/php/php{version}-fpm.sock"));
        if let Some(resource_id) = &runtime_resource_id {
            self.persist_php_runtime(resource_id, version.as_deref().unwrap(), components, now)?;
        }

        Ok(RuntimeInstallResult {
            runtime,
            runtime_version: version,
            runtime_resource_id,
            components: components.to_vec(),
            packages,
            fpm_socket,
        })
    }

    /// Validates a runtime selection and every component before any package mutation.
    pub fn validate_request(
        &self,
        runtime: ApplicationRuntime,
        requested_version: Option<&str>,
        components: &[String],
    ) -> Result<Option<String>> {
        let version = runtime_version(runtime, requested_version)?;
        for component in components {
            component_package(runtime, version.as_deref(), component)?;
        }
        Ok(version)
    }

    fn persist_php_runtime(
        &self,
        resource_id: &str,
        version: &str,
        components: &[String],
        now: u64,
    ) -> Result<()> {
        let mut state = self.state.load_or_migrate(now)?;
        let runtime_ref = ResourceRef::new(ResourceKind::Runtime, resource_id)?;
        let outputs = ResourceOutputs::from([
            (
                "fpm".into(),
                ResourceOutput {
                    value: json!({
                        "kind": "unix_socket_endpoint",
                        "path": format!("/run/php/php{version}-fpm.sock"),
                        "unit": format!("php{version}-fpm.service"),
                        "capability": "runtime.php_fpm",
                    }),
                    sensitive: false,
                    updated_at_unix_ms: now,
                },
            ),
            (
                "cli".into(),
                ResourceOutput {
                    value: json!({
                        "kind": "executable",
                        "path": format!("/usr/bin/php{version}"),
                        "capability": "runtime.php_cli",
                    }),
                    sensitive: false,
                    updated_at_unix_ms: now,
                },
            ),
        ]);
        upsert_resource(
            &mut state.resources,
            ResourceRecord {
                resource: runtime_ref.clone(),
                attributes: BTreeMap::from([
                    ("definition_id".into(), Value::String("php".into())),
                    ("version".into(), Value::String(version.into())),
                    ("ownership".into(), Value::String("lumic".into())),
                ]),
                outputs,
                created_at_unix_ms: now,
                updated_at_unix_ms: now,
            },
        );
        state.resources.retain(|resource| {
            resource
                .attributes
                .get("runtime_resource_id")
                .and_then(Value::as_str)
                != Some(resource_id)
                || resource.resource.kind != ResourceKind::Component
        });
        for component in components {
            state.resources.push(ResourceRecord {
                resource: ResourceRef::new(
                    ResourceKind::Component,
                    format!("php.{version}.{component}"),
                )?,
                attributes: BTreeMap::from([
                    ("definition_id".into(), Value::String(component.clone())),
                    (
                        "runtime_resource_id".into(),
                        Value::String(resource_id.into()),
                    ),
                    (
                        "package".into(),
                        Value::String(component_package(
                            ApplicationRuntime::Php,
                            Some(version),
                            component,
                        )?),
                    ),
                    ("ownership".into(), Value::String("lumic".into())),
                ]),
                outputs: ResourceOutputs::new(),
                created_at_unix_ms: now,
                updated_at_unix_ms: now,
            });
        }
        self.state.save(&state)
    }
}

fn upsert_resource(resources: &mut Vec<ResourceRecord>, mut record: ResourceRecord) {
    if let Some(existing) = resources
        .iter_mut()
        .find(|existing| existing.resource == record.resource)
    {
        record.created_at_unix_ms = existing.created_at_unix_ms;
        *existing = record;
    } else {
        resources.push(record);
    }
}

fn runtime_version(runtime: ApplicationRuntime, requested: Option<&str>) -> Result<Option<String>> {
    match runtime {
        ApplicationRuntime::Php => {
            let version = requested.ok_or_else(|| LumicError::InvalidInput {
                field: "runtime_version".into(),
                message: format!(
                    "PHP requires an explicit version; supported versions: {}",
                    SUPPORTED_PHP_VERSIONS.join(", ")
                ),
            })?;
            if SUPPORTED_PHP_VERSIONS.contains(&version) {
                Ok(Some(version.into()))
            } else {
                Err(LumicError::InvalidInput {
                    field: "runtime_version".into(),
                    message: format!(
                        "unsupported PHP version {version}; supported versions: {}",
                        SUPPORTED_PHP_VERSIONS.join(", ")
                    ),
                })
            }
        }
        _ if requested.is_some() => Err(LumicError::InvalidInput {
            field: "runtime_version".into(),
            message: "runtime versions are currently supported only for PHP".into(),
        }),
        _ => Ok(None),
    }
}

fn component_package(
    runtime: ApplicationRuntime,
    version: Option<&str>,
    component: &str,
) -> Result<String> {
    let trusted = matches!(
        component,
        "curl" | "intl" | "mbstring" | "mysql" | "xml" | "zip"
    );
    match (runtime, version, trusted) {
        (ApplicationRuntime::Php, Some(version), true) => Ok(format!("php{version}-{component}")),
        _ => Err(LumicError::InvalidInput {
            field: "component".into(),
            message: format!("{component} is not in the trusted catalog for {runtime:?}"),
        }),
    }
}

fn unix_time_ms() -> Result<u64> {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| LumicError::Internal {
            message: format!("system clock is before Unix epoch: {error}"),
        })?
        .as_millis();
    u64::try_from(millis).map_err(|_| LumicError::Internal {
        message: "current time does not fit in the resource state format".into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, process};

    fn test_state_dir(name: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!(
            "lumic-runtime-{name}-{}-{}",
            process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn component_catalog_is_explicit_and_versioned() {
        assert_eq!(
            component_package(ApplicationRuntime::Php, Some("8.4"), "intl").unwrap(),
            "php8.4-intl"
        );
        assert!(component_package(ApplicationRuntime::Node, None, "npm-shell-plugin").is_err());
    }

    #[test]
    fn php_versions_are_allowlisted() {
        assert!(runtime_version(ApplicationRuntime::Php, None).is_err());
        assert_eq!(
            runtime_version(ApplicationRuntime::Php, Some("8.1")).unwrap(),
            Some("8.1".into())
        );
        assert!(runtime_version(ApplicationRuntime::Php, Some("8.3; reboot")).is_err());
        assert!(runtime_version(ApplicationRuntime::Static, Some("8.3")).is_err());
    }

    #[test]
    fn php_runtime_publishes_fpm_and_cli_outputs_with_owned_components() {
        let state_dir = test_state_dir("outputs");
        let manager = RuntimeManager::at_state_dir(&state_dir);
        manager
            .persist_php_runtime("php.8.4", "8.4", &["intl".into()], 10)
            .unwrap();

        let state = FrameworkStateStore::at_state_dir(&state_dir)
            .load()
            .unwrap();
        let runtime = state
            .resources
            .iter()
            .find(|resource| resource.resource.id == "php.8.4")
            .unwrap();
        assert_eq!(
            runtime.outputs["fpm"].value["path"],
            "/run/php/php8.4-fpm.sock"
        );
        assert_eq!(runtime.outputs["cli"].value["path"], "/usr/bin/php8.4");
        assert!(state.resources.iter().any(|resource| {
            resource.resource.id == "php.8.4.intl"
                && resource.attributes["package"] == "php8.4-intl"
        }));
        fs::remove_dir_all(state_dir).unwrap();
    }
}
