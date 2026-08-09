use crate::{
    application::ApplicationService, audit_store::AuditStore, event_store::EventStore,
    managed_service::ManagedServiceManager, secret_store::SecretStore,
};
use lumic_core::{
    LumicError, OperationContext, Plan, Result,
    application::{ApplicationServiceReference, unix_time_ms},
    events::{AuditRecord, Event},
    recipe::{
        RecipeApplyResult, RecipeDefinition, RecipeEnvironmentSource, RecipeInstallRequest,
        RecipeInstallation, RecipeInstallationStatus, RecipeSetupStep, reference_recipes,
    },
};
use serde::{Deserialize, Serialize};
use serde_json::json;
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
                    database: None,
                    user: None,
                    secret_reference: None,
                },
                context,
            )?;
            service_ids.push(service_id);
        }
        apps.provision(&request.application_id, &recipe.components, context)
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
            apps.delete(application_id, context)?;
        }
        let secrets = SecretStore::at_state_dir(&self.state_dir);
        for reference in installation.secret_references.values() {
            secrets.delete(reference)?;
        }
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
        assert_eq!(manager.catalog().len(), 1);
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
}
