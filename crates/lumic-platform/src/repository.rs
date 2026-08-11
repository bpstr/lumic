//! Host-native repository orchestration over the Git executable.

use crate::{
    ProcessOutput, ProcessRunner, ProcessSpec, atomic_file::write_atomic, audit_store::AuditStore,
    event_store::EventStore, resource_lock::ResourceLock, secret_store::SecretStore,
};
use lumic_core::{
    Capability, Change, LumicError, OperationContext, Plan, Result, Risk, RiskLevel,
    application::unix_time_ms,
    events::{AuditRecord, Event},
    repository::{
        GitConfiguration, Repository, RepositoryDeploymentConfiguration, RepositoryDiscovery,
        RepositoryMutation, RepositoryRef, RepositoryRemote, RepositoryRemoteInput,
        RepositoryStatus, RepositoryStorage, create_plan, repository_id, validate_absolute_path,
        validate_ref, validate_remote_url, validate_segment,
    },
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    collections::BTreeSet,
    fs::{self, OpenOptions},
    io::Read,
    os::unix::fs::{DirBuilderExt, OpenOptionsExt},
    path::{Path, PathBuf},
};

const STATE_VERSION: u32 = 1;
const MAX_VISIBLE_REFS: usize = 1_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RepositoryState {
    version: u32,
    #[serde(default)]
    repositories: Vec<Repository>,
}

#[derive(Debug, Default, Deserialize)]
struct LumicConfigurationFile {
    #[serde(default)]
    git: GitConfiguration,
}

