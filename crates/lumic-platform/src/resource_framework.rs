//! Shared catalog and resource-graph queries used by CLI, UI, and MCP adapters.

use crate::{framework_state::FrameworkStateStore, resource_lock::ResourceLock};
use lumic_core::{
    LumicError, Result,
    binding::Binding,
    catalog::{ApplicationDefinition, Catalog, RuntimeDefinition, ServiceDefinition},
    pipeline::PipelineExecution,
    resource::{ResourceKind, ResourceRecord, ResourceRef},
    service::ServiceInstance,
};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize)]
pub struct CatalogSnapshot {
    pub services: Vec<ServiceDefinition>,
    pub runtimes: Vec<RuntimeDefinition>,
    pub applications: Vec<ApplicationDefinition>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ResourceInspection {
    Service { service: ServiceInstance },
    Resource { resource: ResourceRecord },
}

#[derive(Debug, Clone, Serialize)]
pub struct BindingMutation {
    pub binding: Binding,
    pub changed: bool,
    pub dry_run: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct BindingRemoval {
    pub binding_id: String,
    pub changed: bool,
    pub dry_run: bool,
}

#[derive(Debug, Clone)]
pub struct ResourceFramework {
    state_dir: PathBuf,
    store: FrameworkStateStore,
}

impl ResourceFramework {
    pub fn at_state_dir(state_dir: impl AsRef<Path>) -> Self {
        let state_dir = state_dir.as_ref().to_path_buf();
        Self {
            store: FrameworkStateStore::at_state_dir(&state_dir),
            state_dir,
        }
    }

    pub fn catalog(&self) -> Result<CatalogSnapshot> {
        let catalog = Catalog::built_in()?;
        Ok(CatalogSnapshot {
            services: catalog.services().cloned().collect(),
            runtimes: catalog.runtimes().cloned().collect(),
            applications: catalog.applications().cloned().collect(),
        })
    }

    pub fn service_schema(&self, definition_id: &str) -> Result<ServiceDefinition> {
        Catalog::built_in()?
            .service(definition_id)
            .cloned()
            .ok_or_else(|| invalid("definition", "unknown service catalog definition"))
    }

    pub fn inspect(&self, resource: &ResourceRef) -> Result<ResourceInspection> {
        let state = self.store.load_or_migrate(now())?;
        match resource.kind {
            ResourceKind::ManagedService => state
                .services
                .into_iter()
                .find(|service| service.id == resource.id)
                .map(|mut service| {
                    if let Some(definition) = Catalog::built_in()
                        .ok()
                        .and_then(|catalog| catalog.service(&service.definition_id).cloned())
                    {
                        service.configuration =
                            definition.configuration.redacted(&service.configuration);
                    }
                    ResourceInspection::Service { service }
                })
                .ok_or_else(|| invalid("resource", "managed service was not found")),
            _ => state
                .resources
                .into_iter()
                .find(|record| record.resource == *resource)
                .map(|mut record| {
                    for output in record
                        .outputs
                        .values_mut()
                        .filter(|output| output.sensitive)
                    {
                        output.value = serde_json::Value::String("[redacted]".into());
                    }
                    ResourceInspection::Resource { resource: record }
                })
                .ok_or_else(|| invalid("resource", "resource was not found")),
        }
    }

    pub fn bindings(&self, resource: Option<&ResourceRef>) -> Result<Vec<Binding>> {
        let state = self.store.load_or_migrate(now())?;
        Ok(state
            .bindings
            .0
            .into_iter()
            .filter(|binding| {
                resource.is_none_or(|resource| {
                    binding.producer == *resource || binding.consumer == *resource
                })
            })
            .collect())
    }

    pub fn operations(&self, resource: Option<&ResourceRef>) -> Result<Vec<PipelineExecution>> {
        let state = self.store.load_or_migrate(now())?;
        Ok(state
            .pipeline_executions
            .into_iter()
            .filter(|execution| resource.is_none_or(|resource| execution.target == *resource))
            .collect())
    }

    pub fn operation(&self, id: &str) -> Result<PipelineExecution> {
        self.store
            .load_or_migrate(now())?
            .pipeline_executions
            .into_iter()
            .find(|execution| execution.id == id)
            .ok_or_else(|| invalid("operation", "pipeline operation was not found"))
    }

