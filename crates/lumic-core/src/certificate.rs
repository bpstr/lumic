//! Provider-neutral certificate lifecycle contracts.

use crate::{
    LumicError, Result,
    resource::{ResourceKind, ResourceRef},
};
use serde::{Deserialize, Serialize};

/// A validated request for a certificate owned by Lumic.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CertificateRequest {
    pub resource: ResourceRef,
    pub consumer: ResourceRef,
    pub provider: String,
    pub certificate_name: String,
    pub domains: Vec<String>,
    pub contact_email: String,
}

impl CertificateRequest {
    pub fn validate(&self) -> Result<()> {
        self.resource.validate()?;
        self.consumer.validate()?;
        if self.resource.kind != ResourceKind::Certificate {
            return Err(invalid(
                "certificate.resource",
                "must reference a certificate resource",
            ));
        }
        if self.consumer.kind != ResourceKind::ServiceResource {
            return Err(invalid(
                "certificate.consumer",
                "must reference an owned service resource",
            ));
        }
        if self.provider != "certbot-letsencrypt" {
            return Err(invalid(
                "certificate.provider",
                "must identify a trusted registered certificate provider",
            ));
        }
        validate_dns_name("certificate.certificate_name", &self.certificate_name)?;
        if self.domains.is_empty() {
            return Err(invalid(
                "certificate.domains",
                "must contain at least one DNS name",
            ));
        }
        for domain in &self.domains {
            validate_dns_name("certificate.domains", domain)?;
        }
        if !valid_email(&self.contact_email) {
            return Err(invalid(
                "certificate.contact_email",
                "must be a valid non-empty contact email",
            ));
        }
        Ok(())
    }
}

/// A provider-neutral certificate lifecycle action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CertificateAction {
    Issue,
    Renew,
    Detach,
}

/// One human-readable, non-secret step in a certificate plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CertificatePlanStep {
    pub action: String,
    pub target: String,
    pub description: String,
}

/// A read-only plan that describes certificate and consumer mutations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CertificatePlan {
    pub action: CertificateAction,
    pub resource: ResourceRef,
    pub provider: String,
    pub domains: Vec<String>,
    pub preconditions: Vec<String>,
    pub steps: Vec<CertificatePlanStep>,
}

/// Evidence collected before an issue or renewal operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CertificatePreflight {
    pub provider_available: bool,
    pub web_server_valid: bool,
    pub details: Vec<String>,
}

/// Provider inspection data safe to persist and expose to operators.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CertificateInspection {
    pub provider: String,
    pub certificate_name: String,
    pub domains: Vec<String>,
    pub fullchain_path: String,
    pub private_key_path: String,
    pub present: bool,
    pub renewable: bool,
    pub expires_at_unix_seconds: Option<u64>,
}

impl CertificateInspection {
    pub fn validate(&self) -> Result<()> {
        if self.provider != "certbot-letsencrypt" {
            return Err(invalid(
                "certificate.provider",
                "must identify a trusted registered certificate provider",
            ));
        }
        validate_dns_name("certificate.certificate_name", &self.certificate_name)?;
        if self.domains.is_empty() {
            return Err(invalid(
                "certificate.domains",
                "must contain at least one DNS name",
            ));
        }
        for domain in &self.domains {
            validate_dns_name("certificate.domains", domain)?;
        }
        for (field, path) in [
            ("certificate.fullchain_path", &self.fullchain_path),
            ("certificate.private_key_path", &self.private_key_path),
        ] {
            if !path.starts_with('/') || path.contains(['\n', '\r']) {
                return Err(invalid(field, "must be an absolute path"));
            }
        }
        Ok(())
    }
}

fn validate_dns_name(field: &str, value: &str) -> Result<()> {
    let valid = !value.is_empty()
        && value.len() <= 253
        && !value.contains(['\n', '\r'])
        && value.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        });
    if valid {
        Ok(())
    } else {
        Err(invalid(
            field,
            "must be a lowercase DNS name without wildcards or control characters",
        ))
    }
}

fn valid_email(email: &str) -> bool {
    !email.is_empty()
        && email.len() <= 254
        && !email.contains(['\n', '\r'])
        && email
            .split_once('@')
            .is_some_and(|(local, domain)| !local.is_empty() && domain.contains('.'))
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
    use crate::resource::ResourceKind;

    #[test]
    fn certificate_request_rejects_untrusted_provider_and_invalid_domains() {
        let mut request = CertificateRequest {
            resource: ResourceRef::new(ResourceKind::Certificate, "certificate.demo").unwrap(),
            consumer: ResourceRef::new(ResourceKind::ServiceResource, "nginx.web-host.demo")
                .unwrap(),
            provider: "unknown".into(),
            certificate_name: "demo.example.com".into(),
            domains: vec!["demo.example.com".into()],
            contact_email: "ops@example.com".into(),
        };
        assert!(request.validate().is_err());
        request.provider = "certbot-letsencrypt".into();
        request.domains = vec!["*.example.com".into()];
        assert!(request.validate().is_err());
    }

    #[test]
    fn certificate_request_accepts_explicit_dns_names() {
        let request = CertificateRequest {
            resource: ResourceRef::new(ResourceKind::Certificate, "certificate.demo").unwrap(),
            consumer: ResourceRef::new(ResourceKind::ServiceResource, "nginx.web-host.demo")
                .unwrap(),
            provider: "certbot-letsencrypt".into(),
            certificate_name: "demo.example.com".into(),
            domains: vec!["demo.example.com".into(), "www.demo.example.com".into()],
            contact_email: "ops@example.com".into(),
        };
        assert!(request.validate().is_ok());
    }
}
