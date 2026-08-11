use crate::{
    application::ApplicationService,
    audit_store::AuditStore,
    event_store::EventStore,
    framework_state::FrameworkStateStore,
    managed_service::ManagedServiceManager,
    secret_store::SecretStore,
    web::NginxManager,
    wordpress::{WordPressApplyInput, WordPressInstaller},
};
use lumic_core::{
    LumicError, OperationContext, Plan, Result,
    application::{ApplicationServiceReference, unix_time_ms},
    binding::Binding,
    events::{AuditRecord, Event},
    recipe::{
        RecipeApplyResult, RecipeArtifact, RecipeDefinition, RecipeEnvironmentSource,
        RecipeInstallRequest, RecipeInstallation, RecipeInstallationStatus,
        RecipeOperationProgress, RecipeSetupStep, reference_recipes,
    },
    resource::{ResourceKind, ResourceOutput, ResourceOutputs, ResourceRecord, ResourceRef},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct RecipeState {
    version: u32,
    installations: Vec<RecipeInstallation>,
}

#[derive(Debug, Clone)]
pub struct RecipeManager {
    state_dir: PathBuf,
    apps_root: PathBuf,
    definitions: Vec<RecipeDefinition>,
}

impl RecipeManager {
    pub fn at_state_dir(state_dir: impl AsRef<Path>, apps_root: impl Into<PathBuf>) -> Self {
        Self {
            state_dir: state_dir.as_ref().to_path_buf(),
            apps_root: apps_root.into(),
            definitions: reference_recipes(),
        }
    }

    pub fn catalog(&self) -> &[RecipeDefinition] {
        &self.definitions
    }

    pub fn list(&self) -> Result<Vec<RecipeInstallation>> {
        Ok(self.load()?.installations)
    }

    pub fn inspect(&self, application_id: &str) -> Result<RecipeInstallation> {
        self.load()?
            .installations
            .into_iter()
            .find(|item| item.application_id == application_id)
            .ok_or_else(|| LumicError::InvalidInput {
                field: "application".into(),
                message: "recipe installation does not exist".into(),
            })
    }

    pub fn plan_install(&self, request: &RecipeInstallRequest) -> Result<Plan> {
        let recipe = self.definition(&request.recipe_id)?;
        recipe.plan(
            request,
            self.load()?
                .installations
                .iter()
                .any(|item| item.application_id == request.application_id),
        )
    }

    pub async fn install(
        &self,
        request: &RecipeInstallRequest,
        context: &OperationContext,
    ) -> Result<RecipeApplyResult> {
        let recipe = self.definition(&request.recipe_id)?.clone();
        if recipe.metadata.id == "wordpress" {
            return self.install_wordpress(request, context, &recipe).await;
        }
        let mut state = self.load()?;
        let existing_installation = state
            .installations
            .iter()
            .find(|item| item.application_id == request.application_id)
            .cloned();
        let mut resolved = request.clone();
        if let Some(existing) = &existing_installation {
            if resolved.repository_url.is_none() {
                resolved.repository_url.clone_from(&existing.repository_url);
                resolved.branch.clone_from(&existing.branch);
            }
            if resolved.tls_email.is_none() {
                resolved.tls_email.clone_from(&existing.tls_email);
            }
        }
        let plan = recipe.plan(&resolved, existing_installation.is_some())?;
        if context.dry_run {
            return Err(LumicError::InvalidInput {
                field: "dry_run".into(),
                message: format!("use plan_install for read-only planning: {}", plan.summary),
            });
        }
        let apps = ApplicationService::new(&self.state_dir, &self.apps_root);
        let secrets = SecretStore::at_state_dir(&self.state_dir);
        let services = ManagedServiceManager::at_state_dir(&self.state_dir);
        if let Some(existing) = &existing_installation {
            if existing.recipe_id != request.recipe_id || existing.domain != request.domain {
                return Err(LumicError::InvalidInput {
                    field: "application".into(),
                    message: "existing recipe installation has different recipe or domain".into(),
                });
            }
            let managed_services = services.list()?;
            let secrets_converged = existing.secret_references.values().try_fold(
                true,
                |converged, reference| -> Result<bool> {
                    Ok(converged && secrets.exists(reference)?)
                },
            )?;
            let services_converged = existing.service_ids.iter().all(|service_id| {
                managed_services
                    .iter()
                    .any(|service| service.id == *service_id)
            });
            let application = apps.list()?.into_iter().find(|item| {
                item.id == request.application_id
                    && resolved.repository_url.as_ref().is_none_or(|url| {
                        item.repository.as_ref().is_some_and(|repository| {
                            repository.url == *url && repository.branch == resolved.branch
                        })
                    })
                    && (resolved.tls_email.is_none() || item.tls.enabled)
                    && secrets_converged
                    && existing.secret_references.iter().all(|(name, reference)| {
                        item.environment_references.get(name) == Some(reference)
                    })
                    && services_converged
                    && existing.service_ids.iter().all(|service_id| {
                        item.service_references
                            .iter()
                            .any(|reference| reference.service_id == *service_id)
                    })
            });
            if existing.recipe_version == recipe.metadata.version
                && application.is_some()
                && request.environment.is_empty()
            {
                return Ok(RecipeApplyResult {
                    installation: Some(existing.clone()),
                    changed: false,
                    message: "recipe is already converged".into(),
                });
            }
        }
        let existing_app = apps
            .list()?
            .into_iter()
            .find(|item| item.id == request.application_id);
        if let Some(app) = &existing_app {
            if app.domain != request.domain || app.runtime != recipe.runtime {
                return Err(LumicError::InvalidInput {
                    field: "application".into(),
                    message: "existing application does not match recipe domain/runtime".into(),
                });
            }
        } else {
            apps.create(
                &request.application_id,
                &request.domain,
                recipe.runtime,
                false,
                context,
            )?;
        }
        if let Some(url) = &resolved.repository_url {
            apps.set_repository(
                &request.application_id,
                url,
                &resolved.branch,
                None,
                context,
            )?;
        }
        let mut secret_references = BTreeMap::new();
        for environment in &recipe.environment {
            let reference = format!(
                "recipe-{}-{}",
                request.application_id,
                environment.name.to_ascii_lowercase().replace('_', "-")
            );
            match &environment.source {
                RecipeEnvironmentSource::GeneratedSecret => {
                    if !secrets.exists(&reference)? {
                        secrets.create(&reference)?;
                    }
                }
                RecipeEnvironmentSource::Input { required } => {
                    match request.environment.get(&environment.name) {
                        Some(value) => {
                            secrets.put(&reference, value.as_bytes())?;
                        }
                        None if existing_installation.is_some()
                            && secrets.exists(&reference)? => {}
                        None if *required => {
                            return Err(LumicError::InvalidInput {
                                field: "environment".into(),
                                message: format!("{} is required", environment.name),
                            });
                        }
                        None => continue,
                    }
                }
                RecipeEnvironmentSource::Literal { value } => {
                    secrets.put(&reference, value.as_bytes())?;
                }
            }
            apps.set_environment_reference(
                &request.application_id,
                &environment.name,
                &reference,
                context,
            )?;
            secret_references.insert(environment.name.clone(), reference);
        }
        let mut service_ids = Vec::new();
        for requirement in &recipe.services {
            let service_id = format!("{}-{}", request.application_id, requirement.id_suffix);
            if services
                .list()?
                .iter()
                .all(|service| service.id != service_id)
            {
                services
                    .install(&service_id, requirement.kind, context)
                    .await?;
            }
            services.attach_to_application(
                &apps,
                &request.application_id,
                ApplicationServiceReference {
                    service_id: service_id.clone(),
                    role: requirement.role.clone(),
                    service_type: None,
                    database: None,
                    user: None,
                    secret_reference: None,
                },
                context,
            )?;
            service_ids.push(service_id);
        }
        apps.provision_versioned(
            &request.application_id,
            recipe.runtime_version.as_deref(),
            &recipe.components,
            context,
        )
        .await?;
        for step in &recipe.setup {
            match step {
                RecipeSetupStep::HealthCheck { path, port } => {
                    apps.set_health_check(&request.application_id, path, *port, context)?;
                }
                RecipeSetupStep::Process { process } => {
                    apps.add_process(&request.application_id, process.clone(), context)
                        .await?;
                }
                RecipeSetupStep::Deploy => {
                    apps.deploy(&request.application_id, context).await?;
                }
                RecipeSetupStep::WordPress { .. } => {
                    return Err(LumicError::Internal {
                        message: "WordPress setup reached the generic recipe executor".into(),
                    });
                }
            }
        }
        if let Some(email) = &resolved.tls_email {
            apps.enable_tls(&request.application_id, email, context)
                .await?;
        }
        let now = unix_time_ms();
        let installed_at = state
            .installations
            .iter()
            .find(|item| item.application_id == request.application_id)
            .map(|item| item.installed_at_unix_ms)
            .unwrap_or(now);
        let installation = RecipeInstallation {
            recipe_id: recipe.metadata.id.clone(),
            recipe_version: recipe.metadata.version.clone(),
            application_id: request.application_id.clone(),
            domain: request.domain.clone(),
            repository_url: resolved.repository_url,
            branch: resolved.branch,
            tls_email: resolved.tls_email,
            secret_references,
            service_ids,
            owned_resources: vec![format!("application:{}", request.application_id)],
            binding_ids: Vec::new(),
            artifacts: BTreeMap::new(),
            operation: None,
            status: RecipeInstallationStatus::Installed,
            installed_at_unix_ms: installed_at,
            updated_at_unix_ms: now,
        };
        if let Some(existing) = state
            .installations
            .iter_mut()
            .find(|item| item.application_id == request.application_id)
        {
            *existing = installation.clone();
        } else {
            state.installations.push(installation.clone());
        }
        self.save(&state)?;
        self.record("recipe.installed", "install", &installation, context)?;
        Ok(RecipeApplyResult {
            installation: Some(installation),
            changed: true,
            message: "recipe installation converged".into(),
        })
    }

    async fn install_wordpress(
        &self,
        request: &RecipeInstallRequest,
        context: &OperationContext,
        recipe: &RecipeDefinition,
    ) -> Result<RecipeApplyResult> {
        let mut state = self.load()?;
        let existing = state
            .installations
            .iter()
            .find(|item| item.application_id == request.application_id)
            .cloned();
        recipe.plan(request, existing.is_some())?;
        if context.dry_run {
            return Err(LumicError::InvalidInput {
                field: "dry_run".into(),
                message: "use recipe plan for read-only planning".into(),
            });
        }
        if let Some(installed) = &existing
            && (installed.recipe_id != "wordpress" || installed.domain != request.domain)
        {
            return Err(LumicError::InvalidInput {
                field: "application".into(),
                message: "existing recipe installation has different recipe or domain".into(),
            });
        }
        let (source, wp_cli) = wordpress_artifacts(recipe)?;
        let apps = ApplicationService::new(&self.state_dir, &self.apps_root);
        let secrets = SecretStore::at_state_dir(&self.state_dir);
        if let Some(installed) = &existing {
            let installer = WordPressInstaller::new(&self.state_dir, &self.apps_root);
            let secrets_exist = installed
                .secret_references
                .values()
                .try_fold(true, |ok, reference| {
                    Ok::<bool, LumicError>(ok && secrets.exists(reference)?)
                })?;
            if installed.recipe_version == recipe.metadata.version
                && installed.status == RecipeInstallationStatus::Installed
                && request.environment.is_empty()
                && secrets_exist
                && apps
                    .list()?
                    .iter()
                    .any(|app| app.id == request.application_id && app.web_configured)
                && installer
                    .is_installed(&request.application_id, wp_cli)
                    .await?
            {
                return Ok(RecipeApplyResult {
                    installation: Some(installed.clone()),
                    changed: false,
                    message: "recipe is already converged".into(),
                });
            }
        }

        let now = unix_time_ms();
        let execution_id = format!("wordpress-{}-{now}", request.application_id);
        let progress = RecipeOperationProgress {
            execution_id,
            current_step: Some("application".into()),
            ..Default::default()
        };
        let placeholder = RecipeInstallation {
            recipe_id: "wordpress".into(),
            recipe_version: recipe.metadata.version.clone(),
            application_id: request.application_id.clone(),
            domain: request.domain.clone(),
            repository_url: None,
            branch: request.branch.clone(),
            tls_email: request
                .tls_email
                .clone()
                .or_else(|| existing.as_ref().and_then(|item| item.tls_email.clone())),
            secret_references: existing
                .as_ref()
                .map(|item| item.secret_references.clone())
                .unwrap_or_default(),
            service_ids: existing
                .as_ref()
                .map(|item| item.service_ids.clone())
                .unwrap_or_default(),
            owned_resources: existing
                .as_ref()
                .map(|item| item.owned_resources.clone())
                .unwrap_or_default(),
            binding_ids: existing
                .as_ref()
                .map(|item| item.binding_ids.clone())
                .unwrap_or_default(),
            artifacts: existing
                .as_ref()
                .map(|item| item.artifacts.clone())
                .unwrap_or_default(),
            operation: Some(progress),
            status: RecipeInstallationStatus::Updating,
            installed_at_unix_ms: existing
                .as_ref()
                .map(|item| item.installed_at_unix_ms)
                .unwrap_or(now),
            updated_at_unix_ms: now,
        };
        upsert_installation(&mut state.installations, placeholder);
        self.save(&state)?;

        let applied = self
            .apply_wordpress(request, context, recipe, source, wp_cli)
            .await;
        if let Err(error) = applied {
            let mut failed = self.load()?;
            if let Some(installation) = failed
                .installations
                .iter_mut()
                .find(|item| item.application_id == request.application_id)
            {
                installation.status = RecipeInstallationStatus::Failed;
                installation.updated_at_unix_ms = unix_time_ms();
                if let Some(progress) = &mut installation.operation {
                    progress.failure = Some(error.to_string());
                }
            }
            self.save(&failed)?;
            return Err(error);
        }
        let installation = applied.expect("error handled above");
        self.record("recipe.installed", "install", &installation, context)?;
        Ok(RecipeApplyResult {
            installation: Some(installation),
            changed: true,
            message: "WordPress recipe installation converged".into(),
        })
    }

    async fn apply_wordpress(
        &self,
        request: &RecipeInstallRequest,
        context: &OperationContext,
        recipe: &RecipeDefinition,
        source: &RecipeArtifact,
        wp_cli: &RecipeArtifact,
    ) -> Result<RecipeInstallation> {
        let apps = ApplicationService::new(&self.state_dir, &self.apps_root);
        let secrets = SecretStore::at_state_dir(&self.state_dir);
        let services = ManagedServiceManager::at_state_dir(&self.state_dir);
        match apps
            .list()?
            .into_iter()
            .find(|item| item.id == request.application_id)
        {
            Some(app) if app.domain == request.domain && app.runtime == recipe.runtime => {}
            Some(_) => {
                return Err(LumicError::InvalidInput {
                    field: "application".into(),
                    message: "existing application does not match WordPress domain/runtime".into(),
                });
            }
            None => {
                apps.create(
                    &request.application_id,
                    &request.domain,
                    recipe.runtime,
                    false,
                    context,
                )?;
            }
        }
        self.complete_wordpress_step(&request.application_id, "application", "secrets")?;

        let mut secret_references = BTreeMap::new();
        for environment in &recipe.environment {
            let reference = format!(
                "recipe-{}-{}",
                request.application_id,
                environment.name.to_ascii_lowercase().replace('_', "-")
            );
            match &environment.source {
                RecipeEnvironmentSource::GeneratedSecret => {
                    if !secrets.exists(&reference)? {
                        secrets.create(&reference)?;
                    }
                }
                RecipeEnvironmentSource::Input { required } => {
                    match request.environment.get(&environment.name) {
                        Some(value) => {
                            secrets.put(&reference, value.as_bytes())?;
                        }
                        None if secrets.exists(&reference)? => {}
                        None if *required => {
                            return Err(LumicError::InvalidInput {
                                field: "environment".into(),
                                message: format!("{} is required", environment.name),
                            });
                        }
                        None => continue,
                    }
                }
                RecipeEnvironmentSource::Literal { value } => {
                    secrets.put(&reference, value.as_bytes())?;
                }
            }
            apps.set_environment_reference(
                &request.application_id,
                &environment.name,
                &reference,
                context,
            )?;
            secret_references.insert(environment.name.clone(), reference);
        }
        self.complete_wordpress_step(&request.application_id, "secrets", "database")?;

        let service_id = format!("{}-mysql", request.application_id);
        if services
            .list()?
            .iter()
            .all(|service| service.id != service_id)
        {
            services
                .install(
                    &service_id,
                    lumic_core::managed_service::ManagedServiceKind::Mysql,
                    context,
                )
                .await?;
        }
        let prefix = database_prefix(&request.application_id);
        let database = format!("{prefix}_wp");
        let database_user = format!("{prefix}_user");
        services
            .create_database(&service_id, &database, None, context)
            .await?;
        let user = services
            .create_database_user(&service_id, &database_user, context)
            .await?;
        services
            .grant_database(&service_id, &database, &database_user, context)
            .await?;
        services.attach_to_application(
            &apps,
            &request.application_id,
            ApplicationServiceReference {
                service_id: service_id.clone(),
                role: "database".into(),
                service_type: None,
                database: Some(database.clone()),
                user: Some(database_user.clone()),
                secret_reference: None,
            },
            context,
        )?;
        self.complete_wordpress_step(&request.application_id, "database", "runtime")?;

        apps.provision_versioned(
            &request.application_id,
            Some("8.3"),
            &recipe.components,
            context,
        )
        .await?;
        self.complete_wordpress_step(&request.application_id, "runtime", "wordpress")?;

        let read = |name: &str| -> Result<Vec<u8>> {
            let reference = secret_references
                .get(name)
                .ok_or_else(|| LumicError::Internal {
                    message: format!("missing recipe secret reference for {name}"),
                })?;
            secrets.read(reference)
        };
        let site_title = String::from_utf8(read("WORDPRESS_SITE_TITLE")?).map_err(secret_utf8)?;
        let admin_user = String::from_utf8(read("WORDPRESS_ADMIN_USER")?).map_err(secret_utf8)?;
        let admin_email = String::from_utf8(read("WORDPRESS_ADMIN_EMAIL")?).map_err(secret_utf8)?;
        let admin_password = read("WORDPRESS_ADMIN_PASSWORD")?;
        let database_password = secrets.read(&user.secret_reference)?;
        let installer = WordPressInstaller::new(&self.state_dir, &self.apps_root);
        let result = installer
            .apply(
                &WordPressApplyInput {
                    application_id: &request.application_id,
                    domain: &request.domain,
                    site_title: &site_title,
                    admin_user: &admin_user,
                    admin_email: &admin_email,
                    admin_password: &admin_password,
                    database: &database,
                    database_user: &database_user,
                    database_password: &database_password,
                },
                source,
                wp_cli,
            )
            .await?;
        persist_wordpress_artifacts(
            &self.state_dir,
            &request.application_id,
            source,
            wp_cli,
            &result.source_artifact,
            &result.wp_cli_artifact,
        )?;
        self.complete_wordpress_step(&request.application_id, "wordpress", "health")?;

        apps.set_health_check(&request.application_id, "/wp-login.php", 80, context)?;
        let tls_email = request.tls_email.clone().or_else(|| {
            self.inspect(&request.application_id)
                .ok()
                .and_then(|item| item.tls_email)
        });
        if let Some(email) = &tls_email {
            apps.enable_tls(&request.application_id, email, context)
                .await?;
        }
        self.complete_wordpress_step(&request.application_id, "health", "commit")?;

        let mut state = self.load()?;
        let installation = state
            .installations
            .iter_mut()
            .find(|item| item.application_id == request.application_id)
            .ok_or_else(|| LumicError::Internal {
                message: "WordPress progress state disappeared".into(),
            })?;
        installation.secret_references = secret_references;
        installation.service_ids = vec![service_id.clone()];
        installation.owned_resources = vec![
            format!("application:{}", request.application_id),
            format!("service_resource:database.{service_id}-{database}"),
            format!("service_resource:database-user.{service_id}-{database_user}"),
            format!("service_resource:nginx.web-host.{}", request.application_id),
        ];
        installation.binding_ids = framework_binding_ids(&self.state_dir, &request.application_id)?;
        installation.artifacts = BTreeMap::from([
            (source.id.clone(), source.version.clone()),
            (wp_cli.id.clone(), wp_cli.version.clone()),
        ]);
        installation.status = RecipeInstallationStatus::Installed;
        installation.updated_at_unix_ms = unix_time_ms();
        if let Some(progress) = &mut installation.operation {
            progress.completed_steps.push("commit".into());
            progress.current_step = None;
            progress.failure = None;
        }
        let installation = installation.clone();
        self.save(&state)?;
        Ok(installation)
    }

    fn complete_wordpress_step(
        &self,
        application_id: &str,
        completed: &str,
        next: &str,
    ) -> Result<()> {
        let mut state = self.load()?;
        let installation = state
            .installations
            .iter_mut()
            .find(|item| item.application_id == application_id)
            .ok_or_else(|| LumicError::Internal {
                message: "WordPress progress state disappeared".into(),
            })?;
        let progress = installation
            .operation
            .as_mut()
            .ok_or_else(|| LumicError::Internal {
                message: "WordPress operation journal is missing".into(),
            })?;
        if !progress
            .completed_steps
            .iter()
            .any(|step| step == completed)
        {
            progress.completed_steps.push(completed.into());
        }
        progress.current_step = Some(next.into());
        installation.updated_at_unix_ms = unix_time_ms();
        self.save(&state)
    }

    pub async fn update(
        &self,
        application_id: &str,
        context: &OperationContext,
    ) -> Result<RecipeApplyResult> {
        let installed = self.inspect(application_id)?;
        self.install(
            &RecipeInstallRequest {
                recipe_id: installed.recipe_id,
                application_id: installed.application_id,
                domain: installed.domain,
                repository_url: installed.repository_url,
                branch: installed.branch,
                tls_email: installed.tls_email,
                environment: BTreeMap::new(),
            },
            context,
        )
        .await
    }

    pub fn uninstall(
        &self,
        application_id: &str,
        context: &OperationContext,
    ) -> Result<RecipeApplyResult> {
        let mut state = self.load()?;
        let Some(index) = state
            .installations
            .iter()
            .position(|item| item.application_id == application_id)
        else {
            return Ok(RecipeApplyResult {
                installation: None,
                changed: false,
                message: "recipe installation is already absent".into(),
            });
        };
        let installation = state.installations.remove(index);
        let apps = ApplicationService::new(&self.state_dir, &self.apps_root);
        if apps.list()?.iter().any(|item| item.id == application_id) {
            NginxManager::system(&self.state_dir).remove_configuration(application_id)?;
            apps.delete(application_id, context)?;
        }
        let secrets = SecretStore::at_state_dir(&self.state_dir);
        for reference in installation.secret_references.values() {
            secrets.delete(reference)?;
        }
        cleanup_wordpress_resources(&self.state_dir, application_id)?;
        self.save(&state)?;
        self.record("recipe.uninstalled", "uninstall", &installation, context)?;
        Ok(RecipeApplyResult {
            installation: Some(installation),
            changed: true,
            message: "recipe metadata removed; application data moved to Lumic trash".into(),
        })
    }

    fn definition(&self, id: &str) -> Result<&RecipeDefinition> {
        self.definitions
            .iter()
            .find(|item| item.metadata.id == id)
            .ok_or_else(|| LumicError::InvalidInput {
                field: "recipe".into(),
                message: "unknown recipe identifier".into(),
            })
    }
    fn path(&self) -> PathBuf {
        self.state_dir.join("recipes.json")
    }
    fn load(&self) -> Result<RecipeState> {
        let path = self.path();
        if !path.exists() {
            return Ok(RecipeState {
                version: 1,
                ..Default::default()
            });
        }
        serde_json::from_slice(&fs::read(path).map_err(io)?).map_err(|error| LumicError::Internal {
            message: format!("recipe state is invalid: {error}"),
        })
    }
    fn save(&self, state: &RecipeState) -> Result<()> {
        let bytes = serde_json::to_vec_pretty(state).map_err(|error| LumicError::Internal {
            message: format!("could not serialize recipe state: {error}"),
        })?;
        crate::atomic_file::write_atomic(&self.path(), &bytes, 0o600).map(|_| ())
    }
    fn record(
        &self,
        event_type: &str,
        operation: &str,
        installation: &RecipeInstallation,
        context: &OperationContext,
    ) -> Result<()> {
        EventStore::at_state_dir(&self.state_dir).append(&Event::now(
            event_type,
            &context.actor,
            context.interface,
            "recipe_installation",
            &installation.application_id,
            &context.correlation_id,
            json!({"recipe_id":installation.recipe_id,"version":installation.recipe_version}),
        ))?;
        AuditStore::at_state_dir(&self.state_dir).append(&AuditRecord::now(
            context,
            "recipe.apply",
            operation,
            "recipe_installation",
            &installation.application_id,
            json!({"recipe_id":installation.recipe_id,"version":installation.recipe_version}),
            None,
            Some(json!(installation)),
            true,
            "recipe lifecycle applied",
        ))
    }
}

fn wordpress_artifacts(recipe: &RecipeDefinition) -> Result<(&RecipeArtifact, &RecipeArtifact)> {
    recipe
        .setup
        .iter()
        .find_map(|step| match step {
            RecipeSetupStep::WordPress { source, wp_cli } => Some((source, wp_cli)),
            _ => None,
        })
        .ok_or_else(|| LumicError::Internal {
            message: "WordPress recipe has no artifact setup step".into(),
        })
}

fn database_prefix(application_id: &str) -> String {
    application_id
        .chars()
        .take(48)
        .map(|character| if character == '-' { '_' } else { character })
        .collect()
}

fn upsert_installation(
    installations: &mut Vec<RecipeInstallation>,
    installation: RecipeInstallation,
) {
    if let Some(existing) = installations
        .iter_mut()
        .find(|item| item.application_id == installation.application_id)
    {
        *existing = installation;
    } else {
        installations.push(installation);
    }
}

fn secret_utf8(error: std::string::FromUtf8Error) -> LumicError {
    LumicError::Internal {
        message: format!("recipe secret is not UTF-8: {error}"),
    }
}

fn persist_wordpress_artifacts(
    state_dir: &Path,
    application_id: &str,
    source: &RecipeArtifact,
    wp_cli: &RecipeArtifact,
    source_path: &Path,
    wp_cli_path: &Path,
) -> Result<()> {
    let store = FrameworkStateStore::at_state_dir(state_dir);
    let now = u64::try_from(unix_time_ms()).unwrap_or(u64::MAX);
    let mut state = store.load_or_migrate(now)?;
    let application = ResourceRef::new(ResourceKind::Application, application_id)?;
    for (artifact, path, input) in [
        (source, source_path, "wordpress_source"),
        (wp_cli, wp_cli_path, "wp_cli"),
    ] {
        let resource = ResourceRef::new(
            ResourceKind::Artifact,
            format!("{}.{}", artifact.id, artifact.version),
        )?;
        let record = ResourceRecord {
            resource: resource.clone(),
            attributes: BTreeMap::from([
                ("version".into(), Value::String(artifact.version.clone())),
                ("url".into(), Value::String(artifact.url.clone())),
                ("sha256".into(), Value::String(artifact.sha256.clone())),
                ("ownership".into(), Value::String("lumic_cache".into())),
            ]),
            outputs: ResourceOutputs::from([(
                "path".into(),
                ResourceOutput {
                    value: Value::String(path.to_string_lossy().into_owned()),
                    sensitive: false,
                    updated_at_unix_ms: now,
                },
            )]),
            created_at_unix_ms: now,
            updated_at_unix_ms: now,
        };
        if let Some(existing) = state
            .resources
            .iter_mut()
            .find(|item| item.resource == resource)
        {
            let created = existing.created_at_unix_ms;
            *existing = ResourceRecord {
                created_at_unix_ms: created,
                ..record
            };
        } else {
            state.resources.push(record);
        }
        let id = format!("artifact-{}-to-{application_id}", artifact.id);
        state.bindings.0.retain(|binding| {
            binding.id != id && !(binding.consumer == application && binding.input == input)
        });
        state.bindings.0.push(Binding {
            id,
            producer: resource,
            output: "path".into(),
            consumer: application.clone(),
            input: input.into(),
            created_at_unix_ms: now,
        });
    }
    store.save(&state)
}

fn framework_binding_ids(state_dir: &Path, application_id: &str) -> Result<Vec<String>> {
    let state = FrameworkStateStore::at_state_dir(state_dir).load()?;
    let application = ResourceRef::new(ResourceKind::Application, application_id)?;
    let web_host = ResourceRef::new(
        ResourceKind::ServiceResource,
        format!("nginx.web-host.{application_id}"),
    )?;
    Ok(state
        .bindings
        .0
        .iter()
        .filter(|binding| {
            binding.producer == application
                || binding.consumer == application
                || binding.producer == web_host
                || binding.consumer == web_host
        })
        .map(|binding| binding.id.clone())
        .collect())
}

fn cleanup_wordpress_resources(state_dir: &Path, application_id: &str) -> Result<()> {
    let store = FrameworkStateStore::at_state_dir(state_dir);
    let mut state = store.load()?;
    let application = ResourceRef::new(ResourceKind::Application, application_id)?;
    let web_host = ResourceRef::new(
        ResourceKind::ServiceResource,
        format!("nginx.web-host.{application_id}"),
    )?;
    state.bindings.0.retain(|binding| {
        binding.producer != application
            && binding.consumer != application
            && binding.producer != web_host
            && binding.consumer != web_host
    });
    state
        .resources
        .retain(|resource| resource.resource != application && resource.resource != web_host);
    store.save(&state)
}

fn io(error: std::io::Error) -> LumicError {
    LumicError::Internal {
        message: format!("recipe state I/O failed: {error}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn catalog_and_plan_do_not_mutate_state() {
        let dir = std::env::temp_dir().join(format!("lumic-recipe-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let manager = RecipeManager::at_state_dir(&dir, dir.join("apps"));
        let catalog = manager.catalog();
        assert_eq!(catalog.len(), 8);
        assert!(catalog.iter().any(|recipe| recipe.metadata.id == "laravel"));
        assert!(catalog.iter().any(|recipe| recipe.metadata.id == "ghost"));
        let plan = manager
            .plan_install(&RecipeInstallRequest {
                recipe_id: "static-git".into(),
                application_id: "demo".into(),
                domain: "demo.example.com".into(),
                repository_url: Some("https://example.com/demo.git".into()),
                branch: "main".into(),
                tls_email: None,
                environment: BTreeMap::new(),
            })
            .unwrap();
        assert!(plan.summary.contains("static-git"));
        assert!(!dir.join("recipes.json").exists());
        let result = manager
            .uninstall(
                "absent",
                &OperationContext {
                    actor: "test".into(),
                    interface: lumic_core::OperationInterface::Cli,
                    correlation_id: "recipe-test".into(),
                    dry_run: false,
                    approved: true,
                },
            )
            .unwrap();
        assert!(!result.changed);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn wordpress_uninstall_removes_owned_state_but_retains_shared_artifacts() {
        let dir = std::env::temp_dir().join(format!(
            "lumic-recipe-wordpress-uninstall-{}-{}",
            std::process::id(),
            unix_time_ms()
        ));
        fs::create_dir_all(&dir).unwrap();
        let apps_root = dir.join("apps");
        let manager = RecipeManager::at_state_dir(&dir, &apps_root);
        let context = OperationContext {
            actor: "test".into(),
            interface: lumic_core::OperationInterface::Cli,
            correlation_id: "wordpress-uninstall-test".into(),
            dry_run: false,
            approved: true,
        };
        ApplicationService::new(&dir, &apps_root)
            .create(
                "blog",
                "blog.example.com",
                lumic_core::application::ApplicationRuntime::Php,
                false,
                &context,
            )
            .unwrap();
        let secrets = SecretStore::at_state_dir(&dir);
        secrets.put("recipe-blog-admin", b"private").unwrap();
        let now = u64::try_from(unix_time_ms()).unwrap();
        let application = ResourceRef::new(ResourceKind::Application, "blog").unwrap();
        let artifact = ResourceRef::new(ResourceKind::Artifact, "wordpress.6.8.2").unwrap();
        let mut framework = crate::framework_state::FrameworkState::default();
        framework.resources.push(ResourceRecord {
            resource: application.clone(),
            attributes: BTreeMap::new(),
            outputs: ResourceOutputs::new(),
            created_at_unix_ms: now,
            updated_at_unix_ms: now,
        });
        framework.resources.push(ResourceRecord {
            resource: artifact.clone(),
            attributes: BTreeMap::from([("ownership".into(), Value::String("lumic_cache".into()))]),
            outputs: ResourceOutputs::from([(
                "path".into(),
                ResourceOutput {
                    value: Value::String("/cache/wordpress.tar.gz".into()),
                    sensitive: false,
                    updated_at_unix_ms: now,
                },
            )]),
            created_at_unix_ms: now,
            updated_at_unix_ms: now,
        });
        framework.bindings.0.push(Binding {
            id: "artifact-wordpress-to-blog".into(),
            producer: artifact.clone(),
            output: "path".into(),
            consumer: application,
            input: "wordpress_source".into(),
            created_at_unix_ms: now,
        });
        FrameworkStateStore::at_state_dir(&dir)
            .save(&framework)
            .unwrap();
        manager
            .save(&RecipeState {
                version: 1,
                installations: vec![RecipeInstallation {
                    recipe_id: "wordpress".into(),
                    recipe_version: "1.0.0".into(),
                    application_id: "blog".into(),
                    domain: "blog.example.com".into(),
                    repository_url: None,
                    branch: "main".into(),
                    tls_email: None,
                    secret_references: BTreeMap::from([(
                        "WORDPRESS_ADMIN_PASSWORD".into(),
                        "recipe-blog-admin".into(),
                    )]),
                    service_ids: Vec::new(),
                    owned_resources: vec!["application:blog".into()],
                    binding_ids: vec!["artifact-wordpress-to-blog".into()],
                    artifacts: BTreeMap::from([("wordpress".into(), "6.8.2".into())]),
                    operation: None,
                    status: RecipeInstallationStatus::Installed,
                    installed_at_unix_ms: u128::from(now),
                    updated_at_unix_ms: u128::from(now),
                }],
            })
            .unwrap();

        assert!(manager.uninstall("blog", &context).unwrap().changed);
        assert!(!secrets.exists("recipe-blog-admin").unwrap());
        let framework = FrameworkStateStore::at_state_dir(&dir).load().unwrap();
        assert!(
            framework
                .resources
                .iter()
                .any(|item| item.resource == artifact)
        );
        assert!(
            framework
                .resources
                .iter()
                .all(|item| item.resource.id != "blog")
        );
        assert!(framework.bindings.0.is_empty());
        assert!(
            apps_root
                .join(".trash")
                .read_dir()
                .unwrap()
                .next()
                .is_some()
        );
        fs::remove_dir_all(dir).unwrap();
    }
}