    pub fn bind(&self, binding: Binding, dry_run: bool) -> Result<BindingMutation> {
        binding.validate()?;
        let lock_resource = ResourceRef::new(ResourceKind::Pipeline, "resource-bindings")?;
        let _lock = ResourceLock::try_acquire(&self.state_dir, &lock_resource)?;
        let mut state = self.store.load_or_migrate(now())?;
        if let Some(existing) = state.bindings.0.iter().find(|item| item.id == binding.id) {
            if existing.producer == binding.producer
                && existing.output == binding.output
                && existing.consumer == binding.consumer
                && existing.input == binding.input
            {
                return Ok(BindingMutation {
                    binding: existing.clone(),
                    changed: false,
                    dry_run,
                });
            }
            return Err(invalid(
                "binding.id",
                "binding id already describes another relationship",
            ));
        }
        state.bindings.0.push(binding.clone());
        state.validate()?;
        if !dry_run {
            self.store.save(&state)?;
        }
        Ok(BindingMutation {
            binding,
            changed: true,
            dry_run,
        })
    }

    pub fn unbind(&self, binding_id: &str, dry_run: bool) -> Result<BindingRemoval> {
        let lock_resource = ResourceRef::new(ResourceKind::Pipeline, "resource-bindings")?;
        let _lock = ResourceLock::try_acquire(&self.state_dir, &lock_resource)?;
        let mut state = self.store.load_or_migrate(now())?;
        let before = state.bindings.0.len();
        state.bindings.0.retain(|binding| binding.id != binding_id);
        let changed = before != state.bindings.0.len();
        if changed && !dry_run {
            self.store.save(&state)?;
        }
        Ok(BindingRemoval {
            binding_id: binding_id.into(),
            changed,
            dry_run,
        })
    }
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
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
    use lumic_core::resource::{ResourceOutput, ResourceOutputs};

    fn temp_state() -> PathBuf {
        std::env::temp_dir().join(format!("lumic-resource-framework-{}", std::process::id()))
    }

    #[test]
    fn catalog_and_binding_queries_share_validated_state() {
        let directory = temp_state();
        let store = FrameworkStateStore::at_state_dir(&directory);
        let producer = ResourceRef::new(ResourceKind::Application, "producer").unwrap();
        let consumer = ResourceRef::new(ResourceKind::Application, "consumer").unwrap();
        let mut state = crate::framework_state::FrameworkState::default();
        state.resources.push(ResourceRecord {
            resource: producer.clone(),
            attributes: Default::default(),
            outputs: ResourceOutputs::from([(
                "endpoint".into(),
                ResourceOutput {
                    value: serde_json::json!("http://127.0.0.1"),
                    sensitive: false,
                    updated_at_unix_ms: 1,
                },
            )]),
            created_at_unix_ms: 1,
            updated_at_unix_ms: 1,
        });
        state.resources.push(ResourceRecord {
            resource: consumer.clone(),
            attributes: Default::default(),
            outputs: Default::default(),
            created_at_unix_ms: 1,
            updated_at_unix_ms: 1,
        });
        store.save(&state).unwrap();
        let framework = ResourceFramework::at_state_dir(&directory);
        assert!(framework.catalog().unwrap().services.len() >= 6);
        let binding = Binding {
            id: "producer-consumer".into(),
            producer,
            output: "endpoint".into(),
            consumer,
            input: "upstream".into(),
            created_at_unix_ms: 2,
        };
        assert!(framework.bind(binding.clone(), true).unwrap().changed);
        assert!(framework.bind(binding.clone(), false).unwrap().changed);
        let mut replay = binding;
        replay.created_at_unix_ms = 3;
        let replayed = framework.bind(replay, false).unwrap();
        assert!(!replayed.changed);
        assert_eq!(replayed.binding.created_at_unix_ms, 2);
        assert_eq!(framework.bindings(None).unwrap().len(), 1);
        assert!(
            framework
                .unbind("producer-consumer", false)
                .unwrap()
                .changed
        );
        std::fs::remove_dir_all(directory).unwrap();
    }
}
