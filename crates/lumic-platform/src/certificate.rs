//! Certificate providers and explicit nginx certificate attachment.

use crate::{
    ProcessRunner, ProcessSpec,
    atomic_file::{restore_backup, write_atomic},
    framework_state::{FrameworkState, FrameworkStateStore},
    resource_lock::ResourceLock,
    systemd::{ServiceAction, SystemdServiceManager},
};
use lumic_core::{
    LumicError, OperationContext, Result,
    binding::Binding,
    certificate::{
        CertificateAction, CertificateInspection, CertificatePlan, CertificatePlanStep,
        CertificatePreflight, CertificateRequest,
    },
    resource::{ResourceOutput, ResourceOutputs, ResourceRecord},
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

/// Semantic provider contract used by certificate orchestration.
#[allow(async_fn_in_trait)]
pub trait CertificateProvider: Send + Sync {
    fn id(&self) -> &'static str;
    fn plan(&self, request: &CertificateRequest, action: CertificateAction) -> CertificatePlan;
    async fn preflight(&self, request: &CertificateRequest) -> Result<CertificatePreflight>;
    async fn issue(&self, request: &CertificateRequest) -> Result<CertificateInspection>;
    async fn inspect(&self, request: &CertificateRequest) -> Result<CertificateInspection>;
    async fn renew(&self, request: &CertificateRequest) -> Result<CertificateInspection>;
    async fn detach(&self, request: &CertificateRequest) -> Result<()>;
}

/// Consumer adapter contract kept separate from certificate issuance.
#[allow(async_fn_in_trait)]
pub trait CertificateAttacher: Send + Sync {
    async fn attach(
        &self,
        configuration_path: &Path,
        certificate: &CertificateInspection,
        context: &OperationContext,
    ) -> Result<AttachmentResult>;
    async fn detach(
        &self,
        configuration_path: &Path,
        previous_configuration: &str,
        context: &OperationContext,
    ) -> Result<()>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttachmentResult {
    pub previous_configuration: String,
    pub changed: bool,
}

/// Certbot implementation for Let's Encrypt certificates.
#[derive(Debug, Clone, Copy, Default)]
pub struct CertbotProvider {
    runner: ProcessRunner,
}

impl CertificateProvider for CertbotProvider {
    fn id(&self) -> &'static str {
        "certbot-letsencrypt"
    }

    fn plan(&self, request: &CertificateRequest, action: CertificateAction) -> CertificatePlan {
        certificate_plan(request, action)
    }

    async fn preflight(&self, request: &CertificateRequest) -> Result<CertificatePreflight> {
        request.validate()?;
        let certbot = self
            .run(ProcessSpec::new("certbot").args(["--version"]))
            .await?;
        let nginx = self.run(ProcessSpec::new("nginx").args(["-t"])).await?;
        Ok(CertificatePreflight {
            provider_available: certbot.success(),
            web_server_valid: nginx.success(),
            details: vec![
                process_evidence("certbot --version", &certbot),
                process_evidence("nginx -t", &nginx),
            ],
        })
    }

    async fn issue(&self, request: &CertificateRequest) -> Result<CertificateInspection> {
        request.validate()?;
        let mut args = vec![
            "certonly".to_owned(),
            "--nginx".to_owned(),
            "--non-interactive".to_owned(),
            "--agree-tos".to_owned(),
            "--keep-until-expiring".to_owned(),
            "--email".to_owned(),
            request.contact_email.clone(),
            "--cert-name".to_owned(),
            request.certificate_name.clone(),
        ];
        for domain in &request.domains {
            args.extend(["-d".to_owned(), domain.clone()]);
        }
        let mut spec = ProcessSpec::new("certbot").args(args);
        spec.timeout = Duration::from_secs(300);
        self.run_checked(spec).await?;
        self.inspect(request).await
    }

    async fn inspect(&self, request: &CertificateRequest) -> Result<CertificateInspection> {
        request.validate()?;
        let output = self
            .run_checked(ProcessSpec::new("certbot").args([
                "certificates",
                "--cert-name",
                &request.certificate_name,
            ]))
            .await?;
        let base = PathBuf::from("/etc/letsencrypt/live").join(&request.certificate_name);
        let fullchain = base.join("fullchain.pem");
        let private_key = base.join("privkey.pem");
        let inspection = CertificateInspection {
            provider: self.id().into(),
            certificate_name: request.certificate_name.clone(),
            domains: request.domains.clone(),
            fullchain_path: fullchain.to_string_lossy().into(),
            private_key_path: private_key.to_string_lossy().into(),
            present: fullchain.exists()
                && private_key.exists()
                && String::from_utf8_lossy(&output.stdout).contains(&request.certificate_name),
            renewable: true,
            expires_at_unix_seconds: None,
        };
        inspection.validate()?;
        if !inspection.present {
            return Err(LumicError::Inspection {
                fact: "certificate".into(),
                message: format!(
                    "Certbot did not report certificate '{}' with readable live paths",
                    request.certificate_name
                ),
            });
        }
        Ok(inspection)
    }

    async fn renew(&self, request: &CertificateRequest) -> Result<CertificateInspection> {
        request.validate()?;
        let mut spec = ProcessSpec::new("certbot").args([
            "renew",
            "--non-interactive",
            "--cert-name",
            &request.certificate_name,
        ]);
        spec.timeout = Duration::from_secs(300);
        self.run_checked(spec).await?;
        self.inspect(request).await
    }

    async fn detach(&self, request: &CertificateRequest) -> Result<()> {
        request.validate()?;
        self.run_checked(ProcessSpec::new("certbot").args([
            "delete",
            "--non-interactive",
            "--cert-name",
            &request.certificate_name,
        ]))
        .await?;
        Ok(())
    }
}

impl CertbotProvider {
    async fn run(&self, spec: ProcessSpec) -> Result<crate::ProcessOutput> {
        self.runner.run(&spec).await
    }

    async fn run_checked(&self, spec: ProcessSpec) -> Result<crate::ProcessOutput> {
        let executable = spec.executable.clone();
        let output = self.run(spec).await?;
        if output.success() {
            Ok(output)
        } else {
            Err(LumicError::Process {
                executable,
                message: String::from_utf8_lossy(&output.stderr).trim().into(),
            })
        }
    }
}

#[derive(Debug, Clone)]
pub struct NginxCertificateAttacher {
    state_dir: PathBuf,
    runner: ProcessRunner,
}

impl NginxCertificateAttacher {
    pub fn new(state_dir: impl Into<PathBuf>) -> Self {
        Self {
            state_dir: state_dir.into(),
            runner: ProcessRunner,
        }
    }

    async fn validate_and_reload(&self, context: &OperationContext) -> Result<()> {
        let validation = self
            .runner
            .run(&ProcessSpec::new("nginx").args(["-t"]))
            .await?;
        if !validation.success() {
            return Err(LumicError::Process {
                executable: "nginx".into(),
                message: String::from_utf8_lossy(&validation.stderr).trim().into(),
            });
        }
        SystemdServiceManager::at_state_dir(&self.state_dir)
            .apply("nginx.service", ServiceAction::Reload, context)
            .await?;
        Ok(())
    }
}

impl CertificateAttacher for NginxCertificateAttacher {
    async fn attach(
        &self,
        configuration_path: &Path,
        certificate: &CertificateInspection,
        context: &OperationContext,
    ) -> Result<AttachmentResult> {
        certificate.validate()?;
        let previous_configuration = fs::read_to_string(configuration_path).map_err(nginx_io)?;
        let rendered = render_tls_configuration(&previous_configuration, certificate)?;
        let write = write_atomic(configuration_path, rendered.as_bytes(), 0o644)?;
        if let Err(error) = self.validate_and_reload(context).await {
            restore_write(configuration_path, &write)?;
            let _ = self.validate_and_reload(context).await;
            return Err(error);
        }
        Ok(AttachmentResult {
            previous_configuration,
            changed: write.changed,
        })
    }

    async fn detach(
        &self,
        configuration_path: &Path,
        previous_configuration: &str,
        context: &OperationContext,
    ) -> Result<()> {
        let current = fs::read_to_string(configuration_path).map_err(nginx_io)?;
        write_atomic(configuration_path, previous_configuration.as_bytes(), 0o644)?;
        if let Err(error) = self.validate_and_reload(context).await {
            write_atomic(configuration_path, current.as_bytes(), 0o644)?;
            let _ = self.validate_and_reload(context).await;
            return Err(error);
        }
        Ok(())
    }
}

/// Coordinates provider operations, nginx attachment, locks, and resource state.
#[derive(Debug, Clone)]
pub struct CertificateManager<P, A> {
    state_dir: PathBuf,
    provider: P,
    attacher: A,
}

impl<P, A> CertificateManager<P, A>
where
    P: CertificateProvider,
    A: CertificateAttacher,
{
    pub fn new(state_dir: impl Into<PathBuf>, provider: P, attacher: A) -> Self {
        Self {
            state_dir: state_dir.into(),
            provider,
            attacher,
        }
    }

    pub fn plan(
        &self,
        request: &CertificateRequest,
        action: CertificateAction,
    ) -> Result<CertificatePlan> {
        request.validate()?;
        if request.provider != self.provider.id() {
            return Err(invalid(
                "certificate.provider",
                "provider is not registered",
            ));
        }
        Ok(self.provider.plan(request, action))
    }

    pub async fn preflight(&self, request: &CertificateRequest) -> Result<CertificatePreflight> {
        self.consumer_configuration(request)?;
        self.provider.preflight(request).await
    }

    pub async fn issue(
        &self,
        request: &CertificateRequest,
        context: &OperationContext,
    ) -> Result<CertificateInspection> {
        request.validate()?;
        let configuration_path = self.consumer_configuration(request)?;
        let _certificate_lock = ResourceLock::try_acquire(&self.state_dir, &request.resource)?;
        let _nginx_lock = ResourceLock::try_acquire_nginx(&self.state_dir)?;
        let existing_previous = self.previous_configuration(request).ok();
        self.require_preflight(request).await?;
        let certificate = self.provider.issue(request).await?;
        let attachment = match self
            .attacher
            .attach(&configuration_path, &certificate, context)
            .await
        {
            Ok(attachment) => attachment,
            Err(error) => {
                if existing_previous.is_none() {
                    let _ = self.provider.detach(request).await;
                }
                return Err(error);
            }
        };
        let previous_configuration = existing_previous
            .as_deref()
            .unwrap_or(&attachment.previous_configuration);
        if let Err(error) = self.persist(
            request,
            &certificate,
            &configuration_path,
            previous_configuration,
        ) {
            let _ = self
                .attacher
                .detach(
                    &configuration_path,
                    &attachment.previous_configuration,
                    context,
                )
                .await;
            if existing_previous.is_none() {
                let _ = self.provider.detach(request).await;
            }
            return Err(error);
        }
        Ok(certificate)
    }

    pub async fn inspect(&self, request: &CertificateRequest) -> Result<CertificateInspection> {
        self.provider.inspect(request).await
    }

    pub async fn renew(
        &self,
        request: &CertificateRequest,
        context: &OperationContext,
    ) -> Result<CertificateInspection> {
        request.validate()?;
        let configuration_path = self.consumer_configuration(request)?;
        let previous = self.previous_configuration(request)?;
        let _certificate_lock = ResourceLock::try_acquire(&self.state_dir, &request.resource)?;
        let _nginx_lock = ResourceLock::try_acquire_nginx(&self.state_dir)?;
        self.require_preflight(request).await?;
        let certificate = self.provider.renew(request).await?;
        self.attacher
            .attach(&configuration_path, &certificate, context)
            .await?;
        self.persist(request, &certificate, &configuration_path, &previous)?;
        Ok(certificate)
    }

    pub async fn detach(
        &self,
        request: &CertificateRequest,
        context: &OperationContext,
    ) -> Result<()> {
        request.validate()?;
        let configuration_path = self.consumer_configuration(request)?;
        let previous = self.previous_configuration(request)?;
        let _certificate_lock = ResourceLock::try_acquire(&self.state_dir, &request.resource)?;
        let _nginx_lock = ResourceLock::try_acquire_nginx(&self.state_dir)?;
        self.attacher
            .detach(&configuration_path, &previous, context)
            .await?;
        if let Err(error) = self.provider.detach(request).await {
            if let Ok(certificate) = self.provider.inspect(request).await {
                let _ = self
                    .attacher
                    .attach(&configuration_path, &certificate, context)
                    .await;
            }
            return Err(error);
        }
        self.remove_persisted(request)
    }

    fn consumer_configuration(&self, request: &CertificateRequest) -> Result<PathBuf> {
        let state = FrameworkStateStore::at_state_dir(&self.state_dir).load()?;
        let consumer = state
            .resources
            .iter()
            .find(|resource| resource.resource == request.consumer)
            .ok_or_else(|| invalid("certificate.consumer", "web-host resource does not exist"))?;
        let path = consumer
            .attributes
            .get("configuration_path")
            .and_then(Value::as_str)
            .ok_or_else(|| invalid("certificate.consumer", "web host has no configuration path"))?;
        let path = PathBuf::from(path);
        if !path.is_absolute() {
            return Err(invalid(
                "certificate.consumer.configuration_path",
                "must be absolute",
            ));
        }
        Ok(path)
    }

    async fn require_preflight(&self, request: &CertificateRequest) -> Result<()> {
        let preflight = self.provider.preflight(request).await?;
        if preflight.provider_available && preflight.web_server_valid {
            return Ok(());
        }
        Err(LumicError::Inspection {
            fact: "certificate_preflight".into(),
            message: preflight.details.join("; "),
        })
    }

    fn previous_configuration(&self, request: &CertificateRequest) -> Result<String> {
        let state = FrameworkStateStore::at_state_dir(&self.state_dir).load()?;
        state
            .resources
            .iter()
            .find(|resource| resource.resource == request.resource)
            .and_then(|resource| resource.attributes.get("previous_configuration"))
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| {
                invalid(
                    "certificate.resource",
                    "certificate attachment state does not exist",
                )
            })
    }

    fn persist(
        &self,
        request: &CertificateRequest,
        certificate: &CertificateInspection,
        configuration_path: &Path,
        previous_configuration: &str,
    ) -> Result<()> {
        let now = unix_time_ms()?;
        let store = FrameworkStateStore::at_state_dir(&self.state_dir);
        let mut state = store.load_or_migrate(now)?;
        let created = state
            .resources
            .iter()
            .find(|resource| resource.resource == request.resource)
            .map_or(now, |resource| resource.created_at_unix_ms);
        let record = ResourceRecord {
            resource: request.resource.clone(),
            attributes: BTreeMap::from([
                (
                    "provider".into(),
                    Value::String(certificate.provider.clone()),
                ),
                (
                    "certificate_name".into(),
                    Value::String(certificate.certificate_name.clone()),
                ),
                ("domains".into(), json!(certificate.domains)),
                (
                    "fullchain_path".into(),
                    Value::String(certificate.fullchain_path.clone()),
                ),
                (
                    "private_key_path".into(),
                    Value::String(certificate.private_key_path.clone()),
                ),
                (
                    "configuration_path".into(),
                    Value::String(configuration_path.to_string_lossy().into()),
                ),
                (
                    "previous_configuration".into(),
                    Value::String(previous_configuration.into()),
                ),
                ("attached".into(), Value::Bool(true)),
            ]),
            outputs: ResourceOutputs::from([(
                "tls".into(),
                ResourceOutput {
                    value: json!({
                        "kind": "tls_certificate",
                        "domains": certificate.domains,
                        "fullchain_path": certificate.fullchain_path,
                        "private_key_path": certificate.private_key_path,
                        "capability": "web.tls",
                    }),
                    sensitive: false,
                    updated_at_unix_ms: now,
                },
            )]),
            created_at_unix_ms: created,
            updated_at_unix_ms: now,
        };
        upsert(&mut state, record);
        state.bindings.0.retain(|binding| {
            binding.id != certificate_binding_id(request)
                && !(binding.consumer == request.consumer && binding.input == "certificate")
        });
        state.bindings.0.push(Binding {
            id: certificate_binding_id(request),
            producer: request.resource.clone(),
            output: "tls".into(),
            consumer: request.consumer.clone(),
            input: "certificate".into(),
            created_at_unix_ms: now,
        });
        store.save(&state)
    }

    fn remove_persisted(&self, request: &CertificateRequest) -> Result<()> {
        let store = FrameworkStateStore::at_state_dir(&self.state_dir);
        let mut state = store.load()?;
        state
            .bindings
            .0
            .retain(|binding| binding.producer != request.resource);
        state
            .resources
            .retain(|resource| resource.resource != request.resource);
        store.save(&state)
    }
}