impl Default for RepositoryState {
    fn default() -> Self {
        Self {
            version: STATE_VERSION,
            repositories: Vec::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct RepositoryService {
    state_dir: PathBuf,
    state_path: PathBuf,
    config: GitConfiguration,
    secrets: SecretStore,
    events: EventStore,
    audit: AuditStore,
    runner: ProcessRunner,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SmartHttpResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

struct RepositoryRecord<'a> {
    event_type: &'a str,
    operation: &'a str,
    before: Option<&'a Repository>,
    after: Option<&'a Repository>,
    message: &'a str,
}

impl RepositoryService {
    pub fn new(state_dir: impl AsRef<Path>) -> Result<Self> {
        let path = std::env::var_os("LUMIC_CONFIG_FILE")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/etc/lumic/config.toml"));
        let config = if path.exists() {
            let text = fs::read_to_string(&path).map_err(io_error)?;
            toml::from_str::<LumicConfigurationFile>(&text)
                .map_err(|error| LumicError::InvalidInput {
                    field: "config.git".into(),
                    message: format!("{}: {error}", path.display()),
                })?
                .git
        } else {
            GitConfiguration::default()
        };
        Self::with_config(state_dir, config)
    }

    pub fn with_config(state_dir: impl AsRef<Path>, config: GitConfiguration) -> Result<Self> {
        config.validate()?;
        let state_dir = state_dir.as_ref().to_path_buf();
        Ok(Self {
            state_path: state_dir.join("repositories.json"),
            secrets: SecretStore::at_state_dir(&state_dir),
            events: EventStore::at_state_dir(&state_dir),
            audit: AuditStore::at_state_dir(&state_dir),
            state_dir,
            config,
            runner: ProcessRunner,
        })
    }

    pub fn configuration(&self) -> &GitConfiguration {
        &self.config
    }

    pub fn list(&self) -> Result<Vec<Repository>> {
        let mut repositories = self.load()?.repositories;
        repositories.sort_by(|left, right| left.id.cmp(&right.id));
        Ok(repositories)
    }

    pub fn get(&self, id: &str) -> Result<Repository> {
        self.load()?
            .repositories
            .into_iter()
            .find(|item| item.id == id)
            .ok_or_else(|| invalid("repository", "repository is not registered"))
    }

    /// Plans repository deployment configuration without changing state or host files.
    pub fn plan_deployment_configuration(
        &self,
        id: &str,
        configuration: &RepositoryDeploymentConfiguration,
    ) -> Result<Plan> {
        let repository = self.get(id)?;
        configuration.validate()?;
        Ok(Plan {
            id: format!("repository-deployment-configure:{id}"),
            summary: format!("Configure deployment for repository {id}"),
            changes: vec![Change {
                capability: Capability::new("application.deploy"),
                summary: "Persist reviewed repository deployment intent".into(),
                before: repository
                    .deployment
                    .as_ref()
                    .and_then(|value| serde_json::to_string(value).ok()),
                after: serde_json::to_string(configuration).ok(),
                reversible: true,
            }],
            risks: vec![Risk {
                level: RiskLevel::Medium,
                summary: match configuration.strategy {
                    lumic_core::repository::DeploymentStrategy::Atomic => "atomic activation is zero-downtime-capable but runtime behavior may still require a reload".into(),
                    lumic_core::repository::DeploymentStrategy::InPlace => "in-place deployment can expose a partially updated application".into(),
                },
                mitigation: Some("Plan each deployment and retain known-good atomic releases for rollback".into()),
            }],
            preconditions: vec![
                "the associated application exists".into(),
                "the destination is owned or deliberately adopted by Lumic".into(),
                "configured commands are reviewed argument vectors".into(),
            ],
            validation: vec![
                "the exact Git commit is resolved before apply".into(),
                "candidate and final health checks pass when enabled".into(),
            ],
            recovery: vec!["restore the previous known-good release; database migrations are not reversed automatically".into()],
        })
    }

    pub fn configure_deployment(
        &self,
        id: &str,
        configuration: RepositoryDeploymentConfiguration,
        context: &OperationContext,
    ) -> Result<RepositoryMutation> {
        self.authorize(context)?;
        configuration.validate()?;
        let mut repository = self.get(id)?;
        let before = repository.clone();
        repository.deployment = Some(configuration);
        repository.updated_at_unix_ms = unix_time_ms();
        if !context.dry_run {
            self.persist(repository.clone())?;
            self.record(
                context,
                id,
                RepositoryRecord {
                    event_type: "repository.deployment.configured",
                    operation: "configure_deployment",
                    before: Some(&before),
                    after: Some(&repository),
                    message: "repository deployment configuration persisted",
                },
            )?;
        }
        Ok(RepositoryMutation {
            repository,
            action: "configure_deployment".into(),
            changed: !context.dry_run,
            message: if context.dry_run {
                "dry run: deployment configuration validated".into()
            } else {
                "repository deployment configuration persisted".into()
            },
        })
    }

    pub fn plan_create(
        &self,
        namespace: Option<&str>,
        name: &str,
        branch: Option<&str>,
    ) -> Result<Plan> {
        create_plan(
            &self.config,
            namespace.unwrap_or(&self.config.default_namespace),
            name,
            branch.unwrap_or(&self.config.default_branch),
        )
    }

    pub async fn create(
        &self,
        namespace: Option<&str>,
        name: &str,
        branch: Option<&str>,
        context: &OperationContext,
    ) -> Result<RepositoryMutation> {
        self.authorize(context)?;
        let namespace = namespace.unwrap_or(&self.config.default_namespace);
        let branch = branch.unwrap_or(&self.config.default_branch);
        let id = repository_id(namespace, name)?;
        validate_ref("default_branch", branch)?;
        let path = self.managed_path(namespace, name)?;
        if let Ok(existing) = self.get(&id) {
            return Ok(RepositoryMutation {
                repository: existing,
                action: "create".into(),
                changed: false,
                message: "repository already exists".into(),
            });
        }
        if path.exists() {
            return Err(invalid(
                "repository",
                "managed path already exists; discover or adopt it explicitly",
            ));
        }
        let now = unix_time_ms();
        let repository = Repository {
            id: id.clone(),
            namespace: namespace.into(),
            name: name.into(),
            slug: name.into(),
            storage: RepositoryStorage::ManagedBare { path: path.clone() },
            default_branch: branch.into(),
            remotes: Vec::new(),
            deployment: None,
            created_at_unix_ms: now,
            updated_at_unix_ms: now,
        };
        if context.dry_run {
            return Ok(RepositoryMutation {
                repository,
                action: "create".into(),
                changed: false,
                message: "dry run: initialize managed bare repository".into(),
            });
        }
        let _lock = ResourceLock::try_acquire_repository(&self.state_dir, namespace, name)?;
        let parent = path
            .parent()
            .ok_or_else(|| invalid("repository.path", "has no parent"))?;
        fs::create_dir_all(parent).map_err(io_error)?;
        self.run_git([
            "-c".into(),
            format!("core.hooksPath={}", self.disabled_hooks()?.display()),
            "init".into(),
            "--bare".into(),
            "--shared=group".into(),
            format!("--initial-branch={branch}"),
            path.display().to_string(),
        ])
        .await?;
        if let Err(error) = self.persist(repository.clone()) {
            let _ = fs::remove_dir_all(&path);
            return Err(error);
        }
        self.record(
            context,
            &id,
            RepositoryRecord {
                event_type: "repository.created",
                operation: "create",
                before: None,
                after: Some(&repository),
                message: "repository created",
            },
        )?;
        Ok(RepositoryMutation {
            repository,
            action: "create".into(),
            changed: true,
            message: "managed bare repository created".into(),
        })
    }

    pub async fn import(
        &self,
        namespace: Option<&str>,
        name: &str,
        url: &str,
        credential_reference: Option<String>,
        context: &OperationContext,
    ) -> Result<RepositoryMutation> {
        self.authorize(context)?;
        let namespace = namespace.unwrap_or(&self.config.default_namespace);
        let provider = validate_remote_url(url)?;
        let id = repository_id(namespace, name)?;
        if self.get(&id).is_ok() {
            return Err(invalid("repository", "repository is already registered"));
        }
        let path = self.managed_path(namespace, name)?;
        if path.exists() {
            return Err(invalid("repository.path", "destination already exists"));
        }
        if let Some(reference) = &credential_reference {
            self.require_secret(reference)?;
        }
        let now = unix_time_ms();
        let remote = RepositoryRemote {
            id: "origin".into(),
            name: "origin".into(),
            url: url.into(),
            provider,
            credential_reference,
            fetch_enabled: true,
            push_enabled: true,
            mirror: false,
            created_at_unix_ms: now,
        };
        let repository = Repository {
            id: id.clone(),
            namespace: namespace.into(),
            name: name.into(),
            slug: name.into(),
            storage: RepositoryStorage::ManagedBare { path: path.clone() },
            default_branch: self.config.default_branch.clone(),
            remotes: vec![remote.clone()],
            deployment: None,
            created_at_unix_ms: now,
            updated_at_unix_ms: now,
        };
        if context.dry_run {
            return Ok(RepositoryMutation {
                repository,
                action: "import".into(),
                changed: false,
                message: "dry run: bare clone and register repository".into(),
            });
        }
        let _lock = ResourceLock::try_acquire_repository(&self.state_dir, namespace, name)?;
        fs::create_dir_all(
            path.parent()
                .ok_or_else(|| invalid("repository.path", "has no parent"))?,
        )
        .map_err(io_error)?;
        let spec = self.git_spec(vec![
            "clone".into(),
            "--bare".into(),
            "--".into(),
            url.into(),
            path.display().to_string(),
        ])?;
        if let Err(error) = self.run_remote(spec, &remote).await {
            let _ = fs::remove_dir_all(&path);
            return Err(error);
        }
        if let Err(error) = self.disable_repository_hooks(&path).await {
            let _ = fs::remove_dir_all(&path);
            return Err(error);
        }
        if let Err(error) = self.enable_shared_repository(&path).await {
            let _ = fs::remove_dir_all(&path);
            return Err(error);
        }
        if let Err(error) = self.persist(repository.clone()) {
            let _ = fs::remove_dir_all(&path);
            return Err(error);
        }
        self.record(
            context,
            &id,
            RepositoryRecord {
                event_type: "repository.imported",
                operation: "import",
                before: None,
                after: Some(&repository),
                message: "repository imported",
            },
        )?;
        Ok(RepositoryMutation {
            repository,
            action: "import".into(),
            changed: true,
            message: "repository imported as a managed bare clone".into(),
        })
    }

    pub fn register_external(
        &self,
        namespace: Option<&str>,
        name: &str,
        path: &Path,
        context: &OperationContext,
    ) -> Result<RepositoryMutation> {
        self.authorize(context)?;
        let namespace = namespace.unwrap_or(&self.config.default_namespace);
        let id = repository_id(namespace, name)?;
        validate_absolute_path("repository.path", path)?;
        if !path.exists() {
            return Err(invalid("repository.path", "does not exist"));
        }
        let resolved_path = fs::canonicalize(path).map_err(io_error)?;
        self.require_discovery_root(&resolved_path)?;
        let metadata = fs::symlink_metadata(path).map_err(io_error)?;
        if metadata.file_type().is_symlink()
            || !metadata.is_dir()
            || !path.join("HEAD").is_file()
            || !path.join("objects").is_dir()
            || !path.join("refs").is_dir()
        {
            return Err(invalid(
                "repository.path",
                "must identify a non-symlink Git directory",
            ));
        }
        if self.get(&id).is_ok() {
            return Err(invalid("repository", "repository is already registered"));
        }
        let now = unix_time_ms();
        let repository = Repository {
            id: id.clone(),
            namespace: namespace.into(),
            name: name.into(),
            slug: name.into(),
            storage: RepositoryStorage::External {
                path: resolved_path,
            },
            default_branch: self.config.default_branch.clone(),
            remotes: Vec::new(),
            deployment: None,
            created_at_unix_ms: now,
            updated_at_unix_ms: now,
        };
        if context.dry_run {
            return Ok(RepositoryMutation {
                repository,
                action: "register".into(),
                changed: false,
                message: "dry run: register external repository without mutation".into(),
            });
        }
        self.persist(repository.clone())?;
        self.record(
            context,
            &id,
            RepositoryRecord {
                event_type: "repository.created",
                operation: "register",
                before: None,
                after: Some(&repository),
                message: "external repository registered",
            },
        )?;
        Ok(RepositoryMutation {
            repository,
            action: "register".into(),
            changed: true,
            message: "external repository registered; repository contents were not changed".into(),
        })
    }

    pub async fn adopt(&self, id: &str, context: &OperationContext) -> Result<RepositoryMutation> {
        self.authorize(context)?;
        let mut repository = self.get(id)?;
        let source = match &repository.storage {
            RepositoryStorage::External { path } => path.clone(),
            RepositoryStorage::ManagedBare { .. } => {
                return Ok(RepositoryMutation {
                    repository,
                    action: "adopt".into(),
                    changed: false,
                    message: "repository is already managed".into(),
                });
            }
        };
        self.status(id).await?;
        let target = self.managed_path(&repository.namespace, &repository.name)?;
        if target.exists() {
            return Err(invalid(
                "repository.path",
                "managed destination already exists",
            ));
        }
        let before = repository.clone();
        repository.storage = RepositoryStorage::ManagedBare {
            path: target.clone(),
        };
        repository.updated_at_unix_ms = unix_time_ms();
        if context.dry_run {
            return Ok(RepositoryMutation {
                repository,
                action: "adopt".into(),
                changed: false,
                message:
                    "dry run: create a managed bare clone and preserve the external repository"
                        .into(),
            });
        }
        let _lock = ResourceLock::try_acquire_repository(
            &self.state_dir,
            &repository.namespace,
            &repository.name,
        )?;
        fs::create_dir_all(
            target
                .parent()
                .ok_or_else(|| invalid("repository.path", "has no parent"))?,
        )
        .map_err(io_error)?;
        if let Err(error) = self
            .run_git([
                "clone".into(),
                "--bare".into(),
                "--".into(),
                source.display().to_string(),
                target.display().to_string(),
            ])
            .await
        {
            let _ = fs::remove_dir_all(&target);
            return Err(error);
        }
        if let Err(error) = self.disable_repository_hooks(&target).await {
            let _ = fs::remove_dir_all(&target);
            return Err(error);
        }
        if let Err(error) = self.enable_shared_repository(&target).await {
            let _ = fs::remove_dir_all(&target);
            return Err(error);
        }
        if let Err(error) = self.persist(repository.clone()) {
            let _ = fs::remove_dir_all(&target);
            return Err(error);
        }
        self.record(
            context,
            id,
            RepositoryRecord {
                event_type: "repository.adopted",
                operation: "adopt",
                before: Some(&before),
                after: Some(&repository),
                message: "external repository adopted into managed storage",
            },
        )?;
        Ok(RepositoryMutation {
            repository,
            action: "adopt".into(),
            changed: true,
            message: "external repository adopted as a managed bare clone; source preserved".into(),
        })
    }

    pub fn delete(&self, id: &str, context: &OperationContext) -> Result<RepositoryMutation> {
        self.authorize(context)?;
        let repository = self.get(id)?;
        if context.dry_run {
            return Ok(RepositoryMutation {
                repository,
                action: "delete".into(),
                changed: false,
                message: "dry run: unregister repository and trash managed storage".into(),
            });
        }
        let _lock = ResourceLock::try_acquire_repository(
            &self.state_dir,
            &repository.namespace,
            &repository.name,
        )?;
        let mut trashed = None;
        if repository.storage.managed() && repository.storage.path().exists() {
            let trash = self.state_dir.join("trash/repositories").join(format!(
                "{}-{}-{}.git",
                repository.namespace,
                repository.name,
                unix_time_ms()
            ));
            fs::create_dir_all(
                trash
                    .parent()
                    .ok_or_else(|| invalid("trash", "has no parent"))?,
            )
            .map_err(io_error)?;
            fs::rename(repository.storage.path(), &trash).map_err(|error| {
                LumicError::Internal {
                    message: format!("failed to move repository into recoverable trash: {error}"),
                }
            })?;
            trashed = Some(trash);
        }
        let mut state = self.load()?;
        state.repositories.retain(|item| item.id != id);
        if let Err(error) = self.save_state(&state) {
            if let Some(trash) = &trashed {
                let _ = fs::rename(trash, repository.storage.path());
            }
            return Err(error);
        }
        self.record(
            context,
            id,
            RepositoryRecord {
                event_type: "repository.deleted",
                operation: "delete",
                before: Some(&repository),
                after: None,
                message: "repository unregistered; managed storage moved to trash",
            },
        )?;
        Ok(RepositoryMutation {
            repository,
            action: "delete".into(),
            changed: true,
            message: trashed.map_or_else(
                || "external repository unregistered; contents unchanged".into(),
                |path| {
                    format!(
                        "managed repository moved to recoverable trash at {}",
                        path.display()
                    )
                },
            ),
        })
    }

    pub fn discover(&self, root: &Path) -> Result<Vec<RepositoryDiscovery>> {
        validate_absolute_path("discovery.root", root)?;
        let root = fs::canonicalize(root).map_err(io_error)?;
        self.require_discovery_root(&root)?;
        let registered: BTreeSet<PathBuf> = self
            .list()?
            .into_iter()
            .map(|item| item.storage.path().to_path_buf())
            .collect();
        let mut found = Vec::new();
        self.scan(&root, 0, &registered, &mut found)?;
        found.sort_by(|left, right| left.path.cmp(&right.path));
        Ok(found)
    }

    pub async fn status(&self, id: &str) -> Result<RepositoryStatus> {
        let repository = self.get(id)?;
        let path = repository.storage.path();
        if !path.exists() {
            return Ok(RepositoryStatus {
                repository_id: id.into(),
                exists: false,
                bare: false,
                healthy: false,
                head: None,
                object_count: None,
                message: "repository path is missing".into(),
            });
        }
        let bare = self
            .git_text(path, ["rev-parse", "--is-bare-repository"])
            .await?
            == "true";
        let head = self
            .git_text(path, ["symbolic-ref", "--quiet", "--short", "HEAD"])
            .await
            .ok();
        let object_count = self
            .git_text(path, ["count-objects", "-v"])
            .await
            .ok()
            .and_then(|text| {
                text.lines().find_map(|line| {
                    line.strip_prefix("count: ")
                        .and_then(|value| value.parse().ok())
                })
            });
        let healthy = !repository.storage.managed() || bare;
        Ok(RepositoryStatus {
            repository_id: id.into(),
            exists: true,
            bare,
            healthy,
            head,
            object_count,
            message: if bare {
                "healthy bare repository"
            } else if repository.storage.managed() {
                "managed repository is not bare"
            } else {
                "healthy external working repository"
            }
            .into(),
        })
    }

    pub async fn branches(&self, id: &str) -> Result<Vec<RepositoryRef>> {
        self.refs(id, "refs/heads").await
    }
    pub async fn tags(&self, id: &str) -> Result<Vec<RepositoryRef>> {
        self.refs(id, "refs/tags").await
    }

    pub fn add_remote(
        &self,
        id: &str,
        input: RepositoryRemoteInput,
        context: &OperationContext,
    ) -> Result<Repository> {
        self.authorize(context)?;
        validate_segment("remote.name", &input.name)?;
        let provider = validate_remote_url(&input.url)?;
        if let Some(reference) = &input.credential_reference {
            self.require_secret(reference)?;
        }
        let mut repository = self.get(id)?;
        if repository
            .remotes
            .iter()
            .any(|remote| remote.name == input.name)
        {
            return Err(invalid("remote.name", "remote already exists"));
        }
        let before = repository.clone();
        repository.remotes.push(RepositoryRemote {
            id: input.name.clone(),
            name: input.name,
            url: input.url,
            provider,
            credential_reference: input.credential_reference,
            fetch_enabled: input.fetch_enabled,
            push_enabled: input.push_enabled,
            mirror: input.mirror,
            created_at_unix_ms: unix_time_ms(),
        });
        repository.updated_at_unix_ms = unix_time_ms();
        if !context.dry_run {
            self.persist(repository.clone())?;
            self.record(
                context,
                id,
                RepositoryRecord {
                    event_type: "repository.remote.added",
                    operation: "remote_add",
                    before: Some(&before),
                    after: Some(&repository),
                    message: "repository remote added",
                },
            )?;
        }
        Ok(repository)
    }

    pub fn remove_remote(
        &self,
        id: &str,
        name: &str,
        context: &OperationContext,
    ) -> Result<Repository> {
        self.authorize(context)?;
        let mut repository = self.get(id)?;
        let before = repository.clone();
        repository.remotes.retain(|remote| remote.name != name);
        if repository.remotes.len() == before.remotes.len() {
            return Err(invalid("remote.name", "remote is not registered"));
        }
        repository.updated_at_unix_ms = unix_time_ms();
        if !context.dry_run {
            self.persist(repository.clone())?;
            self.record(
                context,
                id,
                RepositoryRecord {
                    event_type: "repository.remote.removed",
                    operation: "remote_remove",
                    before: Some(&before),
                    after: Some(&repository),
                    message: "repository remote removed",
                },
            )?;
        }
        Ok(repository)
    }

    pub async fn fetch(
        &self,
        id: &str,
        remote_name: &str,
        context: &OperationContext,
    ) -> Result<RepositoryMutation> {
        self.remote_operation(id, remote_name, false, false, context)
            .await
    }

    pub async fn push(
        &self,
        id: &str,
        remote_name: &str,
        mirror: bool,
        context: &OperationContext,
    ) -> Result<RepositoryMutation> {
        self.remote_operation(id, remote_name, true, mirror, context)
            .await
    }

    pub fn clone_url(&self, id: &str, origin: &str) -> Result<String> {
        let repository = self.get(id)?;
        if !self.config.http_enabled {
            return Err(invalid(
                "git.http_enabled",
                "Smart HTTP transport is disabled",
            ));
        }
        Ok(format!(
            "{}{}{}/{}/{}.git",
            origin.trim_end_matches('/'),
            self.config.http_path,
            if self.config.http_path.ends_with('/') {
                ""
            } else {
                "/"
            },
            repository.namespace,
            repository.name
        ))
    }

    /// Serve one authenticated Git Smart HTTP CGI request through git-http-backend.
    pub async fn smart_http(
        &self,
        method: &str,
        repository_path: &str,
        query: &str,
        content_type: Option<&str>,
        body: Vec<u8>,
        actor: &str,
    ) -> Result<SmartHttpResponse> {
        if !self.config.enabled || !self.config.http_enabled {
            return Err(invalid(
                "git.http_enabled",
                "Smart HTTP transport is disabled",
            ));
        }
        if !matches!(method, "GET" | "POST") {
            return Err(invalid(
                "request.method",
                "Smart HTTP accepts only GET and POST",
            ));
        }
        if body.len() > 64 * 1024 * 1024 {
            return Err(invalid("request.body", "Smart HTTP request exceeds 64 MiB"));
        }
        let path = repository_path.trim_start_matches('/');
        let mut parts = path.split('/');
        let namespace = parts.next().unwrap_or_default();
        let repository = parts.next().unwrap_or_default();
        let name = repository.strip_suffix(".git").unwrap_or_default();
        validate_segment("namespace", namespace)?;
        validate_segment("repository", name)?;
        let id = repository_id(namespace, name)?;
        let registered = self.get(&id)?;
        if !registered.storage.managed() {
            return Err(invalid(
                "repository",
                "Smart HTTP is available only for managed repositories",
            ));
        }
        let suffix = parts.collect::<Vec<_>>().join("/");
        if suffix.is_empty()
            || suffix
                .split('/')
                .any(|part| part.is_empty() || part == "." || part == "..")
        {
            return Err(invalid(
                "request.path",
                "Smart HTTP service path is invalid",
            ));
        }
        let mut spec = ProcessSpec::new("git")
            .args(["http-backend"])
            .environment(
                "GIT_PROJECT_ROOT",
                self.config.repository_root.display().to_string(),
            )
            .environment("GIT_HTTP_EXPORT_ALL", "1")
            .environment("PATH_INFO", format!("/{namespace}/{repository}/{suffix}"))
            .environment("REQUEST_METHOD", method)
            .environment("QUERY_STRING", query)
            .environment("CONTENT_LENGTH", body.len().to_string())
            .environment("REMOTE_USER", actor)
            .stdin(body);
        if let Some(content_type) = content_type {
            if content_type.contains(['\n', '\r']) {
                return Err(invalid("content_type", "contains a line break"));
            }
            spec = spec.environment("CONTENT_TYPE", content_type);
        }
        spec.stdout_limit = 64 * 1024 * 1024;
        spec.stderr_limit = 64 * 1024;
        let output = self.run_backend(spec).await?;
        parse_cgi_response(output.stdout)
    }

    async fn remote_operation(
        &self,
        id: &str,
        remote_name: &str,
        is_push: bool,
        mirror_push: bool,
        context: &OperationContext,
    ) -> Result<RepositoryMutation> {
        self.authorize(context)?;
        let repository = self.get(id)?;
        let remote = repository
            .remotes
            .iter()
            .find(|item| item.name == remote_name)
            .cloned()
            .ok_or_else(|| invalid("remote", "remote is not registered"))?;
        if is_push && !remote.push_enabled {
            return Err(invalid(
                "remote.push_enabled",
                "pushes are disabled for this remote",
            ));
        }
        if !is_push && !remote.fetch_enabled {
            return Err(invalid(
                "remote.fetch_enabled",
                "fetches are disabled for this remote",
            ));
        }
        if mirror_push && !remote.mirror {
            return Err(invalid(
                "remote.mirror",
                "remote is not approved for mirror pushes",
            ));
        }
        let action = if is_push { "push" } else { "fetch" };
        if context.dry_run {
            return Ok(RepositoryMutation {
                repository,
                action: action.into(),
                changed: false,
                message: format!("dry run: {action} remote {remote_name}"),
            });
        }
        let _lock = ResourceLock::try_acquire_repository(
            &self.state_dir,
            &repository.namespace,
            &repository.name,
        )?;
        let mut args = vec![
            "--git-dir".into(),
            repository.storage.path().display().to_string(),
            action.into(),
        ];
        if mirror_push {
            args.push("--mirror".into());
        }
        args.extend(["--".into(), remote.url.clone()]);
        let spec = self.git_spec(args)?;
        self.run_remote(spec, &remote).await?;
        let event = if is_push {
            "repository.pushed"
        } else {
            "repository.fetched"
        };
        let message = format!("repository {action} completed");
        self.record(
            context,
            id,
            RepositoryRecord {
                event_type: event,
                operation: action,
                before: Some(&repository),
                after: Some(&repository),
                message: &message,
            },
        )?;
        Ok(RepositoryMutation {
            repository,
            action: action.into(),
            changed: true,
            message: format!("repository {action} completed"),
        })
    }

    async fn refs(&self, id: &str, prefix: &str) -> Result<Vec<RepositoryRef>> {
        let repository = self.get(id)?;
        let mut spec = self.git_spec(vec![
            "--git-dir".into(),
            repository.storage.path().display().to_string(),
            "for-each-ref".into(),
            format!("--count={}", MAX_VISIBLE_REFS + 1),
            "--format=%(refname:short) %(objectname)".into(),
            prefix.into(),
        ])?;
        spec.stdout_limit = 1024 * 1024;
        let output = self.run(spec).await?;
        let text = String::from_utf8(output.stdout)
            .map_err(|_| invalid("git.output", "Git returned non-UTF-8 output"))?;
        let references = text
            .lines()
            .filter_map(|line| line.split_once(' '))
            .map(|(name, object_id)| RepositoryRef {
                name: name.into(),
                object_id: object_id.into(),
            })
            .collect::<Vec<_>>();
        if references.len() > MAX_VISIBLE_REFS {
            return Err(invalid(
                "repository.refs",
                "more than 1000 refs matched; use a narrower ref namespace",
            ));
        }
        Ok(references)
    }

    fn managed_path(&self, namespace: &str, name: &str) -> Result<PathBuf> {
        validate_segment("namespace", namespace)?;
        validate_segment("name", name)?;
        Ok(self
            .config
            .repository_root
            .join(namespace)
            .join(format!("{name}.git")))
    }

    fn load(&self) -> Result<RepositoryState> {
        let mut file = match OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&self.state_path)
        {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(RepositoryState::default());
            }
            Err(error) => return Err(io_error(error)),
        };
        if !file.metadata().map_err(io_error)?.is_file() {
            return Err(invalid(
                "repository.state",
                "state path must be a regular file",
            ));
        }
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes).map_err(io_error)?;
        let state: RepositoryState = serde_json::from_slice(&bytes).map_err(json_error)?;
        if state.version != STATE_VERSION {
            return Err(LumicError::Internal {
                message: format!("unsupported repository state version {}", state.version),
            });
        }
        let mut ids = BTreeSet::new();
        for repository in &state.repositories {
            validate_repository_record(repository)?;
            match &repository.storage {
                RepositoryStorage::ManagedBare { path } => {
                    if path != &self.managed_path(&repository.namespace, &repository.name)? {
                        return Err(invalid(
                            "repository.state",
                            "managed repository path does not match its identity",
                        ));
                    }
                }
                RepositoryStorage::External { path } => {
                    self.require_discovery_root(path)?;
                }
            }
            if !ids.insert(&repository.id) {
                return Err(invalid(
                    "repository.state",
                    "contains duplicate repository identifiers",
                ));
            }
        }
        Ok(state)
    }

