use crate::{ProcessOutput, ProcessRunner, ProcessSpec, event_store::EventStore};
use lumic_core::{
    LumicError, OperationContext, Result,
    application::{
        Application, ApplicationRuntime, Deployment, DeploymentStatus, RepositoryConfig,
        unix_time_ms, validate_branch, validate_domain, validate_repository_url, validate_slug,
    },
    events::Event,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
#[cfg(unix)]
use std::os::unix::fs::{OpenOptionsExt, symlink};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    time::Duration,
};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct ApplicationState {
    version: u32,
    applications: Vec<Application>,
    deployments: Vec<Deployment>,
}

#[derive(Debug, Clone)]
pub struct ApplicationStore {
    path: PathBuf,
}

impl ApplicationStore {
    pub fn at_state_dir(directory: impl AsRef<Path>) -> Self {
        Self {
            path: directory.as_ref().join("applications.json"),
        }
    }

    fn load(&self) -> Result<ApplicationState> {
        if !self.path.exists() {
            return Ok(ApplicationState {
                version: 1,
                ..ApplicationState::default()
            });
        }
        let bytes = fs::read(&self.path).map_err(state_io_error)?;
        serde_json::from_slice(&bytes).map_err(|error| LumicError::Internal {
            message: format!("application state is invalid: {error}"),
        })
    }

    fn save(&self, state: &ApplicationState) -> Result<()> {
        let parent = self.path.parent().ok_or_else(|| LumicError::Internal {
            message: "application state path has no parent".into(),
        })?;
        fs::create_dir_all(parent).map_err(state_io_error)?;
        let temporary = parent.join(format!(".applications-{}.tmp", std::process::id()));
        let mut options = OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        options.mode(0o600);
        let mut file = options.open(&temporary).map_err(state_io_error)?;
        serde_json::to_writer_pretty(&mut file, state).map_err(|error| LumicError::Internal {
            message: format!("could not serialize application state: {error}"),
        })?;
        file.write_all(b"\n").map_err(state_io_error)?;
        file.sync_all().map_err(state_io_error)?;
        fs::rename(&temporary, &self.path).map_err(state_io_error)
    }
}

#[derive(Debug, Clone)]
pub struct ApplicationService {
    store: ApplicationStore,
    events: EventStore,
    apps_root: PathBuf,
    runner: ProcessRunner,
}

impl ApplicationService {
    pub fn new(state_dir: impl AsRef<Path>, apps_root: impl Into<PathBuf>) -> Self {
        Self {
            store: ApplicationStore::at_state_dir(&state_dir),
            events: EventStore::at_state_dir(state_dir),
            apps_root: apps_root.into(),
            runner: ProcessRunner,
        }
    }

    pub fn list(&self) -> Result<Vec<Application>> {
        Ok(self.store.load()?.applications)
    }

    pub fn inspect(&self, id: &str) -> Result<Application> {
        validate_slug("application", id)?;
        self.store
            .load()?
            .applications
            .into_iter()
            .find(|application| application.id == id)
            .ok_or_else(|| not_found(id))
    }

    pub fn create(
        &self,
        name: &str,
        domain: &str,
        runtime: ApplicationRuntime,
        www_alias: bool,
        context: &OperationContext,
    ) -> Result<Application> {
        validate_slug("application", name)?;
        validate_domain(domain)?;
        let mut state = self.store.load()?;
        if state
            .applications
            .iter()
            .any(|app| app.id == name || app.domain == domain)
        {
            return Err(LumicError::InvalidInput {
                field: "application".into(),
                message: "application name and domain must be unique".into(),
            });
        }
        let root = self.apps_root.join(name);
        for directory in ["releases", "shared", "repository"] {
            fs::create_dir_all(root.join(directory)).map_err(state_io_error)?;
        }
        let now = unix_time_ms();
        let application = Application {
            id: name.into(),
            name: name.into(),
            domain: domain.into(),
            www_alias,
            root: root.to_string_lossy().into_owned(),
            runtime,
            repository: None,
            environment_references: Default::default(),
            health_check: Default::default(),
            release_retention: 5,
            health_status: "not_deployed".into(),
            created_at_unix_ms: now,
            updated_at_unix_ms: now,
        };
        state.applications.push(application.clone());
        self.store.save(&state)?;
        self.emit(
            "application.created",
            &application.id,
            context,
            json!({
                "domain": application.domain,
                "runtime": application.runtime,
            }),
        )?;
        Ok(application)
    }