fn certificate_plan(request: &CertificateRequest, action: CertificateAction) -> CertificatePlan {
    let provider_action = match action {
        CertificateAction::Issue => "request or reuse the named Let's Encrypt certificate",
        CertificateAction::Renew => "renew the named Let's Encrypt certificate when due",
        CertificateAction::Detach => "delete the detached named Certbot certificate",
    };
    let consumer_action = match action {
        CertificateAction::Issue => "attach the issued certificate, validate, and reload",
        CertificateAction::Renew => "validate the existing attachment and reload renewed material",
        CertificateAction::Detach => "restore the previous HTTP configuration and reload",
    };
    CertificatePlan {
        action,
        resource: request.resource.clone(),
        provider: request.provider.clone(),
        domains: request.domains.clone(),
        preconditions: vec![
            "the owned nginx web host exists and validates".into(),
            "Certbot and its nginx plugin are installed".into(),
            "the requested DNS names resolve to this host".into(),
        ],
        steps: vec![
            CertificatePlanStep {
                action: "provider".into(),
                target: request.certificate_name.clone(),
                description: provider_action.into(),
            },
            CertificatePlanStep {
                action: "nginx".into(),
                target: request.consumer.id.clone(),
                description: consumer_action.into(),
            },
            CertificatePlanStep {
                action: "state".into(),
                target: request.resource.id.clone(),
                description: "persist the certificate resource and binding".into(),
            },
        ],
    }
}