    fn persist(&self, repository: Repository) -> Result<()> {
        validate_repository_record(&repository)?;
        let _state_lock = ResourceLock::acquire_repository_state(&self.state_dir)?;
        let mut state = self.load()?;
        state.repositories.retain(|item| item.id != repository.id);
        state.repositories.push(repository);
        state
            .repositories
            .sort_by(|left, right| left.id.cmp(&right.id));
        self.save_state(&state)
    }

    fn save_state(&self, state: &RepositoryState) -> Result<()> {
        let mut bytes = serde_json::to_vec_pretty(state).map_err(json_error)?;
        bytes.push(b'\n');
        write_atomic(&self.state_path, &bytes, 0o600).map(|_| ())
    }

    fn scan(
        &self,
        root: &Path,
        depth: usize,
        registered: &BTreeSet<PathBuf>,
        output: &mut Vec<RepositoryDiscovery>,
    ) -> Result<()> {
        if depth > 4 {
            return Ok(());
        }
        for entry in fs::read_dir(root).map_err(io_error)? {
            let entry = entry.map_err(io_error)?;
            let path = entry.path();
            let kind = entry.file_type().map_err(io_error)?;
            if kind.is_symlink() || !kind.is_dir() {
                continue;
            }
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name == ".git" || name.ends_with(".git") {
                output.push(RepositoryDiscovery {
                    path: path.clone(),
                    bare: name.ends_with(".git"),
                    already_registered: registered.contains(&path),
                });
            } else {
                self.scan(&path, depth + 1, registered, output)?;
            }
        }
        Ok(())
    }

