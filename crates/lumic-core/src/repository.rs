//! Provider-neutral Git repository contracts.

use crate::{Capability, Change, LumicError, Plan, Result, Risk, RiskLevel};
use serde::{Deserialize, Serialize};
use std::path::{Component, Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Repository {
    pub id: String,
    pub namespace: String,
    pub name: String,
    pub slug: String,
    pub storage: RepositoryStorage,
    pub default_branch: String,
    #[serde(default)]
    pub remotes: Vec<RepositoryRemote>,
    /// Optional deployment intent. Repository registration remains useful without it.
    #[serde(default)]
    pub deployment: Option<RepositoryDeploymentConfiguration>,
    pub created_at_unix_ms: u128,
    pub updated_at_unix_ms: u128,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeploymentStrategy {
    InPlace,
    Atomic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeploymentHookStage {
    Prepare,
    BeforeInstall,
    AfterInstall,
    BeforeBuild,
    AfterBuild,
    BeforeMigrate,
    AfterMigrate,
    BeforeSwitch,
    AfterSwitch,
    Rollback,
    Cleanup,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeploymentHook {
    pub stage: DeploymentHookStage,
    /// A validated argument vector, never a shell command string.
    pub command: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct DeploymentHealthConfiguration {
    pub enabled: bool,
    pub url: String,
    pub timeout_seconds: u64,
    pub retry_interval_seconds: u64,
    pub expected_status_min: u16,
    pub expected_status_max: u16,
    pub automatic_rollback: bool,
}

impl Default for DeploymentHealthConfiguration {
    fn default() -> Self {
        Self {
            enabled: false,
            url: String::new(),
            timeout_seconds: 30,
            retry_interval_seconds: 2,
            expected_status_min: 200,
            expected_status_max: 399,
            automatic_rollback: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepositoryDeploymentConfiguration {
    pub enabled: bool,
    pub application_id: String,
    pub branch: String,
    pub destination: PathBuf,
    pub strategy: DeploymentStrategy,
    #[serde(default)]
    pub deploy_on_push: bool,
    pub keep_releases: usize,
    #[serde(default)]
    pub install_command: Option<Vec<String>>,
    #[serde(default)]
    pub build_command: Option<Vec<String>>,
    #[serde(default)]
    pub migrate_command: Option<Vec<String>>,
    #[serde(default)]
    pub hooks: Vec<DeploymentHook>,
    #[serde(default)]
    pub shared_directories: Vec<PathBuf>,
    #[serde(default)]
    pub shared_files: Vec<PathBuf>,
    #[serde(default)]
    pub health: DeploymentHealthConfiguration,
}

impl RepositoryDeploymentConfiguration {
    pub fn validate(&self) -> Result<()> {
        validate_segment("deployment.application_id", &self.application_id)?;
        validate_ref("deployment.branch", &self.branch)?;
        validate_deployment_destination(&self.destination)?;
        if self.keep_releases == 0 || self.keep_releases > 100 {
            return Err(invalid(
                "deployment.keep_releases",
                "must be between 1 and 100",
            ));
        }
        for command in self
            .install_command
            .iter()
            .chain(self.build_command.iter())
            .chain(self.migrate_command.iter())
            .chain(self.hooks.iter().map(|hook| &hook.command))
        {
            validate_deployment_command(command)?;
        }
        for path in self
            .shared_directories
            .iter()
            .chain(self.shared_files.iter())
        {
            validate_shared_path(path)?;
        }
        self.health.validate()
    }
}

impl DeploymentHealthConfiguration {
    fn validate(&self) -> Result<()> {
        if !self.enabled {
            return Ok(());
        }
        if !(self.url.starts_with("http://127.0.0.1")
            || self.url.starts_with("http://localhost")
            || self.url.starts_with("https://127.0.0.1")
            || self.url.starts_with("https://localhost"))
            || self.url.contains(['\n', '\r', '\0'])
            || self.timeout_seconds == 0
            || self.retry_interval_seconds == 0
            || self.retry_interval_seconds > self.timeout_seconds
            || !(100..=599).contains(&self.expected_status_min)
            || !(self.expected_status_min..=599).contains(&self.expected_status_max)
        {
            return Err(invalid(
                "deployment.health",
                "must be a bounded localhost HTTP(S) check with a valid status range",
            ));
        }
        Ok(())
    }
}

pub fn validate_deployment_destination(path: &Path) -> Result<()> {
    validate_absolute_path("deployment.destination", path)?;
    const PROTECTED: [&str; 8] = [
        "/",
        "/etc",
        "/bin",
        "/sbin",
        "/usr",
        "/boot",
        "/dev",
        "/var/lib/lumic",
    ];
    if PROTECTED
        .iter()
        .any(|protected| path == Path::new(protected))
    {
        return Err(invalid(
            "deployment.destination",
            "targets a protected system location",
        ));
    }
    Ok(())
}

fn validate_deployment_command(command: &[String]) -> Result<()> {
    if command.is_empty()
        || command.len() > 128
        || command
            .iter()
            .any(|part| part.is_empty() || part.len() > 4096 || part.contains(['\n', '\r', '\0']))
    {
        return Err(invalid(
            "deployment.command",
            "must be a bounded non-empty argument vector without control characters",
        ));
    }
    Ok(())
}

fn validate_shared_path(path: &Path) -> Result<()> {
    if path.is_absolute()
        || path.as_os_str().is_empty()
        || path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(invalid(
            "deployment.shared",
            "must be a non-empty relative path without parent traversal",
        ));
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RepositoryStorage {
    ManagedBare { path: PathBuf },
    External { path: PathBuf },
}

impl RepositoryStorage {
    pub fn path(&self) -> &Path {
        match self {
            Self::ManagedBare { path } | Self::External { path } => path,
        }
    }

    pub const fn managed(&self) -> bool {
        matches!(self, Self::ManagedBare { .. })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GitProvider {
    Github,
    Gitlab,
    Bitbucket,
    Generic,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepositoryRemote {
    pub id: String,
    pub name: String,
    pub url: String,
    pub provider: GitProvider,
    pub credential_reference: Option<String>,
    pub fetch_enabled: bool,
    pub push_enabled: bool,
    pub mirror: bool,
    pub created_at_unix_ms: u128,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepositoryRemoteInput {
    pub name: String,
    pub url: String,
    pub credential_reference: Option<String>,
    pub fetch_enabled: bool,
    pub push_enabled: bool,
    pub mirror: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepositoryStatus {
    pub repository_id: String,
    pub exists: bool,
    pub bare: bool,
    pub healthy: bool,
    pub head: Option<String>,
    pub object_count: Option<u64>,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepositoryRef {
    pub name: String,
    pub object_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepositoryDiscovery {
    pub path: PathBuf,
    pub bare: bool,
    pub already_registered: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepositoryMutation {
    pub repository: Repository,
    pub action: String,
    pub changed: bool,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct GitConfiguration {
    pub enabled: bool,
    pub repository_root: PathBuf,
    pub http_enabled: bool,
    pub http_path: String,
    pub default_namespace: String,
    pub default_branch: String,
    #[serde(default)]
    pub discovery_roots: Vec<PathBuf>,
}

impl Default for GitConfiguration {
    fn default() -> Self {
        Self {
            enabled: true,
            repository_root: PathBuf::from("/var/lib/lumic/repositories"),
            http_enabled: true,
            http_path: "/git".into(),
            default_namespace: "default".into(),
            default_branch: "main".into(),
            discovery_roots: Vec::new(),
        }
    }
}

impl GitConfiguration {
    pub fn validate(&self) -> Result<()> {
        validate_segment("git.default_namespace", &self.default_namespace)?;
        validate_ref("git.default_branch", &self.default_branch)?;
        validate_absolute_path("git.repository_root", &self.repository_root)?;
        if !self.http_path.starts_with('/') || self.http_path.contains("..") {
            return Err(invalid(
                "git.http_path",
                "must be an absolute URL path without '..'",
            ));
        }
        for root in &self.discovery_roots {
            validate_absolute_path("git.discovery_roots", root)?;
        }
        Ok(())
    }
}

pub fn repository_id(namespace: &str, name: &str) -> Result<String> {
    validate_segment("namespace", namespace)?;
    validate_segment("name", name)?;
    Ok(format!("{namespace}/{name}"))
}

pub fn validate_segment(field: &str, value: &str) -> Result<()> {
    let valid = !value.is_empty()
        && value.len() <= 64
        && !value.starts_with(['.', '-'])
        && !value.ends_with('.')
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'));
    if valid {
        Ok(())
    } else {
        Err(invalid(
            field,
            "must be a safe 1-64 character repository segment",
        ))
    }
}

pub fn validate_ref(field: &str, value: &str) -> Result<()> {
    let invalid_ref = value.is_empty()
        || value.len() > 255
        || value.starts_with('-')
        || value.starts_with('/')
        || value.ends_with('/')
        || value.ends_with('.')
        || value.contains("..")
        || value.contains("@{")
        || value.contains("//")
        || value.bytes().any(|byte| {
            byte.is_ascii_control()
                || byte == b' '
                || matches!(byte, b'~' | b'^' | b':' | b'?' | b'*' | b'[' | b'\\')
        });
    if invalid_ref {
        Err(invalid(field, "is not a safe Git reference name"))
    } else {
        Ok(())
    }
}

pub fn validate_remote_url(value: &str) -> Result<GitProvider> {
    if value.len() > 2048 || value.contains(['\n', '\r', '\0']) || value.starts_with('-') {
        return Err(invalid("remote.url", "is not a safe Git remote URL"));
    }
    let lower = value.to_ascii_lowercase();
    let supported = lower.starts_with("https://")
        || lower.starts_with("ssh://")
        || lower.starts_with("git://")
        || lower.starts_with("git@")
        || lower.starts_with("file://");
    if !supported {
        return Err(invalid(
            "remote.url",
            "must use https, ssh, git, file, or scp-style SSH syntax",
        ));
    }
    if lower.contains(['?', '#']) {
        return Err(invalid(
            "remote.url",
            "query strings and fragments are not allowed in Git remote URLs",
        ));
    }
    if lower.starts_with("https://")
        && lower
            .strip_prefix("https://")
            .and_then(|rest| rest.split('/').next())
            .is_some_and(|authority| authority.contains('@'))
    {
        return Err(invalid(
            "remote.url",
            "HTTPS credentials must be stored as a secret reference, not embedded in the URL",
        ));
    }
    let host = remote_host(&lower);
    if lower.starts_with("file://") {
        if !lower
            .strip_prefix("file://")
            .is_some_and(|path| path.starts_with('/'))
        {
            return Err(invalid(
                "remote.url",
                "file remotes must use an absolute path",
            ));
        }
    } else if host.is_none_or(str::is_empty) {
        return Err(invalid("remote.url", "must include a host name"));
    }
    Ok(if host == Some("github.com") {
        GitProvider::Github
    } else if host == Some("gitlab.com") {
        GitProvider::Gitlab
    } else if host == Some("bitbucket.org") {
        GitProvider::Bitbucket
    } else {
        GitProvider::Generic
    })
}

fn remote_host(value: &str) -> Option<&str> {
    let authority = if let Some((_, rest)) = value.split_once("://") {
        rest.split('/').next()?
    } else {
        value.split(':').next()?
    };
    let host_and_port = authority.rsplit('@').next()?;
    host_and_port.split(':').next()
}

pub fn validate_absolute_path(field: &str, path: &Path) -> Result<()> {
    if !path.is_absolute()
        || path
            .components()
            .any(|component| matches!(component, Component::ParentDir))
    {
        return Err(invalid(field, "must be an absolute normalized path"));
    }
    Ok(())
}

pub fn create_plan(
    config: &GitConfiguration,
    namespace: &str,
    name: &str,
    branch: &str,
) -> Result<Plan> {
    let id = repository_id(namespace, name)?;
    validate_ref("default_branch", branch)?;
    let path = config
        .repository_root
        .join(namespace)
        .join(format!("{name}.git"));
    Ok(Plan {
        id: format!("repository-create:{id}"),
        summary: format!("Create managed bare repository {id}"),
        changes: vec![Change {
            capability: Capability::new("repository:write"),
            summary: "Initialize a managed bare Git repository".into(),
            before: None,
            after: Some(path.display().to_string()),
            reversible: true,
        }],
        risks: vec![Risk {
            level: RiskLevel::Low,
            summary: "Creates repository storage on the host".into(),
            mitigation: Some(
                "The empty repository can be removed through repository delete".into(),
            ),
        }],
        preconditions: vec![
            "Git is installed".into(),
            "repository:write is approved".into(),
        ],
        validation: vec!["git rev-parse confirms a bare repository".into()],
        recovery: vec!["Remove the newly created empty repository directory".into()],
    })
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

    #[test]
    fn repository_identity_rejects_traversal() {
        assert!(repository_id("team", "api").is_ok());
        assert!(repository_id("..", "api").is_err());
        assert!(repository_id("team", "api/name").is_err());
    }

    #[test]
    fn provider_detection_is_host_based() {
        assert_eq!(
            validate_remote_url("https://github.com/acme/api.git").unwrap(),
            GitProvider::Github
        );
        assert_eq!(
            validate_remote_url("ssh://git@example.test/acme/api.git").unwrap(),
            GitProvider::Generic
        );
        assert_eq!(
            validate_remote_url("https://github.com.evil.test/acme/api.git").unwrap(),
            GitProvider::Generic
        );
        assert!(validate_remote_url("https://token@github.com/acme/api.git").is_err());
    }

    #[test]
    fn deployment_configuration_rejects_shell_and_protected_paths() {
        let mut configuration = RepositoryDeploymentConfiguration {
            enabled: true,
            application_id: "shop".into(),
            branch: "main".into(),
            destination: PathBuf::from("/var/www/shop"),
            strategy: DeploymentStrategy::Atomic,
            deploy_on_push: true,
            keep_releases: 5,
            install_command: Some(vec!["composer".into(), "install".into()]),
            build_command: None,
            migrate_command: None,
            hooks: Vec::new(),
            shared_directories: vec![PathBuf::from("storage")],
            shared_files: vec![PathBuf::from(".env")],
            health: DeploymentHealthConfiguration::default(),
        };
        assert!(configuration.validate().is_ok());
        configuration.destination = PathBuf::from("/etc");
        assert!(configuration.validate().is_err());
        configuration.destination = PathBuf::from("/var/www/shop");
        configuration.build_command = Some(vec!["sh\n-c".into()]);
        assert!(configuration.validate().is_err());
    }
}