pub fn render_tls_configuration(
    http_configuration: &str,
    certificate: &CertificateInspection,
) -> Result<String> {
    certificate.validate()?;
    if http_configuration.contains("# Lumic TLS attachment") {
        return Ok(http_configuration.into());
    }
    if !http_configuration.starts_with("# Managed by Lumic.") {
        return Err(invalid(
            "nginx.configuration",
            "refusing to attach a certificate to an unmanaged configuration",
        ));
    }
    let tls_server = http_configuration
        .replace("    listen 80;", "    listen 443 ssl;")
        .replace("    listen [::]:80;", "    listen [::]:443 ssl;")
        .replacen(
            "server {\n",
            &format!(
                "server {{\n    ssl_certificate {};\n    ssl_certificate_key {};\n",
                certificate.fullchain_path, certificate.private_key_path
            ),
            1,
        );
    let names = server_names(http_configuration)?;
    Ok(format!(
        "# Managed by Lumic. Edit the application through Lumic, not this file.\n# Lumic TLS attachment. Detach through Lumic.\nserver {{\n    listen 80;\n    listen [::]:80;\n    server_name {names};\n    return 301 https://$host$request_uri;\n}}\n{tls_server}"
    ))
}

fn server_names(configuration: &str) -> Result<&str> {
    configuration
        .lines()
        .find_map(|line| {
            line.trim()
                .strip_prefix("server_name ")
                .and_then(|value| value.strip_suffix(';'))
        })
        .ok_or_else(|| invalid("nginx.configuration", "managed server_name is missing"))
}