    fn require_discovery_root(&self, path: &Path) -> Result<()> {
        if self
            .config
            .discovery_roots
            .iter()
            .filter_map(|root| fs::canonicalize(root).ok())
            .any(|root| path.starts_with(root))
        {
            Ok(())
        } else {
            Err(invalid(
                "repository.path",
                "path is outside configured discovery roots",
            ))
        }
    }

    fn authorize(&self, context: &OperationContext) -> Result<()> {
        if !self.config.enabled {
            return Err(invalid("git.enabled", "repository management is disabled"));
        }
        if !context.approved {
            return Err(LumicError::PolicyDenied {
                capability: lumic_core::Capability::new("repository:write"),
            });
        }
        Ok(())
    }

    fn require_secret(&self, reference: &str) -> Result<()> {
        if self.secrets.exists(reference)? {
            Ok(())
        } else {
            Err(invalid(
                "credential_reference",
                "secret reference does not exist",
            ))
        }
    }

    fn disabled_hooks(&self) -> Result<PathBuf> {
        let path = self.state_dir.join("git-disabled-hooks");
        fs::create_dir_all(&path).map_err(io_error)?;
        Ok(path)
    }

    async fn disable_repository_hooks(&self, path: &Path) -> Result<()> {
        self.run_git([
            "--git-dir".into(),
            path.display().to_string(),
            "config".into(),
            "core.hooksPath".into(),
            self.disabled_hooks()?.display().to_string(),
        ])
        .await
        .map(|_| ())
    }

