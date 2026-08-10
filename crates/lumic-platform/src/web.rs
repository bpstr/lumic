use crate::{
    ProcessRunner, ProcessSpec,
    apt::AptPackageManager,
    atomic_file::{restore_backup, write_atomic},
    event_store::EventStore,
    framework_state::FrameworkStateStore,
    resource_lock::ResourceLock,
    systemd::{ServiceAction, SystemdServiceManager},
};
use lumic_core::{
    LumicError, OperationContext, Result,
    application::{Application, ApplicationRuntime},
    binding::Binding,
    catalog::Configuration,
    package::PackageName,
    resource::{ResourceKind, ResourceOutput, ResourceOutputs, ResourceRecord, ResourceRef},
    service::{DesiredServiceState, ManagementStatus, ResourceOwnership, ServiceInstance},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebConfigurationResult {
    pub application_id: String,
    pub configuration_path: String,
    pub changed: bool,
    pub validated: bool,
    pub reloaded: bool,
    pub backup_path: Option<String>,
}

#[derive(Debug, Clone)]
pub struct NginxManager {
    state_dir: PathBuf,
    available_dir: PathBuf,
    enabled_dir: PathBuf,
    runner: ProcessRunner,
}

impl NginxManager {
    pub fn system(state_dir: impl Into<PathBuf>) -> Self {
        Self::new(
            state_dir,
            "/etc/nginx/sites-available",
            "/etc/nginx/sites-enabled",
        )
    }

    pub fn new(
        state_dir: impl Into<PathBuf>,
        available_dir: impl Into<PathBuf>,
        enabled_dir: impl Into<PathBuf>,
    ) -> Self {
        Self {
            state_dir: state_dir.into(),
            available_dir: available_dir.into(),
            enabled_dir: enabled_dir.into(),
            runner: ProcessRunner,
        }
    }

    /// Removes only the site files owned by Lumic. The shared nginx service and
    /// unrelated hosts remain untouched; a later nginx operation validates and reloads.
    pub fn remove_configuration(&self, application_id: &str) -> Result<bool> {
        lumic_core::application::validate_slug("application", application_id)?;
        let _nginx_lock = ResourceLock::try_acquire_nginx(&self.state_dir)?;
        let filename = format!("lumic-{application_id}.conf");
        let available = self.available_dir.join(&filename);
        let enabled = self.enabled_dir.join(filename);
        let mut changed = false;
        if enabled.symlink_metadata().is_ok() {
            fs::remove_file(&enabled).map_err(io_error)?;
            changed = true;
        }
        if available.is_file() {
            fs::remove_file(&available).map_err(io_error)?;
            changed = true;
        }
        Ok(changed)
    }

    pub async fn configure(
        &self,
        application: &Application,
        php_socket: Option<&Path>,
        runtime_resource_id: Option<&str>,
        context: &OperationContext,
    ) -> Result<WebConfigurationResult> {
        validate_runtime_binding(application, php_socket, runtime_resource_id)?;
        let _nginx_lock = ResourceLock::try_acquire_nginx(&self.state_dir)?;
        let config = render_nginx_config(application, php_socket)?;
        let path = self
            .available_dir
            .join(format!("lumic-{}.conf", application.id));
        let write = write_atomic(&path, config.as_bytes(), 0o644)?;
        if let Err(error) = fs::create_dir_all(&self.enabled_dir).map_err(io_error) {
            self.restore_configuration(&path, Path::new(""), false, &write)?;
            return Err(error);
        }
        let enabled = self
            .enabled_dir
            .join(format!("lumic-{}.conf", application.id));
        let enabled_created = !enabled.exists();
        if enabled_created {
            #[cfg(unix)]
            if let Err(error) = std::os::unix::fs::symlink(&path, &enabled).map_err(io_error) {
                self.restore_configuration(&path, &enabled, false, &write)?;
                return Err(error);
            }
            #[cfg(not(unix))]
            return Err(LumicError::UnsupportedPlatform {
                platform: "nginx site symlinks require Unix".into(),
            });
        }
        if let Err(error) = self.validate().await {
            self.restore_configuration(&path, &enabled, enabled_created, &write)?;
            return Err(error);
        }
        let services = SystemdServiceManager::at_state_dir(&self.state_dir);
        let status = match services.inspect("nginx.service").await {
            Ok(status) => status,
            Err(error) => {
                self.restore_configuration(&path, &enabled, enabled_created, &write)?;
                return Err(error);
            }
        };
        let enabled_by_lumic = !status.enabled;
        let enable_changed = if enabled_by_lumic {
            match services
                .apply("nginx.service", ServiceAction::Enable, context)
                .await
            {
                Ok(mutation) => mutation.changed,
                Err(error) => {
                    self.restore_configuration(&path, &enabled, enabled_created, &write)?;
                    return Err(error);
                }
            }
        } else {
            false
        };
        let service_action = if status.active_state == "active" {
            ServiceAction::Reload
        } else {
            ServiceAction::Start
        };
        let service_mutation = match services
            .apply("nginx.service", service_action, context)
            .await
        {
            Ok(mutation) => mutation,
            Err(error) => {
                self.restore_configuration(&path, &enabled, enabled_created, &write)?;
                if enabled_by_lumic {
                    let _ = services
                        .apply("nginx.service", ServiceAction::Disable, context)
                        .await;
                }
                if service_action == ServiceAction::Reload && self.validate().await.is_ok() {
                    let _ = services
                        .apply("nginx.service", ServiceAction::Reload, context)
                        .await;
                }
                return Err(error);
            }
        };
        let result = WebConfigurationResult {
            application_id: application.id.clone(),
            configuration_path: path.to_string_lossy().into(),
            changed: write.changed || enable_changed || service_mutation.changed,
            validated: true,
            reloaded: service_action == ServiceAction::Reload,
            backup_path: write
                .backup
                .as_ref()
                .map(|path| path.to_string_lossy().into()),
        };
        if let Err(error) = self.persist_web_host(application, &result, runtime_resource_id) {
            self.restore_configuration(&path, &enabled, enabled_created, &write)?;
            if enable_changed {
                let _ = services
                    .apply("nginx.service", ServiceAction::Disable, context)
                    .await;
            }
            if service_action == ServiceAction::Reload && self.validate().await.is_ok() {
                let _ = services
                    .apply("nginx.service", ServiceAction::Reload, context)
                    .await;
            } else if service_action == ServiceAction::Start {
                let _ = services
                    .apply("nginx.service", ServiceAction::Stop, context)
                    .await;
            }
            return Err(error);
        }
        Ok(result)
    }

    /// Installs and records nginx as its own catalog-backed managed service.
    pub async fn ensure_service(&self, context: &OperationContext) -> Result<ServiceInstance> {
        AptPackageManager::system(EventStore::at_state_dir(&self.state_dir))
            .install(&PackageName::parse("nginx")?, context)
            .await?;
        let now = unix_time_ms()?;
        let mut state = FrameworkStateStore::at_state_dir(&self.state_dir).load_or_migrate(now)?;
        let mut service = nginx_service(now);
        if let Some(existing) = state.services.iter_mut().find(|item| item.id == service.id) {
            service.created_at_unix_ms = existing.created_at_unix_ms;
            *existing = service.clone();
        } else {
            state.services.push(service.clone());
        }
        FrameworkStateStore::at_state_dir(&self.state_dir).save(&state)?;
        Ok(service)
    }

    fn persist_web_host(
        &self,
        application: &Application,
        result: &WebConfigurationResult,
        runtime_resource_id: Option<&str>,
    ) -> Result<()> {
        let now = unix_time_ms()?;
        let store = FrameworkStateStore::at_state_dir(&self.state_dir);
        let mut state = store.load_or_migrate(now)?;
        let application_ref = ResourceRef::new(ResourceKind::Application, &application.id)?;
        let web_host_ref = ResourceRef::new(
            ResourceKind::ServiceResource,
            format!("nginx.web-host.{}", application.id),
        )?;
        upsert_resource(
            &mut state.resources,
            ResourceRecord {
                resource: application_ref.clone(),
                attributes: BTreeMap::from([
                    ("domain".into(), Value::String(application.domain.clone())),
                    (
                        "runtime".into(),
                        Value::String(format!("{:?}", application.runtime).to_ascii_lowercase()),
                    ),
                ]),
                outputs: ResourceOutputs::new(),
                created_at_unix_ms: now,
                updated_at_unix_ms: now,
            },
        );
        upsert_resource(
            &mut state.resources,
            ResourceRecord {
                resource: web_host_ref.clone(),
                attributes: BTreeMap::from([
                    ("resource_type".into(), Value::String("web_host".into())),
                    (
                        "provider_service_id".into(),
                        Value::String("nginx.main".into()),
                    ),
                    (
                        "application_id".into(),
                        Value::String(application.id.clone()),
                    ),
                    ("ownership".into(), Value::String("lumic".into())),
                    (
                        "configuration_path".into(),
                        Value::String(result.configuration_path.clone()),
                    ),
                    ("validated".into(), Value::Bool(result.validated)),
                ]),
                outputs: ResourceOutputs::from([(
                    "http".into(),
                    ResourceOutput {
                        value: json!({
                            "kind": "http_endpoint",
                            "url": format!("http://{}", application.domain),
                            "capability": "web.http",
                        }),
                        sensitive: false,
                        updated_at_unix_ms: now,
                    },
                )]),
                created_at_unix_ms: now,
                updated_at_unix_ms: now,
            },
        );
        replace_binding(
            &mut state.bindings.0,
            Binding {
                id: format!("nginx-to-{}-web-host", application.id),
                producer: ResourceRef::new(ResourceKind::ManagedService, "nginx.main")?,
                output: "http".into(),
                consumer: web_host_ref.clone(),
                input: "service".into(),
                created_at_unix_ms: now,
            },
        );
        if let Some(runtime_id) = runtime_resource_id {
            replace_binding(
                &mut state.bindings.0,
                Binding {
                    id: format!("php-to-{}-web-host", application.id),
                    producer: ResourceRef::new(ResourceKind::Runtime, runtime_id)?,
                    output: "fpm".into(),
                    consumer: web_host_ref.clone(),
                    input: "runtime".into(),
                    created_at_unix_ms: now,
                },
            );
        }
        replace_binding(
            &mut state.bindings.0,
            Binding {
                id: format!("{}-web-host-to-application", application.id),
                producer: web_host_ref,
                output: "http".into(),
                consumer: application_ref,
                input: "web".into(),
                created_at_unix_ms: now,
            },
        );
        store.save(&state)
    }

    fn restore_configuration(
        &self,
        path: &Path,
        enabled: &Path,
        enabled_created: bool,
        write: &crate::atomic_file::AtomicWriteResult,
    ) -> Result<()> {
        if let Some(backup) = &write.backup {
            restore_backup(path, backup)?;
        } else if write.changed {
            let _ = fs::remove_file(path);
        }
        if enabled_created && enabled.symlink_metadata().is_ok() {
            let _ = fs::remove_file(enabled);
        }
        Ok(())
    }

    async fn validate(&self) -> Result<()> {
        let output = self
            .runner
            .run(&ProcessSpec::new("nginx").args(["-t"]))
            .await?;
        if output.success() {
            Ok(())
        } else {
            Err(LumicError::Process {
                executable: "nginx".into(),
                message: String::from_utf8_lossy(&output.stderr).trim().into(),
            })
        }
    }
}

pub fn render_nginx_config(application: &Application, php_socket: Option<&Path>) -> Result<String> {
    let aliases = if application.www_alias {
        format!(" www.{}", application.domain)
    } else {
        String::new()
    };
    let current = PathBuf::from(&application.root).join("current");
    let body = match application.runtime {
        ApplicationRuntime::Static => format!(
            "    root {};\n    index index.html;\n    location / {{ try_files $uri $uri/ =404; }}\n",
            current.display()
        ),
        ApplicationRuntime::Php => {
            let socket = php_socket.ok_or_else(|| LumicError::Inspection {
                fact: "php_fpm_socket".into(),
                message: "the selected PHP runtime did not publish an FPM socket".into(),
            })?;
            format!(
                "    root {};\n    index index.php;\n    location / {{ try_files $uri $uri/ /index.php?$query_string; }}\n    location ~ \\.php$ {{ include snippets/fastcgi-php.conf; fastcgi_pass unix:{}; }}\n",
                current.display(), socket.display()
            )
        }
        ApplicationRuntime::Node => "    location / { proxy_pass http://127.0.0.1:3000; proxy_set_header Host $host; proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for; }\n".into(),
    };
    Ok(format!(
        "# Managed by Lumic. Edit the application through Lumic, not this file.\nserver {{\n    listen 80;\n    listen [::]:80;\n    server_name {}{};\n{} }}\n",
        application.domain, aliases, body
    ))
}

fn validate_runtime_binding(
    application: &Application,
    php_socket: Option<&Path>,
    runtime_resource_id: Option<&str>,
) -> Result<()> {
    match application.runtime {
        ApplicationRuntime::Php if php_socket.is_none() || runtime_resource_id.is_none() => {
            Err(LumicError::InvalidInput {
                field: "runtime_binding".into(),
                message: "PHP web hosts require a selected runtime and its published FPM output"
                    .into(),
            })
        }
        ApplicationRuntime::Php => Ok(()),
        _ if php_socket.is_some() || runtime_resource_id.is_some() => {
            Err(LumicError::InvalidInput {
                field: "runtime_binding".into(),
                message: "only PHP web hosts accept an FPM runtime binding".into(),
            })
        }
        _ => Ok(()),
    }
}

fn nginx_service(now: u64) -> ServiceInstance {
    ServiceInstance {
        id: "nginx.main".into(),
        definition_id: "nginx".into(),
        definition_version: 1,
        display_name: "nginx".into(),
        ownership: ResourceOwnership::Lumic,
        management_status: ManagementStatus::Managed,
        desired_state: DesiredServiceState::Running,
        configuration: Configuration::new(),
        outputs: ResourceOutputs::from([(
            "http".into(),
            ResourceOutput {
                value: json!({
                    "kind": "http_endpoint",
                    "scheme": "http",
                    "port": 80,
                    "capability": "web.http",
                }),
                sensitive: false,
                updated_at_unix_ms: now,
            },
        )]),
        platform_metadata: Configuration::from([
            ("package".into(), Value::String("nginx".into())),
            ("unit".into(), Value::String("nginx.service".into())),
        ]),
        installed_version: None,
        created_at_unix_ms: now,
        updated_at_unix_ms: now,
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

fn replace_binding(bindings: &mut Vec<Binding>, binding: Binding) {
    bindings.retain(|existing| {
        existing.id != binding.id
            && !(existing.consumer == binding.consumer && existing.input == binding.input)
    });
    bindings.push(binding);
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

fn io_error(error: std::io::Error) -> LumicError {
    LumicError::Internal {
        message: format!("nginx configuration failed: {error}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumic_core::application::{HealthCheck, TlsState};
    use std::{collections::BTreeMap, process};

    fn test_state_dir(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "lumic-web-{name}-{}-{}",
            process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn application(runtime: ApplicationRuntime) -> Application {
        Application {
            id: "demo".into(),
            name: "demo".into(),
            domain: "demo.example.com".into(),
            www_alias: true,
            root: "/var/lib/lumic/apps/demo".into(),
            runtime,
            repository: None,
            environment_references: BTreeMap::new(),
            service_references: Vec::new(),
            health_check: HealthCheck::default(),
            processes: Vec::new(),
            web_configured: false,
            tls: TlsState::default(),
            release_retention: 5,
            health_status: "not_deployed".into(),
            created_at_unix_ms: 1,
            updated_at_unix_ms: 1,
        }
    }

    #[test]
    fn renders_static_and_php_sites_without_untrusted_directives() {
        let static_site =
            render_nginx_config(&application(ApplicationRuntime::Static), None).unwrap();
        assert!(static_site.contains("server_name demo.example.com www.demo.example.com"));
        assert!(static_site.contains("try_files"));
        let php = render_nginx_config(
            &application(ApplicationRuntime::Php),
            Some(Path::new("/run/php/php8.4-fpm.sock")),
        )
        .unwrap();
        assert!(php.contains("fastcgi_pass unix:/run/php/php8.4-fpm.sock"));
    }

    #[test]
    fn php_web_hosts_require_an_explicit_runtime_output() {
        let php = application(ApplicationRuntime::Php);
        assert!(validate_runtime_binding(&php, None, None).is_err());
        assert!(
            validate_runtime_binding(
                &php,
                Some(Path::new("/run/php/php8.3-fpm.sock")),
                Some("php.8.3")
            )
            .is_ok()
        );
    }

    #[test]
    fn restores_previous_configuration_and_removes_new_enablement() {
        let state_dir = test_state_dir("rollback");
        let available = state_dir.join("available");
        let enabled_dir = state_dir.join("enabled");
        fs::create_dir_all(&available).unwrap();
        fs::create_dir_all(&enabled_dir).unwrap();
        let path = available.join("lumic-demo.conf");
        fs::write(&path, "old").unwrap();
        let write = write_atomic(&path, b"new", 0o644).unwrap();
        let enabled = enabled_dir.join("lumic-demo.conf");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&path, &enabled).unwrap();

        NginxManager::new(&state_dir, &available, &enabled_dir)
            .restore_configuration(&path, &enabled, true, &write)
            .unwrap();

        assert_eq!(fs::read_to_string(path).unwrap(), "old");
        assert!(!enabled.exists());
        fs::remove_dir_all(state_dir).unwrap();
    }

    #[test]
    fn persists_owned_web_host_and_explicit_bindings() {
        let state_dir = test_state_dir("state");
        let store = FrameworkStateStore::at_state_dir(&state_dir);
        let mut state = crate::framework_state::FrameworkState::default();
        state.services.push(nginx_service(1));
        store.save(&state).unwrap();
        let manager = NginxManager::new(
            &state_dir,
            state_dir.join("available"),
            state_dir.join("enabled"),
        );
        manager
            .persist_web_host(
                &application(ApplicationRuntime::Static),
                &WebConfigurationResult {
                    application_id: "demo".into(),
                    configuration_path: "/etc/nginx/sites-available/lumic-demo.conf".into(),
                    changed: true,
                    validated: true,
                    reloaded: true,
                    backup_path: None,
                },
                None,
            )
            .unwrap();

        let state = store.load().unwrap();
        assert!(state.resources.iter().any(|resource| {
            resource.resource.kind == ResourceKind::ServiceResource
                && resource.resource.id == "nginx.web-host.demo"
                && resource.attributes["ownership"] == "lumic"
        }));
        assert_eq!(state.bindings.0.len(), 2);
        fs::remove_dir_all(state_dir).unwrap();
    }
}