fn restore_write(path: &Path, write: &crate::atomic_file::AtomicWriteResult) -> Result<()> {
    if let Some(backup) = &write.backup {
        restore_backup(path, backup)
    } else if write.changed {
        fs::remove_file(path).map_err(nginx_io)
    } else {
        Ok(())
    }
}

fn process_evidence(command: &str, output: &crate::ProcessOutput) -> String {
    format!(
        "{command}: {}",
        if output.success() { "ok" } else { "failed" }
    )
}

fn certificate_binding_id(request: &CertificateRequest) -> String {
    format!("{}-to-{}", request.resource.id, request.consumer.id)
}

fn upsert(state: &mut FrameworkState, record: ResourceRecord) {
    if let Some(existing) = state
        .resources
        .iter_mut()
        .find(|existing| existing.resource == record.resource)
    {
        *existing = record;
    } else {
        state.resources.push(record);
    }
}

fn unix_time_ms() -> Result<u64> {
    let value = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| LumicError::Internal {
            message: format!("system clock is before Unix epoch: {error}"),
        })?
        .as_millis();
    u64::try_from(value).map_err(|_| LumicError::Internal {
        message: "current time does not fit in resource state".into(),
    })
}

fn nginx_io(error: impl std::fmt::Display) -> LumicError {
    LumicError::Internal {
        message: format!("nginx certificate attachment failed: {error}"),
    }
}