    async fn enable_shared_repository(&self, path: &Path) -> Result<()> {
        self.run_git([
            "--git-dir".into(),
            path.display().to_string(),
            "config".into(),
            "core.sharedRepository".into(),
            "group".into(),
        ])
        .await?;
        self.run(ProcessSpec::new("chmod").args(["-R", "g+rwX", path.to_string_lossy().as_ref()]))
            .await
            .map(|_| ())
    }

    fn git_spec(&self, args: Vec<String>) -> Result<ProcessSpec> {
        Ok(ProcessSpec::new("git")
            .args(args)
            .environment("GIT_TERMINAL_PROMPT", "0")
            .environment("GIT_CONFIG_NOSYSTEM", "1"))
    }

    async fn run_remote(
        &self,
        mut spec: ProcessSpec,
        remote: &RepositoryRemote,
    ) -> Result<ProcessOutput> {
        let mut identity_directory = None;
        if let Some(reference) = &remote.credential_reference {
            let value = self.secrets.read(reference)?;
            if is_ssh_remote(&remote.url) {
                let directory = PathBuf::from(format!(
                    "/tmp/lumic-git-identity-{}-{}",
                    std::process::id(),
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map_err(|error| invalid("system_time", &error.to_string()))?
                        .as_nanos()
                ));
                let mut builder = fs::DirBuilder::new();
                builder.recursive(false).mode(0o700);
                builder.create(&directory).map_err(io_error)?;
                let identity = directory.join("identity");
                let known_hosts = directory.join("known_hosts");
                let config = directory.join("config");
                if let Err(error) = (|| -> Result<()> {
                    write_atomic(&identity, &value, 0o600)?;
                    write_atomic(&known_hosts, b"", 0o600)?;
                    let configuration = format!(
                        "Host *\n  IdentityFile {}\n  IdentitiesOnly yes\n  BatchMode yes\n  StrictHostKeyChecking accept-new\n  UserKnownHostsFile {}\n",
                        identity.display(),
                        known_hosts.display()
                    );
                    write_atomic(&config, configuration.as_bytes(), 0o600)?;
                    Ok(())
                })() {
                    let _ = fs::remove_dir_all(&directory);
                    return Err(error);
                }
                spec = spec
                    .environment("HOME", directory.to_string_lossy())
                    .environment("GIT_SSH", "/usr/bin/ssh")
                    .environment("GIT_SSH_VARIANT", "ssh")
                    .environment("GIT_CONFIG_COUNT", "1")
                    .environment("GIT_CONFIG_KEY_0", "core.sshCommand")
                    .environment("GIT_CONFIG_VALUE_0", format!("ssh -F {}", config.display()));
                identity_directory = Some(directory);
            } else {
                let value = String::from_utf8(value).map_err(|_| {
                    invalid("credential_reference", "HTTP credential must be UTF-8")
                })?;
                spec = spec
                    .environment("GIT_CONFIG_COUNT", "1")
                    .environment("GIT_CONFIG_KEY_0", "http.extraHeader")
                    .environment(
                        "GIT_CONFIG_VALUE_0",
                        format!("Authorization: Bearer {}", value.trim()),
                    );
            }
        }
        let result = self.run(spec).await;
        if let Some(directory) = identity_directory {
            fs::remove_dir_all(directory).map_err(io_error)?;
        }
        result
    }