    pub fn set_repository(
        &self,
        id: &str,
        url: &str,
        branch: &str,
        credential_reference: Option<String>,
        context: &OperationContext,
    ) -> Result<Application> {
        validate_repository_url(url)?;
        validate_branch(branch)?;
        let mut state = self.store.load()?;
        let application = state
            .applications
            .iter_mut()
            .find(|application| application.id == id)
            .ok_or_else(|| not_found(id))?;
        application.repository = Some(RepositoryConfig {
            url: url.into(),
            branch: branch.into(),
            credential_reference,
        });
        application.updated_at_unix_ms = unix_time_ms();
        let application = application.clone();
        self.store.save(&state)?;
        self.emit(
            "application.repository_configured",
            id,
            context,
            json!({
                "branch": branch,
            }),
        )?;
        Ok(application)
    }

    pub fn delete(&self, id: &str, context: &OperationContext) -> Result<()> {
        validate_slug("application", id)?;
        let mut state = self.store.load()?;
        let index = state
            .applications
            .iter()
            .position(|application| application.id == id)
            .ok_or_else(|| not_found(id))?;
        let application = state.applications.remove(index);
        state
            .deployments
            .retain(|deployment| deployment.application_id != id);
        let root = PathBuf::from(&application.root);
        if root.exists() {
            let trash = self.apps_root.join(".trash");
            fs::create_dir_all(&trash).map_err(state_io_error)?;
            fs::rename(&root, trash.join(format!("{id}-{}", unix_time_ms())))
                .map_err(state_io_error)?;
        }
        self.store.save(&state)?;
        self.emit(
            "application.deleted",
            id,
            context,
            json!({"recoverable_from_trash": true}),
        )
    }

    pub fn deployments(&self, id: &str) -> Result<Vec<Deployment>> {
        self.inspect(id)?;
        let mut deployments: Vec<_> = self
            .store
            .load()?
            .deployments
            .into_iter()
            .filter(|deployment| deployment.application_id == id)
            .collect();
        deployments.reverse();
        Ok(deployments)
    }

    pub async fn deploy(&self, id: &str, context: &OperationContext) -> Result<Deployment> {
        let application = self.inspect(id)?;
        let repository =
            application
                .repository
                .clone()
                .ok_or_else(|| LumicError::InvalidInput {
                    field: "repository".into(),
                    message: "configure a repository before deployment".into(),
                })?;
        let deployment_id = format!("{}-{}", unix_time_ms(), std::process::id());
        let release = PathBuf::from(&application.root)
            .join("releases")
            .join(&deployment_id);
        let previous_release = current_release(&application)?;
        let mut deployment = Deployment {
            id: deployment_id,
            application_id: id.into(),
            release_path: release.to_string_lossy().into_owned(),
            commit: String::new(),
            status: DeploymentStatus::Started,
            healthy: false,
            message: "preparing release".into(),
            previous_release,
            started_at_unix_ms: unix_time_ms(),
            finished_at_unix_ms: None,
        };
        self.upsert_deployment(&deployment)?;
        self.emit(
            "deployment.started",
            id,
            context,
            json!({"deployment_id": deployment.id}),
        )?;

        match self
            .prepare_release(&application, &repository, &release)
            .await
        {
            Ok(commit) => {
                deployment.commit = commit;
                deployment.healthy = true;
                deployment.status = DeploymentStatus::Completed;
                deployment.message =
                    "release activated and static/runtime entry point verified".into();
                deployment.finished_at_unix_ms = Some(unix_time_ms());
                self.upsert_deployment(&deployment)?;
                self.set_health(id, "healthy")?;
                self.emit(
                    "deployment.completed",
                    id,
                    context,
                    json!({
                        "deployment_id": deployment.id,
                        "commit": deployment.commit,
                    }),
                )?;
                self.prune_releases(&application)?;
                Ok(deployment)
            }
            Err(error) => {
                if release.exists() {
                    fs::remove_dir_all(&release).map_err(state_io_error)?;
                }
                deployment.status = DeploymentStatus::Failed;
                deployment.message = error.to_string();
                deployment.finished_at_unix_ms = Some(unix_time_ms());
                self.upsert_deployment(&deployment)?;
                self.set_health(id, "deployment_failed")?;
                self.emit(
                    "deployment.failed",
                    id,
                    context,
                    json!({
                        "deployment_id": deployment.id,
                        "reason": deployment.message,
                    }),
                )?;
                Err(error)
            }
        }
    }

