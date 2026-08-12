use crate::{
    ProcessRunner, ProcessSpec, apt::AptPackageManager, event_store::EventStore,
    framework_state::FrameworkStateStore,
};
use lumic_core::{
    LumicError, OperationContext, Result,
    application::{ApplicationRuntime, ApplicationRuntimeIntent, NodePackageManager},
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
const SUPPORTED_NODE_VERSIONS: &[&str] = &["20", "22", "24"];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeInstallResult {
    pub runtime: ApplicationRuntime,
    pub runtime_version: Option<String>,
    pub runtime_resource_id: Option<String>,
    pub components: Vec<String>,
    pub packages: Vec<PackageMutation>,
    pub fpm_socket: Option<String>,
    pub package_manager: Option<NodePackageManager>,
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
        let runtime_resource_id = match (runtime, version.as_deref()) {
            (ApplicationRuntime::Php, Some(version)) => Some(format!("php.{version}")),
            (ApplicationRuntime::Node, Some(version)) => Some(format!("node.{version}")),
            _ => None,
        };
        let fpm_socket = match (runtime, version.as_deref()) {
            (ApplicationRuntime::Php, Some(version)) => {
                Some(format!("/run/php/php{version}-fpm.sock"))
            }
            _ => None,
        };
        if let (ApplicationRuntime::Php, Some(resource_id), Some(version)) =
            (runtime, runtime_resource_id.as_deref(), version.as_deref())
        {
            self.persist_php_runtime(resource_id, version, components, now)?;
        } else if let (ApplicationRuntime::Node, Some(resource_id), Some(version)) =
            (runtime, runtime_resource_id.as_deref(), version.as_deref())
        {
            self.persist_node_runtime(resource_id, version, now)?;
        }

        Ok(RuntimeInstallResult {
            runtime,
            runtime_version: version,
            runtime_resource_id,
            components: components.to_vec(),
            packages,
            fpm_socket,
            package_manager: None,
        })
    }

    /// Installs every trusted package required by manifest runtime intent and then proves the
    /// host executables satisfy the requested version, extensions, and package manager.
    pub async fn reconcile_intent(
        &self,
        runtime: ApplicationRuntime,
        intent: &ApplicationRuntimeIntent,
        context: &OperationContext,
    ) -> Result<RuntimeInstallResult> {
        let mut result = self
            .install_versioned(
                runtime,
                intent.version.as_deref(),
                &intent.components,
                context,
            )
            .await?;
        if let Some(manager) = intent.package_manager {
            if runtime != ApplicationRuntime::Node {
                return Err(invalid_runtime(
                    "package_manager",
                    "package managers are valid only for Node runtime",
                ));
            }
            if manager == NodePackageManager::Npm {
                result.packages.push(
                    self.packages
                        .install(&PackageName::parse("npm")?, context)
                        .await?,
                );
            } else {
                let output = ProcessRunner
                    .run(&ProcessSpec::new("corepack").args(["enable", manager.as_str()]))
                    .await?;
                ensure_success("corepack", &output)?;
            }
            result.package_manager = Some(manager);
        }
        self.verify_intent(runtime, intent).await?;
        Ok(result)
    }

    /// Read-only host verification used at deployment time. Deployment never changes runtimes.
    pub async fn verify_intent(
        &self,
        runtime: ApplicationRuntime,
        intent: &ApplicationRuntimeIntent,
    ) -> Result<()> {
        match runtime {
            ApplicationRuntime::Static => return Ok(()),
            ApplicationRuntime::Node => {
                let version = command_stdout("node", &["--version"]).await?;
                ensure_version(
                    "Node",
                    intent.version.as_deref(),
                    version.trim_start_matches('v'),
                )?;
            }
            ApplicationRuntime::Php => {
                let requested = intent.version.as_deref().ok_or_else(|| {
                    invalid_runtime("runtime_version", "PHP requires an explicit version")
                })?;
                let executable = format!("php{requested}");
                let version = command_stdout(&executable, &["-r", "echo PHP_VERSION;"]).await?;
                ensure_version("PHP", Some(requested), &version)?;
                let modules = command_stdout(&executable, &["-m"]).await?;
                let installed = modules
                    .lines()
                    .map(|module| module.trim().to_ascii_lowercase())
                    .collect::<std::collections::BTreeSet<_>>();
                for component in &intent.components {
                    if !php_component_loaded(&installed, component) {
                        return Err(invalid_runtime(
                            "runtime_components",
                            &format!("PHP extension {component} is not loaded by {executable}"),
                        ));
                    }
                }
            }
        }
        if let Some(manager) = intent.package_manager {
            command_stdout(manager.as_str(), &["--version"]).await?;
        }
        Ok(())
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

    fn persist_node_runtime(&self, resource_id: &str, version: &str, now: u64) -> Result<()> {
        let mut state = self.state.load_or_migrate(now)?;
        upsert_resource(
            &mut state.resources,
            ResourceRecord {
                resource: ResourceRef::new(ResourceKind::Runtime, resource_id)?,
                attributes: BTreeMap::from([
                    ("definition_id".into(), Value::String("node".into())),
                    ("version".into(), Value::String(version.into())),
                    ("ownership".into(), Value::String("lumic".into())),
                ]),
                outputs: ResourceOutputs::from([(
                    "cli".into(),
                    ResourceOutput {
                        value: json!({
                            "kind": "executable",
                            "path": "/usr/bin/node",
                            "capability": "runtime.node",
                        }),
                        sensitive: false,
                        updated_at_unix_ms: now,
                    },
                )]),
                created_at_unix_ms: now,
                updated_at_unix_ms: now,
            },
        );
        self.state.save(&state)
    }
}

fn php_component_loaded(installed: &std::collections::BTreeSet<String>, component: &str) -> bool {
    match component {
        "mysql" => ["mysqli", "pdo_mysql", "mysqlnd"]
            .iter()
            .any(|module| installed.contains(*module)),
        "xml" => ["xml", "libxml", "simplexml"]
            .iter()
            .any(|module| installed.contains(*module)),
        value => installed.contains(&value.to_ascii_lowercase()),
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
        ApplicationRuntime::Node => {
            let version = requested.ok_or_else(|| {
                invalid_runtime(
                    "runtime_version",
                    "Node requires an explicit supported major version",
                )
            })?;
            if SUPPORTED_NODE_VERSIONS.contains(&version) {
                Ok(Some(version.into()))
            } else {
                Err(invalid_runtime(
                    "runtime_version",
                    &format!(
                        "unsupported Node version {version}; supported majors: {}",
                        SUPPORTED_NODE_VERSIONS.join(", ")
                    ),
                ))
            }
        }
        ApplicationRuntime::Static if requested.is_some() => Err(invalid_runtime(
            "runtime_version",
            "static applications cannot declare a runtime version",
        )),
        _ => Ok(None),
    }
}

async fn command_stdout(executable: &str, args: &[&str]) -> Result<String> {
    let output = ProcessRunner
        .run(&ProcessSpec::new(executable).args(args.iter().copied()))
        .await?;
    ensure_success(executable, &output)?;
    String::from_utf8(output.stdout).map_err(|_| {
        invalid_runtime(
            "runtime",
            &format!("{executable} returned non-UTF-8 output"),
        )
    })
}

fn ensure_success(executable: &str, output: &crate::ProcessOutput) -> Result<()> {
    if output.success() {
        Ok(())
    } else {
        Err(invalid_runtime(
            "runtime",
            &format!(
                "{executable} runtime verification failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ),
        ))
    }
}

fn ensure_version(runtime: &str, requested: Option<&str>, detected: &str) -> Result<()> {
    if requested.is_some_and(|requested| {
        detected != requested && !detected.starts_with(&format!("{requested}."))
    }) {
        return Err(invalid_runtime(
            "runtime_version",
            &format!("requested {runtime} {requested:?}, but detected {detected}"),
        ));
    }
    Ok(())
}

fn invalid_runtime(field: &str, message: &str) -> LumicError {
    LumicError::InvalidInput {
        field: field.into(),
        message: message.into(),
    }
}

fn component_package(
    runtime: ApplicationRuntime,
    version: Option<&str>,
    component: &str,
) -> Result<String> {
    let package_component = match component {
        "curl" | "intl" | "mbstring" | "mysql" | "xml" | "zip" => Some(component),
        "mysqli" | "pdo_mysql" => Some("mysql"),
        "dom" => Some("xml"),
        "exif" | "fileinfo" | "openssl" => Some("fpm"),
        _ => None,
    };
    match (runtime, version, package_component) {
        (ApplicationRuntime::Php, Some(version), Some(package_component)) => {
            Ok(format!("php{version}-{package_component}"))
        }
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
        assert_eq!(
            runtime_version(ApplicationRuntime::Node, Some("22")).unwrap(),
            Some("22".into())
        );
        assert!(runtime_version(ApplicationRuntime::Node, None).is_err());
        assert!(runtime_version(ApplicationRuntime::Node, Some("23")).is_err());
    }

    #[test]
    fn php_component_verification_accepts_extension_aliases() {
        let modules =
            std::collections::BTreeSet::from(["libxml".to_string(), "pdo_mysql".to_string()]);
        assert!(php_component_loaded(&modules, "xml"));
        assert!(php_component_loaded(&modules, "mysql"));
        assert!(!php_component_loaded(&modules, "intl"));
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

    #[test]
    fn node_runtime_is_persisted_as_node_without_php_outputs() {
        let state_dir = test_state_dir("node-outputs");
        let manager = RuntimeManager::at_state_dir(&state_dir);
        manager.persist_node_runtime("node.22", "22", 10).unwrap();

        let state = FrameworkStateStore::at_state_dir(&state_dir)
            .load()
            .unwrap();
        let runtime = state
            .resources
            .iter()
            .find(|resource| resource.resource.id == "node.22")
            .unwrap();
        assert_eq!(runtime.attributes["definition_id"], "node");
        assert_eq!(runtime.outputs["cli"].value["path"], "/usr/bin/node");
        assert!(!runtime.outputs.contains_key("fpm"));
        fs::remove_dir_all(state_dir).unwrap();
    }
}