    async fn git_text<const N: usize>(&self, path: &Path, args: [&str; N]) -> Result<String> {
        let mut all = vec!["--git-dir".into(), path.display().to_string()];
        all.extend(args.into_iter().map(str::to_owned));
        let output = self.run_git(all).await?;
        String::from_utf8(output.stdout)
            .map(|text| text.trim().to_owned())
            .map_err(|_| invalid("git.output", "Git returned non-UTF-8 output"))
    }

    async fn run_git(&self, args: impl IntoIterator<Item = String>) -> Result<ProcessOutput> {
        self.run(self.git_spec(args.into_iter().collect())?).await
    }

    async fn run(&self, spec: ProcessSpec) -> Result<ProcessOutput> {
        let output = self.runner.run(&spec).await?;
        if output.success() && !output.stdout_truncated && !output.stderr_truncated {
            return Ok(output);
        }
        Err(LumicError::Process {
            executable: "git".into(),
            message: if output.stdout_truncated || output.stderr_truncated {
                "Git output exceeded the configured safety limit".into()
            } else {
                String::from_utf8_lossy(&output.stderr).trim().to_owned()
            },
        })
    }

    async fn run_backend(&self, spec: ProcessSpec) -> Result<ProcessOutput> {
        let output = self.runner.run(&spec).await?;
        if output.success() && !output.stdout_truncated && !output.stderr_truncated {
            Ok(output)
        } else {
            Err(LumicError::Process {
                executable: "git http-backend".into(),
                message: if output.stdout_truncated || output.stderr_truncated {
                    "Git HTTP response exceeded the configured safety limit".into()
                } else {
                    String::from_utf8_lossy(&output.stderr).trim().to_owned()
                },
            })
        }
    }