    pub fn rollback(&self, id: &str, context: &OperationContext) -> Result<Deployment> {
        let application = self.inspect(id)?;
        let current = current_release(&application);
        let current = current?;
        let state = self.store.load()?;
        let target = state
            .deployments
            .iter()
            .rev()
            .find(|deployment| {
                deployment.application_id == id
                    && deployment.status == DeploymentStatus::Completed
                    && Some(&deployment.release_path) != current.as_ref()
                    && Path::new(&deployment.release_path).exists()
            })
            .cloned()
            .ok_or_else(|| LumicError::InvalidInput {
                field: "deployment".into(),
                message: "no previous known-good release is available".into(),
            })?;
        activate(&application, Path::new(&target.release_path), &target.id)?;
        let mut rollback = target;
        rollback.id = format!("rollback-{}-{}", unix_time_ms(), std::process::id());
        rollback.status = DeploymentStatus::RolledBack;
        rollback.previous_release = current;
        rollback.started_at_unix_ms = unix_time_ms();
        rollback.finished_at_unix_ms = Some(unix_time_ms());
        rollback.message = "previous known-good release activated".into();
        self.upsert_deployment(&rollback)?;
        self.set_health(id, "healthy")?;
        self.emit(
            "deployment.rolled_back",
            id,
            context,
            json!({
                "deployment_id": rollback.id,
                "release": rollback.release_path,
            }),
        )?;
        Ok(rollback)
    }

    async fn prepare_release(
        &self,
        application: &Application,
        repository: &RepositoryConfig,
        release: &Path,
    ) -> Result<String> {
        let repository_path = PathBuf::from(&application.root).join("repository/source.git");
        if repository_path.exists() {
            self.run_git([
                "--git-dir",
                path_text(&repository_path)?,
                "remote",
                "set-url",
                "origin",
                &repository.url,
            ])
            .await?;
            self.run_git([
                "--git-dir",
                path_text(&repository_path)?,
                "fetch",
                "--prune",
                "origin",
            ])
            .await?;
        } else {
            self.run_git([
                "clone",
                "--mirror",
                "--",
                &repository.url,
                path_text(&repository_path)?,
            ])
            .await?;
        }
        let reference = format!("refs/heads/{}", repository.branch);
        let commit_output = self
            .run_git([
                "--git-dir",
                path_text(&repository_path)?,
                "rev-parse",
                "--verify",
                &reference,
            ])
            .await?;
        let commit = String::from_utf8_lossy(&commit_output.stdout)
            .trim()
            .to_owned();
        self.run_git([
            "clone",
            "--quiet",
            "--no-checkout",
            "--",
            path_text(&repository_path)?,
            path_text(release)?,
        ])
        .await?;
        self.run_git([
            "-C",
            path_text(release)?,
            "checkout",
            "--quiet",
            "--detach",
            &commit,
        ])
        .await?;

        if application.runtime == ApplicationRuntime::Php && release.join("composer.json").exists()
        {
            let mut spec = ProcessSpec::new("composer")
                .args([
                    "install",
                    "--no-dev",
                    "--no-interaction",
                    "--prefer-dist",
                    "--optimize-autoloader",
                ])
                .current_dir(release);
            spec.timeout = Duration::from_secs(600);
            self.run(spec).await?;
        }
        let entry_point = match application.runtime {
            ApplicationRuntime::Static => release.join("index.html"),
            ApplicationRuntime::Php => release.join("index.php"),
        };
        if !entry_point.is_file() {
            return Err(LumicError::InvalidInput {
                field: "health".into(),
                message: format!("required entry point {} is missing", entry_point.display()),
            });
        }
        activate(
            application,
            release,
            &release.file_name().unwrap_or_default().to_string_lossy(),
        )?;
        Ok(commit)
    }

    async fn run_git<const N: usize>(&self, args: [&str; N]) -> Result<ProcessOutput> {
        let mut spec = ProcessSpec::new("git").args(args);
        spec.timeout = Duration::from_secs(300);
        self.run(spec).await
    }

    async fn run(&self, spec: ProcessSpec) -> Result<ProcessOutput> {
        let executable = spec.executable.clone();
        let output = self.runner.run(&spec).await?;
        if output.success() {
            Ok(output)
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Err(LumicError::Process {
                executable,
                message: stderr.trim().to_owned(),
            })
        }
    }

    fn upsert_deployment(&self, deployment: &Deployment) -> Result<()> {
        let mut state = self.store.load()?;
        if let Some(existing) = state
            .deployments
            .iter_mut()
            .find(|item| item.id == deployment.id)
        {
            *existing = deployment.clone();
        } else {
            state.deployments.push(deployment.clone());
        }
        self.store.save(&state)
    }

    fn set_health(&self, id: &str, health: &str) -> Result<()> {
        let mut state = self.store.load()?;
        let application = state
            .applications
            .iter_mut()
            .find(|application| application.id == id)
            .ok_or_else(|| not_found(id))?;
        application.health_status = health.into();
        application.updated_at_unix_ms = unix_time_ms();
        self.store.save(&state)
    }