fn invalid(field: &str, message: &str) -> LumicError {
    LumicError::InvalidInput {
        field: field.into(),
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumic_core::resource::{ResourceKind, ResourceRef};
    use std::sync::Mutex;

    #[derive(Debug, Default)]
    struct FakeCertificateProvider {
        calls: Mutex<Vec<CertificateAction>>,
    }

    impl CertificateProvider for FakeCertificateProvider {
        fn id(&self) -> &'static str {
            "certbot-letsencrypt"
        }

        fn plan(&self, request: &CertificateRequest, action: CertificateAction) -> CertificatePlan {
            certificate_plan(request, action)
        }

        async fn preflight(&self, _request: &CertificateRequest) -> Result<CertificatePreflight> {
            Ok(CertificatePreflight {
                provider_available: true,
                web_server_valid: true,
                details: vec!["fake provider ready".into()],
            })
        }

        async fn issue(&self, request: &CertificateRequest) -> Result<CertificateInspection> {
            self.calls.lock().unwrap().push(CertificateAction::Issue);
            Ok(inspection(request))
        }

        async fn inspect(&self, request: &CertificateRequest) -> Result<CertificateInspection> {
            Ok(inspection(request))
        }

        async fn renew(&self, request: &CertificateRequest) -> Result<CertificateInspection> {
            self.calls.lock().unwrap().push(CertificateAction::Renew);
            Ok(inspection(request))
        }

        async fn detach(&self, _request: &CertificateRequest) -> Result<()> {
            self.calls.lock().unwrap().push(CertificateAction::Detach);
            Ok(())
        }
    }

    #[derive(Debug, Default)]
    struct FakeAttacher;

    impl CertificateAttacher for FakeAttacher {
        async fn attach(
            &self,
            _configuration_path: &Path,
            _certificate: &CertificateInspection,
            _context: &OperationContext,
        ) -> Result<AttachmentResult> {
            Ok(AttachmentResult {
                previous_configuration: "# Managed by Lumic.\nserver {}\n".into(),
                changed: true,
            })
        }

        async fn detach(
            &self,
            _configuration_path: &Path,
            _previous_configuration: &str,
            _context: &OperationContext,
        ) -> Result<()> {
            Ok(())
        }
    }

    fn request() -> CertificateRequest {
        CertificateRequest {
            resource: ResourceRef::new(ResourceKind::Certificate, "certificate.demo").unwrap(),
            consumer: ResourceRef::new(ResourceKind::ServiceResource, "nginx.web-host.demo")
                .unwrap(),
            provider: "certbot-letsencrypt".into(),
            certificate_name: "demo.example.com".into(),
            domains: vec!["demo.example.com".into(), "www.demo.example.com".into()],
            contact_email: "ops@example.com".into(),
        }
    }

    fn inspection(request: &CertificateRequest) -> CertificateInspection {
        CertificateInspection {
            provider: request.provider.clone(),
            certificate_name: request.certificate_name.clone(),
            domains: request.domains.clone(),
            fullchain_path: "/etc/letsencrypt/live/demo.example.com/fullchain.pem".into(),
            private_key_path: "/etc/letsencrypt/live/demo.example.com/privkey.pem".into(),
            present: true,
            renewable: true,
            expires_at_unix_seconds: Some(2_000_000_000),
        }
    }

    fn context() -> OperationContext {
        OperationContext {
            actor: "test".into(),
            interface: lumic_core::OperationInterface::Internal,
            correlation_id: "phase-5".into(),
            dry_run: false,
            approved: true,
        }
    }

    fn fixture_state(state_dir: &Path) {
        let path = state_dir.join("lumic-demo.conf");
        fs::write(&path, "# Managed by Lumic.\nserver {}\n").unwrap();
        let now = 1;
        let resource = ResourceRecord {
            resource: request().consumer,
            attributes: BTreeMap::from([(
                "configuration_path".into(),
                Value::String(path.to_string_lossy().into()),
            )]),
            outputs: ResourceOutputs::from([(
                "http".into(),
                ResourceOutput {
                    value: json!({"url": "http://demo.example.com"}),
                    sensitive: false,
                    updated_at_unix_ms: now,
                },
            )]),
            created_at_unix_ms: now,
            updated_at_unix_ms: now,
        };
        let mut state = FrameworkState::default();
        state.resources.push(resource);
        FrameworkStateStore::at_state_dir(state_dir)
            .save(&state)
            .unwrap();
    }

    #[test]
    fn plans_are_read_only_and_do_not_expose_contact_email() {
        let provider = FakeCertificateProvider::default();
        let plan = provider.plan(&request(), CertificateAction::Issue);
        let serialized = serde_json::to_string(&plan).unwrap();
        assert!(!serialized.contains("ops@example.com"));
        assert_eq!(plan.steps.len(), 3);
    }

    #[test]
    fn renders_explicit_nginx_tls_attachment() {
        let certificate = inspection(&request());
        let rendered = render_tls_configuration(
            "# Managed by Lumic. Edit through Lumic.\nserver {\n    listen 80;\n    listen [::]:80;\n    server_name demo.example.com www.demo.example.com;\n}\n",
            &certificate,
        )
        .unwrap();
        assert!(rendered.contains("listen 443 ssl"));
        assert!(
            rendered
                .contains("ssl_certificate_key /etc/letsencrypt/live/demo.example.com/privkey.pem")
        );
        assert!(rendered.contains("return 301 https://$host$request_uri"));
        assert_eq!(
            render_tls_configuration(&rendered, &certificate).unwrap(),
            rendered
        );
    }

    #[tokio::test]
    async fn fake_provider_exercises_issue_renew_and_detach_deterministically() {
        let state_dir = std::env::temp_dir().join(format!(
            "lumic-certificate-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&state_dir).unwrap();
        fixture_state(&state_dir);
        let manager =
            CertificateManager::new(&state_dir, FakeCertificateProvider::default(), FakeAttacher);
        manager.issue(&request(), &context()).await.unwrap();
        let state = FrameworkStateStore::at_state_dir(&state_dir)
            .load()
            .unwrap();
        assert!(
            state
                .resources
                .iter()
                .any(|item| item.resource == request().resource)
        );
        assert!(
            state
                .bindings
                .0
                .iter()
                .any(|item| item.input == "certificate")
        );
        manager.renew(&request(), &context()).await.unwrap();
        manager.detach(&request(), &context()).await.unwrap();
        let state = FrameworkStateStore::at_state_dir(&state_dir)
            .load()
            .unwrap();
        assert!(
            !state
                .resources
                .iter()
                .any(|item| item.resource == request().resource)
        );
        fs::remove_dir_all(state_dir).unwrap();
    }
}