    fn record(
        &self,
        context: &OperationContext,
        id: &str,
        record: RepositoryRecord<'_>,
    ) -> Result<()> {
        self.audit
            .append(&AuditRecord::now(
                context,
                "repository:write",
                record.operation,
                "repository",
                id,
                json!({}),
                record.before.and_then(|v| serde_json::to_value(v).ok()),
                record.after.and_then(|v| serde_json::to_value(v).ok()),
                true,
                record.message,
            ))
            .map_err(|error| LumicError::Committed {
                operation: record.operation.into(),
                message: format!("audit append failed: {error}"),
            })?;
        self.events
            .append(&Event::now(
                record.event_type,
                &context.actor,
                context.interface,
                "repository",
                id,
                &context.correlation_id,
                json!({"message": record.message}),
            ))
            .map_err(|error| LumicError::Committed {
                operation: record.operation.into(),
                message: format!("event append failed: {error}"),
            })
    }
}

fn is_ssh_remote(url: &str) -> bool {
    url.starts_with("ssh://") || url.starts_with("git@")
}

fn validate_repository_record(repository: &Repository) -> Result<()> {
    validate_segment("repository.namespace", &repository.namespace)?;
    validate_segment("repository.name", &repository.name)?;
    validate_segment("repository.slug", &repository.slug)?;
    validate_ref("repository.default_branch", &repository.default_branch)?;
    validate_absolute_path("repository.path", repository.storage.path())?;
    if repository.id != repository_id(&repository.namespace, &repository.name)? {
        return Err(invalid(
            "repository.id",
            "does not match the namespace and name",
        ));
    }
    let mut remote_names = BTreeSet::new();
    for remote in &repository.remotes {
        validate_segment("repository.remote.name", &remote.name)?;
        let provider = validate_remote_url(&remote.url)?;
        if provider != remote.provider {
            return Err(invalid(
                "repository.remote.provider",
                "does not match the remote URL",
            ));
        }
        if !remote_names.insert(&remote.name) {
            return Err(invalid(
                "repository.remotes",
                "contains duplicate remote names",
            ));
        }
    }
    if let Some(configuration) = &repository.deployment {
        configuration.validate()?;
    }
    Ok(())
}

fn invalid(field: &str, message: &str) -> LumicError {
    LumicError::InvalidInput {
        field: field.into(),
        message: message.into(),
    }
}
fn io_error(error: std::io::Error) -> LumicError {
    LumicError::Internal {
        message: format!("repository I/O failed: {error}"),
    }
}
fn json_error(error: serde_json::Error) -> LumicError {
    LumicError::Internal {
        message: format!("repository state is invalid: {error}"),
    }
}

