use crate::{
    ProcessRunner, ProcessSpec,
    atomic_file::{restore_backup, write_atomic},
    systemd::{ServiceAction, SystemdServiceManager},
};
use lumic_core::{
    LumicError, OperationContext, Result,
    application::{Application, ApplicationRuntime},
};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
    time::Duration,
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

    pub async fn configure(
        &self,
        application: &Application,
        context: &OperationContext,
    ) -> Result<WebConfigurationResult> {
        let php_socket = if application.runtime == ApplicationRuntime::Php {
            Some(discover_php_fpm_socket()?)
        } else {
            None
        };
        let config = render_nginx_config(application, php_socket.as_deref())?;
        let path = self
            .available_dir
            .join(format!("lumic-{}.conf", application.id));
        let write = write_atomic(&path, config.as_bytes(), 0o644)?;
        fs::create_dir_all(&self.enabled_dir).map_err(io_error)?;
        let enabled = self
            .enabled_dir
            .join(format!("lumic-{}.conf", application.id));
        let enabled_created = !enabled.exists();
        if enabled_created {
            #[cfg(unix)]
            std::os::unix::fs::symlink(&path, &enabled).map_err(io_error)?;
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
        if let Err(error) = services
            .apply("nginx.service", ServiceAction::Reload, context)
            .await
        {
            self.restore_configuration(&path, &enabled, enabled_created, &write)?;
            if self.validate().await.is_ok() {
                let _ = services
                    .apply("nginx.service", ServiceAction::Reload, context)
                    .await;
            }
            return Err(error);
        }
        Ok(WebConfigurationResult {
            application_id: application.id.clone(),
            configuration_path: path.to_string_lossy().into(),
            changed: write.changed,
            validated: true,
            reloaded: true,
            backup_path: write.backup.map(|path| path.to_string_lossy().into()),
        })
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

#[derive(Debug, Clone, Copy, Default)]
pub struct TlsManager;

impl TlsManager {
    pub async fn enable(application: &Application, email: &str) -> Result<()> {
        if !valid_email(email) {
            return Err(LumicError::InvalidInput {
                field: "email".into(),
                message: "must be a valid non-empty certificate contact email".into(),
            });
        }
        if !application.web_configured {
            return Err(LumicError::InvalidInput {
                field: "application".into(),
                message: "configure and validate nginx before requesting TLS".into(),
            });
        }
        let mut args = vec![
            "--nginx",
            "--non-interactive",
            "--agree-tos",
            "--redirect",
            "--email",
            email,
            "-d",
            &application.domain,
        ];
        let www_domain;
        if application.www_alias {
            www_domain = format!("www.{}", application.domain);
            args.extend(["-d", &www_domain]);
        }
        let mut spec = ProcessSpec::new("certbot").args(args);
        spec.timeout = Duration::from_secs(300);
        let output = ProcessRunner.run(&spec).await?;
        if output.success() {
            Ok(())
        } else {
            Err(LumicError::Process {
                executable: "certbot".into(),
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
                message: "no PHP-FPM socket was detected under /run/php".into(),
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

fn discover_php_fpm_socket() -> Result<PathBuf> {
    fs::read_dir("/run/php")
        .map_err(io_error)?
        .filter_map(std::result::Result::ok)
        .map(|entry| entry.path())
        .find(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.ends_with("-fpm.sock"))
        })
        .ok_or_else(|| LumicError::Inspection {
            fact: "php_fpm_socket".into(),
            message: "PHP-FPM is installed but no versioned socket is available".into(),
        })
}

fn valid_email(email: &str) -> bool {
    !email.is_empty()
        && email.len() <= 254
        && !email.contains(['\n', '\r'])
        && email
            .split_once('@')
            .is_some_and(|(local, domain)| !local.is_empty() && domain.contains('.'))
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
    use std::collections::BTreeMap;

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
    fn certificate_email_is_validated() {
        assert!(valid_email("ops@example.com"));
        assert!(!valid_email("--register-unsafely-without-email"));
    }
}