    fn prune_releases(&self, application: &Application) -> Result<()> {
        let current = current_release(application)?;
        let mut successful: Vec<_> = self
            .store
            .load()?
            .deployments
            .into_iter()
            .filter(|item| {
                item.application_id == application.id && item.status == DeploymentStatus::Completed
            })
            .collect();
        successful.reverse();
        let releases_root = PathBuf::from(&application.root).join("releases");
        for deployment in successful.into_iter().skip(application.release_retention) {
            let path = PathBuf::from(deployment.release_path);
            if Some(path.to_string_lossy().as_ref()) == current.as_deref() {
                continue;
            }
            if path.parent() == Some(releases_root.as_path()) && path.is_dir() {
                fs::remove_dir_all(path).map_err(state_io_error)?;
            }
        }
        Ok(())
    }

    fn emit(
        &self,
        event_type: &str,
        id: &str,
        context: &OperationContext,
        payload: serde_json::Value,
    ) -> Result<()> {
        self.events.append(&Event::now(
            event_type,
            &context.actor,
            context.interface,
            "application",
            id,
            &context.correlation_id,
            payload,
        ))
    }
}

fn current_release(application: &Application) -> Result<Option<String>> {
    let current = PathBuf::from(&application.root).join("current");
    match fs::read_link(current) {
        Ok(path) => Ok(Some(path.to_string_lossy().into_owned())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(state_io_error(error)),
    }
}

#[cfg(unix)]
fn activate(application: &Application, release: &Path, suffix: &str) -> Result<()> {
    let root = PathBuf::from(&application.root);
    let temporary = root.join(format!(".current-{suffix}"));
    if temporary.exists() || temporary.is_symlink() {
        fs::remove_file(&temporary).map_err(state_io_error)?;
    }
    symlink(release, &temporary).map_err(state_io_error)?;
    fs::rename(temporary, root.join("current")).map_err(state_io_error)
}

#[cfg(not(unix))]
fn activate(_application: &Application, _release: &Path, _suffix: &str) -> Result<()> {
    Err(LumicError::UnsupportedPlatform {
        platform: "atomic release symlinks require Unix".into(),
    })
}

fn path_text(path: &Path) -> Result<&str> {
    path.to_str().ok_or_else(|| LumicError::InvalidInput {
        field: "path".into(),
        message: "must be valid UTF-8".into(),
    })
}

fn state_io_error(error: std::io::Error) -> LumicError {
    LumicError::Internal {
        message: format!("application state I/O failed: {error}"),
    }
}

fn not_found(id: &str) -> LumicError {
    LumicError::InvalidInput {
        field: "application".into(),
        message: format!("application '{id}' does not exist"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumic_core::{OperationContext, OperationInterface};
    use std::process::Command;

    fn context() -> OperationContext {
        OperationContext {
            actor: "test".into(),
            interface: OperationInterface::Internal,
            correlation_id: "application-test".into(),
            dry_run: false,
            approved: true,
        }
    }

    fn git(repository: &Path, args: &[&str]) {
        let status = Command::new("git")
            .arg("-C")
            .arg(repository)
            .args(args)
            .status()
            .unwrap();
        assert!(status.success(), "git command failed: {args:?}");
    }

    #[tokio::test]
    async fn static_releases_are_persistent_and_rollback_is_atomic() {
        let base = std::env::temp_dir().join(format!(
            "lumic-app-test-{}-{}",
            std::process::id(),
            unix_time_ms()
        ));
        let state = base.join("state");
        let apps = base.join("apps");
        let source = base.join("source");
        fs::create_dir_all(&source).unwrap();
        git(&source, &["init", "--initial-branch=main"]);
        git(&source, &["config", "user.email", "test@lumic.invalid"]);
        git(&source, &["config", "user.name", "Lumic Test"]);
        fs::write(source.join("index.html"), "first").unwrap();
        git(&source, &["add", "index.html"]);
        git(&source, &["commit", "-m", "first"]);

        let service = ApplicationService::new(&state, &apps);
        service
            .create(
                "example",
                "example.com",
                ApplicationRuntime::Static,
                false,
                &context(),
            )
            .unwrap();
        service
            .set_repository(
                "example",
                &format!("file://{}", source.display()),
                "main",
                None,
                &context(),
            )
            .unwrap();
        let first = service.deploy("example", &context()).await.unwrap();

        fs::write(source.join("index.html"), "second").unwrap();
        git(&source, &["add", "index.html"]);
        git(&source, &["commit", "-m", "second"]);
        let second = service.deploy("example", &context()).await.unwrap();
        assert_ne!(first.commit, second.commit);
        assert_eq!(
            fs::read_to_string(apps.join("example/current/index.html")).unwrap(),
            "second"
        );

        service.rollback("example", &context()).unwrap();
        assert_eq!(
            fs::read_to_string(apps.join("example/current/index.html")).unwrap(),
            "first"
        );
        let reloaded = ApplicationService::new(&state, &apps);
        assert_eq!(reloaded.list().unwrap().len(), 1);
        assert_eq!(reloaded.deployments("example").unwrap().len(), 3);
        fs::remove_dir_all(base).unwrap();
    }
}