fn parse_cgi_response(bytes: Vec<u8>) -> Result<SmartHttpResponse> {
    let split = bytes
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|index| (index, 4))
        .or_else(|| {
            bytes
                .windows(2)
                .position(|window| window == b"\n\n")
                .map(|index| (index, 2))
        })
        .ok_or_else(|| invalid("git_http_backend", "returned no CGI header separator"))?;
    let header_text = std::str::from_utf8(&bytes[..split.0])
        .map_err(|_| invalid("git_http_backend", "returned non-UTF-8 CGI headers"))?;
    let mut status = 200;
    let mut headers = Vec::new();
    for line in header_text.lines() {
        let (name, value) = line
            .trim_end_matches('\r')
            .split_once(':')
            .ok_or_else(|| invalid("git_http_backend", "returned an invalid CGI header"))?;
        if name.eq_ignore_ascii_case("status") {
            status = value
                .split_whitespace()
                .next()
                .and_then(|value| value.parse().ok())
                .ok_or_else(|| invalid("git_http_backend", "returned an invalid status"))?;
        } else if name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
            && !value.contains(['\n', '\r'])
        {
            headers.push((name.into(), value.trim().into()));
        }
    }
    Ok(SmartHttpResponse {
        status,
        headers,
        body: bytes[split.0 + split.1..].to_vec(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumic_core::{
        OperationInterface,
        repository::{
            DeploymentHealthConfiguration, DeploymentStrategy, GitConfiguration,
            RepositoryDeploymentConfiguration,
        },
    };
    use std::sync::atomic::{AtomicU64, Ordering};

    static FIXTURE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

    fn context(dry_run: bool) -> OperationContext {
        OperationContext {
            actor: "test".into(),
            interface: OperationInterface::Internal,
            correlation_id: "repository-test".into(),
            dry_run,
            approved: true,
        }
    }
    fn fixture() -> (PathBuf, RepositoryService) {
        let root = std::env::temp_dir().join(format!(
            "lumic-repository-{}-{}-{}",
            std::process::id(),
            unix_time_ms(),
            FIXTURE_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        let config = GitConfiguration {
            repository_root: root.join("repositories"),
            discovery_roots: vec![root.join("external")],
            ..GitConfiguration::default()
        };
        let service = RepositoryService::with_config(root.join("state"), config).unwrap();
        (root, service)
    }

    #[tokio::test]
    async fn creates_namespaced_bare_repository_idempotently() {
        let (root, service) = fixture();
        let created = service
            .create(Some("team"), "api", Some("main"), &context(false))
            .await
            .unwrap();
        assert!(created.changed);
        assert!(created.repository.storage.path().ends_with("team/api.git"));
        assert!(service.status("team/api").await.unwrap().healthy);
        assert!(
            !service
                .create(Some("team"), "api", None, &context(false))
                .await
                .unwrap()
                .changed
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn enforces_fetch_and_push_permissions_independently() {
        let (root, service) = fixture();
        service
            .create(Some("team"), "api", Some("main"), &context(false))
            .await
            .unwrap();
        service
            .add_remote(
                "team/api",
                RepositoryRemoteInput {
                    name: "push-only".into(),
                    url: "https://example.test/team/api.git".into(),
                    credential_reference: None,
                    fetch_enabled: false,
                    push_enabled: true,
                    mirror: false,
                },
                &context(false),
            )
            .unwrap();

        assert!(
            service
                .fetch("team/api", "push-only", &context(true))
                .await
                .is_err()
        );
        assert_eq!(
            service
                .push("team/api", "push-only", false, &context(true))
                .await
                .unwrap()
                .action,
            "push"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn adopts_working_repository_without_changing_the_source() {
        let (root, service) = fixture();
        let working_tree = root.join("external/app");
        fs::create_dir_all(&working_tree).unwrap();
        assert!(
            std::process::Command::new("git")
                .args(["init", "--initial-branch=main", "--"])
                .arg(&working_tree)
                .status()
                .unwrap()
                .success()
        );
        let git_dir = working_tree.join(".git");
        service
            .register_external(Some("team"), "app", &git_dir, &context(false))
            .unwrap();
        let external_status = service.status("team/app").await.unwrap();
        assert!(external_status.healthy);
        assert!(!external_status.bare);

        let adopted = service.adopt("team/app", &context(false)).await.unwrap();
        assert!(git_dir.is_dir());
        assert!(adopted.repository.storage.managed());
        assert!(service.status("team/app").await.unwrap().bare);
        fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn plans_and_persists_repository_deployment_configuration() {
        let (root, service) = fixture();
        service
            .create(Some("team"), "api", Some("main"), &context(false))
            .await
            .unwrap();
        let configuration = RepositoryDeploymentConfiguration {
            enabled: true,
            application_id: "api".into(),
            branch: "main".into(),
            destination: root.join("apps/api"),
            strategy: DeploymentStrategy::Atomic,
            deploy_on_push: true,
            keep_releases: 5,
            install_command: None,
            build_command: None,
            migrate_command: None,
            hooks: Vec::new(),
            shared_directories: vec!["storage".into()],
            shared_files: vec![".env".into()],
            health: DeploymentHealthConfiguration::default(),
        };

        let plan = service
            .plan_deployment_configuration("team/api", &configuration)
            .unwrap();
        assert!(plan.changes[0].reversible);
        assert_eq!(service.get("team/api").unwrap().deployment, None);

        let dry_run = service
            .configure_deployment("team/api", configuration.clone(), &context(true))
            .unwrap();
        assert!(!dry_run.changed);
        assert_eq!(service.get("team/api").unwrap().deployment, None);

        let applied = service
            .configure_deployment("team/api", configuration.clone(), &context(false))
            .unwrap();
        assert!(applied.changed);
        assert_eq!(
            service.get("team/api").unwrap().deployment,
            Some(configuration)
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn discovery_is_confined_to_configured_roots() {
        let (root, service) = fixture();
        assert!(service.discover(&root).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn parses_git_http_backend_response() {
        let response = parse_cgi_response(
            b"Status: 401 Unauthorized\r\nContent-Type: text/plain\r\n\r\ndenied".to_vec(),
        )
        .unwrap();
        assert_eq!(response.status, 401);
        assert_eq!(
            response.headers,
            [("Content-Type".into(), "text/plain".into())]
        );
        assert_eq!(response.body, b"denied");
    }

    #[test]
    fn distinguishes_ssh_identity_remotes_from_http_bearer_remotes() {
        assert!(is_ssh_remote("ssh://git@example.com/team/repo.git"));
        assert!(is_ssh_remote("git@example.com:team/repo.git"));
        assert!(!is_ssh_remote("https://example.com/team/repo.git"));
    }

    #[test]
    fn concurrent_registry_updates_do_not_lose_repositories() {
        let (root, service) = fixture();
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(12));
        let handles = (0..12)
            .map(|index| {
                let service = service.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    let name = format!("repo-{index}");
                    let now = unix_time_ms();
                    let repository = Repository {
                        id: format!("team/{name}"),
                        namespace: "team".into(),
                        name: name.clone(),
                        slug: name.clone(),
                        storage: RepositoryStorage::ManagedBare {
                            path: service
                                .config
                                .repository_root
                                .join("team")
                                .join(format!("{name}.git")),
                        },
                        default_branch: "main".into(),
                        remotes: Vec::new(),
                        deployment: None,
                        created_at_unix_ms: now,
                        updated_at_unix_ms: now,
                    };
                    barrier.wait();
                    service.persist(repository).unwrap();
                })
            })
            .collect::<Vec<_>>();
        for handle in handles {
            handle.join().unwrap();
        }
        assert_eq!(service.list().unwrap().len(), 12);
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn state_loader_rejects_symbolic_links() {
        use std::os::unix::fs::symlink;

        let (root, service) = fixture();
        fs::create_dir_all(&service.state_dir).unwrap();
        let target = root.join("attacker-controlled.json");
        fs::write(&target, b"{\"version\":1,\"repositories\":[]}").unwrap();
        symlink(&target, &service.state_path).unwrap();
        assert!(service.list().is_err());
        fs::remove_dir_all(root).unwrap();
    }
}
