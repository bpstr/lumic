use crate::{
    ProcessRunner, ProcessSpec, application::ApplicationService, atomic_file::write_atomic,
    audit_store::AuditStore, event_store::EventStore, hex_encode, secret_store::SecretStore,
};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use lumic_core::{
    LumicError, OperationContext, Result,
    application::{
        Application, Deployment, unix_time_ms, validate_branch, validate_repository_url,
        validate_slug,
    },
    events::{AuditRecord, Event},
    infrastructure::{
        ConfigurationDiff, CoordinatedDeployment, CoordinationStatus, DeploymentMember,
        DeploymentMemberStatus, EnvironmentBundle, EnvironmentTier, EnvironmentTransform,
        HostedRepository, InfrastructureReadModel, MembershipKind, NodeEnrollment, NodeHealth,
        NodeIdentity, NodeMembership, NodeRole, PortableApplication, PushDeployTrigger,
        RegisteredNode, RemoteOperation, RepositoryMirror, ResourceEndpoint, SignedRemoteRequest,
        TrustStatus, validate_endpoint, validate_secret_reference,
    },
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::{net::TcpStream, time::timeout};

const STATE_VERSION: u32 = 1;
const BUNDLE_VERSION: u32 = 1;
const SIGNING_KEY_REFERENCE: &str = "node-signing-key";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct InfrastructureState {
    version: u32,
    local_node: Option<NodeIdentity>,
    #[serde(default)]
    repositories: Vec<HostedRepository>,
    #[serde(default)]
    mirrors: Vec<RepositoryMirror>,
    #[serde(default)]
    triggers: Vec<PushDeployTrigger>,
    #[serde(default)]
    environments: Vec<EnvironmentBundle>,
    #[serde(default)]
    nodes: Vec<RegisteredNode>,
    #[serde(default)]
    endpoints: Vec<ResourceEndpoint>,
    #[serde(default)]
    memberships: Vec<NodeMembership>,
    #[serde(default)]
    deployments: Vec<CoordinatedDeployment>,
    #[serde(default)]
    consumed_nonces: BTreeMap<String, u128>,
}

#[derive(Debug, Clone)]
struct InfrastructureStore {
    path: PathBuf,
}

impl InfrastructureStore {
    fn at_state_dir(state_dir: impl AsRef<Path>) -> Self {
        Self {
            path: state_dir.as_ref().join("infrastructure.json"),
        }
    }

    fn load(&self) -> Result<InfrastructureState> {
        if !self.path.exists() {
            return Ok(InfrastructureState {
                version: STATE_VERSION,
                ..InfrastructureState::default()
            });
        }
        let bytes = fs::read(&self.path).map_err(state_io)?;
        let state: InfrastructureState = serde_json::from_slice(&bytes).map_err(state_json)?;
        if state.version != STATE_VERSION {
            return Err(LumicError::Internal {
                message: format!(
                    "unsupported infrastructure state version {}; expected {STATE_VERSION}",
                    state.version
                ),
            });
        }
        Ok(state)
    }

    fn save(&self, state: &InfrastructureState) -> Result<()> {
        let mut bytes = serde_json::to_vec_pretty(state).map_err(state_json)?;
        bytes.push(b'\n');
        write_atomic(&self.path, &bytes, 0o600).map(|_| ())
    }
}

#[derive(Debug, Clone)]
pub struct InfrastructureService {
    state_dir: PathBuf,
    git_root: PathBuf,
    store: InfrastructureStore,
    secrets: SecretStore,
    events: EventStore,
    audit: AuditStore,
    applications: ApplicationService,
    runner: ProcessRunner,
}

impl InfrastructureService {
    pub fn new(state_dir: impl AsRef<Path>, apps_root: impl Into<PathBuf>) -> Self {
        let state_dir = state_dir.as_ref().to_path_buf();
        Self {
            git_root: state_dir.join("git"),
            store: InfrastructureStore::at_state_dir(&state_dir),
            secrets: SecretStore::at_state_dir(&state_dir),
            events: EventStore::at_state_dir(&state_dir),
            audit: AuditStore::at_state_dir(&state_dir),
            applications: ApplicationService::new(&state_dir, apps_root),
            state_dir,
            runner: ProcessRunner,
        }
    }

    pub fn initialize_node(
        &self,
        id: &str,
        name: &str,
        mut roles: Vec<NodeRole>,
        context: &OperationContext,
    ) -> Result<NodeIdentity> {
        authorize(context)?;
        validate_slug("node", id)?;
        if name.trim().is_empty() || name.len() > 128 || name.contains(['\n', '\r']) {
            return Err(invalid("name", "must be a non-empty single-line node name"));
        }
        if roles.is_empty() {
            return Err(invalid("roles", "at least one node role is required"));
        }
        roles.sort_unstable();
        roles.dedup();
        let mut state = self.store.load()?;
        if let Some(existing) = state.local_node {
            if existing.id == id && existing.name == name && existing.roles == roles {
                return Ok(existing);
            }
            return Err(invalid(
                "node",
                "node identity is already initialized and cannot be replaced implicitly",
            ));
        }
        if !self.secrets.exists(SIGNING_KEY_REFERENCE)? {
            self.secrets.create(SIGNING_KEY_REFERENCE)?;
        }
        let signing_key = self.signing_key()?;
        let identity = NodeIdentity {
            id: id.into(),
            name: name.into(),
            fingerprint: fingerprint(&signing_key.verifying_key().to_bytes()),
            roles,
            created_at_unix_ms: unix_time_ms(),
        };
        state.local_node = Some(identity.clone());
        self.store.save(&state)?;
        self.record(
            context,
            "infrastructure.node.initialize",
            "initialize",
            "node",
            id,
            json!({"roles": identity.roles}),
            None,
            serde_json::to_value(&identity).ok(),
            "node identity initialized",
        )?;
        Ok(identity)
    }

    pub fn enrollment(&self, endpoint: &str) -> Result<NodeEnrollment> {
        validate_endpoint(endpoint)?;
        let identity = self.local_identity()?;
        let verification_key = hex_encode(&self.signing_key()?.verifying_key().to_bytes());
        Ok(NodeEnrollment {
            identity,
            endpoint: endpoint.into(),
            verification_key,
        })
    }

    pub fn register_node(
        &self,
        enrollment: NodeEnrollment,
        context: &OperationContext,
    ) -> Result<RegisteredNode> {
        authorize(context)?;
        validate_endpoint(&enrollment.endpoint)?;
        validate_slug("node", &enrollment.identity.id)?;
        if enrollment.identity.name.trim().is_empty()
            || enrollment.identity.name.len() > 128
            || enrollment.identity.name.contains(['\n', '\r'])
            || enrollment.identity.roles.is_empty()
        {
            return Err(invalid(
                "identity",
                "peer name and at least one declared role are required",
            ));
        }
        if enrollment.identity.id == self.local_identity()?.id {
            return Err(invalid("node", "cannot register the local node as a peer"));
        }
        let key = decode_fixed::<32>(&enrollment.verification_key, "verification_key")?;
        VerifyingKey::from_bytes(&key)
            .map_err(|_| invalid("verification_key", "is not a valid Ed25519 public key"))?;
        if fingerprint(&key) != enrollment.identity.fingerprint {
            return Err(invalid(
                "fingerprint",
                "does not match the supplied verification key",
            ));
        }
        let mut state = self.store.load()?;
        let before = state
            .nodes
            .iter()
            .find(|node| node.identity.id == enrollment.identity.id)
            .cloned();
        let node = RegisteredNode {
            identity: enrollment.identity,
            endpoint: enrollment.endpoint,
            trust_status: TrustStatus::Trusted,
            verification_key: enrollment.verification_key,
            registered_at_unix_ms: unix_time_ms(),
            last_health: before.as_ref().and_then(|node| node.last_health.clone()),
        };
        upsert_by(&mut state.nodes, node.clone(), |item| &item.identity.id);
        self.store.save(&state)?;
        self.record(
            context,
            "infrastructure.node.trust",
            "register",
            "node",
            &node.identity.id,
            json!({"endpoint": node.endpoint, "fingerprint": node.identity.fingerprint}),
            before.and_then(|value| serde_json::to_value(value).ok()),
            serde_json::to_value(&node).ok(),
            "peer node registered and trusted",
        )?;
        Ok(node)
    }

    pub fn revoke_node(&self, id: &str, context: &OperationContext) -> Result<RegisteredNode> {
        authorize(context)?;
        validate_slug("node", id)?;
        let mut state = self.store.load()?;
        let node = state
            .nodes
            .iter_mut()
            .find(|node| node.identity.id == id)
            .ok_or_else(|| not_found("node", id))?;
        let before = node.clone();
        node.trust_status = TrustStatus::Revoked;
        let node = node.clone();
        self.store.save(&state)?;
        self.record(
            context,
            "infrastructure.node.trust",
            "revoke",
            "node",
            id,
            json!({}),
            serde_json::to_value(before).ok(),
            serde_json::to_value(&node).ok(),
            "peer node trust revoked",
        )?;
        Ok(node)
    }

    pub fn read_model(&self) -> Result<InfrastructureReadModel> {
        let state = self.store.load()?;
        Ok(InfrastructureReadModel {
            local_node: state.local_node,
            repositories: state.repositories,
            mirrors: state.mirrors,
            triggers: state.triggers,
            environments: state.environments,
            nodes: state.nodes,
            endpoints: state.endpoints,
            memberships: state.memberships,
            deployments: state.deployments,
        })
    }

    pub async fn create_hosted_repository(
        &self,
        id: &str,
        default_branch: &str,
        context: &OperationContext,
    ) -> Result<HostedRepository> {
        authorize(context)?;
        validate_slug("repository", id)?;
        validate_branch(default_branch)?;
        let mut state = self.store.load()?;
        if let Some(existing) = state.repositories.iter().find(|repo| repo.id == id) {
            if existing.default_branch == default_branch && Path::new(&existing.path).is_dir() {
                return Ok(existing.clone());
            }
            return Err(invalid("repository", "repository id is already in use"));
        }
        let path = self.git_root.join("hosted").join(format!("{id}.git"));
        if path.exists() {
            return Err(invalid(
                "repository",
                "repository path already exists outside the infrastructure registry",
            ));
        }
        let output = self
            .runner
            .run(&ProcessSpec::new("git").args([
                "init".into(),
                "--bare".into(),
                format!("--initial-branch={default_branch}"),
                path_text(&path)?,
            ]))
            .await?;
        ensure_success("git", &output)?;
        let repository = HostedRepository {
            id: id.into(),
            path: path_text(&path)?,
            default_branch: default_branch.into(),
            created_at_unix_ms: unix_time_ms(),
        };
        state.repositories.push(repository.clone());
        self.store.save(&state)?;
        self.record(
            context,
            "infrastructure.git.host",
            "create",
            "repository",
            id,
            json!({"default_branch": default_branch}),
            None,
            serde_json::to_value(&repository).ok(),
            "bare Git repository created",
        )?;
        Ok(repository)
    }

    pub async fn sync_mirror(
        &self,
        id: &str,
        source_url: &str,
        branch: &str,
        credential_reference: Option<String>,
        context: &OperationContext,
    ) -> Result<RepositoryMirror> {
        authorize(context)?;
        validate_slug("mirror", id)?;
        validate_repository_url(source_url)?;
        validate_branch(branch)?;
        if let Some(reference) = credential_reference.as_deref() {
            validate_secret_reference(reference)?;
            let credential = self.state_dir.join("credentials").join(reference);
            if !credential.is_file() || credential.is_symlink() {
                return Err(invalid(
                    "credential_reference",
                    "must identify an imported regular credential file",
                ));
            }
        }
        let mut state = self.store.load()?;
        let path = self.git_root.join("mirrors").join(format!("{id}.git"));
        let mut spec = if path.exists() {
            ensure_success(
                "git",
                &self
                    .runner
                    .run(&ProcessSpec::new("git").args([
                        "--git-dir".into(),
                        path_text(&path)?,
                        "remote".into(),
                        "set-url".into(),
                        "origin".into(),
                        source_url.into(),
                    ]))
                    .await?,
            )?;
            ProcessSpec::new("git").args([
                "--git-dir".into(),
                path_text(&path)?,
                "remote".into(),
                "update".into(),
                "--prune".into(),
            ])
        } else {
            ProcessSpec::new("git").args([
                "clone".into(),
                "--mirror".into(),
                "--".into(),
                source_url.into(),
                path_text(&path)?,
            ])
        };
        if let Some(reference) = credential_reference.as_deref() {
            let credential = self.state_dir.join("credentials").join(reference);
            spec = spec.environment(
                "GIT_SSH_COMMAND",
                format!(
                    "ssh -i {} -o IdentitiesOnly=yes",
                    shell_quote_path(&credential)?
                ),
            );
        }
        ensure_success("git", &self.runner.run(&spec).await?)?;
        let mirror = RepositoryMirror {
            id: id.into(),
            source_url: source_url.into(),
            branch: branch.into(),
            credential_reference,
            path: path_text(&path)?,
            last_updated_at_unix_ms: unix_time_ms(),
        };
        upsert_by(&mut state.mirrors, mirror.clone(), |item| &item.id);
        self.store.save(&state)?;
        self.record(
            context,
            "infrastructure.git.mirror",
            "sync",
            "mirror",
            id,
            json!({"source_url": source_url, "branch": branch, "credential": mirror.credential_reference.as_ref().map(|_| "redacted")}),
            None,
            serde_json::to_value(&mirror).ok(),
            "Git mirror synchronized",
        )?;
        Ok(mirror)
    }

    pub fn set_push_trigger(
        &self,
        repository_id: &str,
        application_id: &str,
        branch: &str,
        enabled: bool,
        context: &OperationContext,
    ) -> Result<PushDeployTrigger> {
        authorize(context)?;
        validate_slug("repository", repository_id)?;
        validate_slug("application", application_id)?;
        validate_branch(branch)?;
        self.applications.inspect(application_id)?;
        let mut state = self.store.load()?;
        let repository = state
            .repositories
            .iter()
            .find(|repo| repo.id == repository_id)
            .ok_or_else(|| not_found("repository", repository_id))?;
        let hook_path = Path::new(&repository.path).join("hooks/post-receive");
        let hook = format!("#!/bin/sh\nexec /usr/local/bin/lumic git receive {repository_id}\n");
        write_atomic(&hook_path, hook.as_bytes(), 0o755)?;
        let trigger = PushDeployTrigger {
            repository_id: repository_id.into(),
            application_id: application_id.into(),
            branch: branch.into(),
            enabled,
        };
        upsert_by(&mut state.triggers, trigger.clone(), |item| {
            &item.repository_id
        });
        self.store.save(&state)?;
        self.record(
            context,
            "infrastructure.git.push_deploy",
            "configure",
            "repository",
            repository_id,
            json!({"application_id": application_id, "branch": branch, "enabled": enabled}),
            None,
            serde_json::to_value(&trigger).ok(),
            "push-to-deploy trigger configured",
        )?;
        Ok(trigger)
    }

    pub async fn receive_push(
        &self,
        repository_id: &str,
        updates: &str,
        context: &OperationContext,
    ) -> Result<Option<Deployment>> {
        authorize(context)?;
        validate_slug("repository", repository_id)?;
        if updates.len() > 128 * 1024 || updates.contains('\0') {
            return Err(invalid(
                "updates",
                "Git receive input is invalid or too large",
            ));
        }
        let state = self.store.load()?;
        let trigger = state
            .triggers
            .iter()
            .find(|item| item.repository_id == repository_id && item.enabled)
            .cloned();
        let Some(trigger) = trigger else {
            return Ok(None);
        };
        let target_ref = format!("refs/heads/{}", trigger.branch);
        let matched = updates.lines().any(|line| {
            let fields = line.split_whitespace().collect::<Vec<_>>();
            fields.len() == 3
                && fields[0].len() == 40
                && fields[1].len() == 40
                && fields[2] == target_ref
                && fields[0]
                    .bytes()
                    .chain(fields[1].bytes())
                    .all(|byte| byte.is_ascii_hexdigit())
        });
        if !matched {
            return Ok(None);
        }
        self.applications
            .deploy(&trigger.application_id, context)
            .await
            .map(Some)
    }

    pub fn generate_secret(&self, reference: &str, context: &OperationContext) -> Result<String> {
        authorize(context)?;
        validate_secret_reference(reference)?;
        let created = self.secrets.create(reference)?;
        self.record(
            context,
            "infrastructure.secret.generate",
            "generate",
            "secret",
            reference,
            json!({"value": "not_logged"}),
            None,
            Some(json!({"configured": true})),
            "target-local random secret generated",
        )?;
        Ok(created)
    }

    pub fn export_environment(
        &self,
        application_id: &str,
        environment_id: &str,
        tier: EnvironmentTier,
        context: &OperationContext,
    ) -> Result<EnvironmentBundle> {
        authorize(context)?;
        validate_slug("application", application_id)?;
        validate_slug("environment", environment_id)?;
        let application = self.applications.inspect(application_id)?;
        let local = self.local_identity()?;
        let bundle = EnvironmentBundle {
            schema_version: BUNDLE_VERSION,
            id: environment_id.into(),
            tier,
            source_node_id: local.id,
            application: portable(&application),
            exported_at_unix_ms: unix_time_ms(),
        };
        let mut state = self.store.load()?;
        upsert_by(&mut state.environments, bundle.clone(), |item| &item.id);
        self.store.save(&state)?;
        self.record(
            context,
            "infrastructure.environment.export",
            "export",
            "environment",
            environment_id,
            json!({"application_id": application_id, "tier": tier, "secret_values": "not_exported"}),
            None,
            serde_json::to_value(&bundle).ok(),
            "portable environment bundle exported",
        )?;
        Ok(bundle)
    }

    pub fn import_environment(
        &self,
        bundle: &EnvironmentBundle,
        transform: &EnvironmentTransform,
        context: &OperationContext,
    ) -> Result<Application> {
        authorize(context)?;
        if bundle.schema_version != BUNDLE_VERSION {
            return Err(invalid(
                "schema_version",
                "unsupported environment bundle version",
            ));
        }
        validate_slug("environment", &transform.target_id)?;
        lumic_core::application::validate_domain(&transform.target_domain)?;
        for reference in transform.environment_reference_overrides.values() {
            validate_secret_reference(reference)?;
            if !self.secrets.exists(reference)? {
                return Err(invalid(
                    "environment_reference_overrides",
                    "every overridden secret reference must exist on the target node",
                ));
            }
        }
        let target_application_id = transform.target_id.clone();
        let mut configuration = bundle.application.clone();
        configuration.id = target_application_id.clone();
        configuration.name = target_application_id.clone();
        configuration.domain = transform.target_domain.clone();
        for (name, reference) in &transform.environment_reference_overrides {
            if !configuration.environment_references.contains_key(name) {
                return Err(invalid(
                    "environment_reference_overrides",
                    "override names must exist in the source bundle",
                ));
            }
            configuration
                .environment_references
                .insert(name.clone(), reference.clone());
        }
        if let Some(reference) = configuration
            .repository
            .as_ref()
            .and_then(|repository| repository.credential_reference.as_ref())
            && !self.secrets.exists(reference)?
        {
            return Err(invalid(
                "credential_reference",
                "the repository credential reference must exist on the target node",
            ));
        }
        for reference in configuration.environment_references.values() {
            if !self.secrets.exists(reference)? {
                return Err(invalid(
                    "environment_references",
                    "every secret reference must exist on the target node",
                ));
            }
        }
        for reference in configuration
            .service_references
            .iter()
            .filter_map(|service| service.secret_reference.as_ref())
        {
            if !self.secrets.exists(reference)? {
                return Err(invalid(
                    "service_references",
                    "every service secret reference must exist on the target node",
                ));
            }
        }
        for service in &mut configuration.service_references {
            if let Some(target) = transform.service_id_overrides.get(&service.service_id) {
                service.service_id = target.clone();
            }
        }
        let target_exists = self
            .applications
            .list()?
            .iter()
            .any(|application| application.id == target_application_id);
        if !target_exists {
            self.applications.create(
                &target_application_id,
                &configuration.domain,
                configuration.runtime,
                configuration.www_alias,
                context,
            )?;
        }
        let application = self.applications.apply_portable_configuration(
            &target_application_id,
            &configuration,
            context,
        )?;
        let imported = EnvironmentBundle {
            schema_version: BUNDLE_VERSION,
            id: transform.target_id.clone(),
            tier: transform.target_tier,
            source_node_id: bundle.source_node_id.clone(),
            application: configuration,
            exported_at_unix_ms: unix_time_ms(),
        };
        let mut state = self.store.load()?;
        upsert_by(&mut state.environments, imported, |item| &item.id);
        self.store.save(&state)?;
        Ok(application)
    }

    pub fn diff_environments(
        &self,
        source: &EnvironmentBundle,
        target: &EnvironmentBundle,
    ) -> Vec<ConfigurationDiff> {
        let mut diffs = Vec::new();
        diff_value(
            &mut diffs,
            "tier",
            source.tier.as_str(),
            target.tier.as_str(),
            false,
        );
        diff_value(
            &mut diffs,
            "domain",
            &source.application.domain,
            &target.application.domain,
            false,
        );
        diff_value(
            &mut diffs,
            "runtime",
            runtime_name(source.application.runtime),
            runtime_name(target.application.runtime),
            false,
        );
        let names = source
            .application
            .environment_references
            .keys()
            .chain(target.application.environment_references.keys())
            .cloned()
            .collect::<BTreeSet<_>>();
        for name in names {
            let source_value = source.application.environment_references.get(&name);
            let target_value = target.application.environment_references.get(&name);
            if source_value != target_value {
                diffs.push(ConfigurationDiff {
                    field: format!("environment.{name}"),
                    source: source_value.map(|_| "configured".into()),
                    target: target_value.map(|_| "configured".into()),
                    sensitive: true,
                });
            }
        }
        let source_services = source
            .application
            .service_references
            .iter()
            .map(|item| (item.role.clone(), item.service_id.clone()))
            .collect::<BTreeMap<_, _>>();
        let target_services = target
            .application
            .service_references
            .iter()
            .map(|item| (item.role.clone(), item.service_id.clone()))
            .collect::<BTreeMap<_, _>>();
        for role in source_services
            .keys()
            .chain(target_services.keys())
            .cloned()
            .collect::<BTreeSet<_>>()
        {
            let source_value = source_services.get(&role).cloned();
            let target_value = target_services.get(&role).cloned();
            if source_value != target_value {
                diffs.push(ConfigurationDiff {
                    field: format!("service.{role}"),
                    source: source_value,
                    target: target_value,
                    sensitive: false,
                });
            }
        }
        diffs
    }

    pub fn register_endpoint(
        &self,
        endpoint: ResourceEndpoint,
        context: &OperationContext,
    ) -> Result<ResourceEndpoint> {
        authorize(context)?;
        validate_slug("endpoint", &endpoint.id)?;
        validate_slug("provider_node", &endpoint.provider_node_id)?;
        validate_slug("provider_kind", &endpoint.provider_kind)?;
        validate_slug("provider_id", &endpoint.provider_id)?;
        validate_slug("consumer_node", &endpoint.consumer_node_id)?;
        validate_slug("consumer_kind", &endpoint.consumer_kind)?;
        validate_slug("consumer_id", &endpoint.consumer_id)?;
        if endpoint.port == 0
            || !matches!(endpoint.protocol.as_str(), "tcp" | "http" | "https")
            || endpoint.host.is_empty()
            || endpoint.host.len() > 253
            || endpoint.host.contains(['\n', '\r', '\0', '/', '@'])
        {
            return Err(invalid(
                "endpoint",
                "contains an invalid protocol, host, or port",
            ));
        }
        if let Some(reference) = endpoint.secret_reference.as_deref() {
            validate_secret_reference(reference)?;
            if !self.secrets.exists(reference)? {
                return Err(invalid(
                    "secret_reference",
                    "must exist on the node registering the endpoint",
                ));
            }
        }
        if endpoint.health_path.as_deref().is_some_and(|path| {
            !path.starts_with('/') || path.len() > 512 || path.contains(['\n', '\r', '\0'])
        }) {
            return Err(invalid(
                "health_path",
                "must be an absolute path without control characters",
            ));
        }
        let mut state = self.store.load()?;
        for node_id in [&endpoint.provider_node_id, &endpoint.consumer_node_id] {
            if !node_is_known(&state, node_id) {
                return Err(invalid(
                    "endpoint",
                    "provider and consumer nodes must be local or explicitly trusted",
                ));
            }
        }
        upsert_by(&mut state.endpoints, endpoint.clone(), |item| &item.id);
        self.store.save(&state)?;
        self.record(
            context,
            "infrastructure.endpoint.register",
            "register",
            "endpoint",
            &endpoint.id,
            json!({"provider_node_id": endpoint.provider_node_id, "consumer_node_id": endpoint.consumer_node_id, "protocol": endpoint.protocol, "host": endpoint.host, "port": endpoint.port, "secret_reference": endpoint.secret_reference.as_ref().map(|_| "redacted")}),
            None,
            serde_json::to_value(&endpoint).ok(),
            "resource endpoint registered",
        )?;
        Ok(endpoint)
    }

    pub fn register_membership(
        &self,
        kind: MembershipKind,
        environment_id: &str,
        application_id: &str,
        node_id: &str,
        enabled: bool,
        context: &OperationContext,
    ) -> Result<NodeMembership> {
        authorize(context)?;
        validate_slug("environment", environment_id)?;
        validate_slug("application", application_id)?;
        validate_slug("node", node_id)?;
        let mut state = self.store.load()?;
        if !node_is_known(&state, node_id) {
            return Err(invalid(
                "node",
                "membership node must be local or explicitly trusted",
            ));
        }
        let id = format!(
            "{}-{application_id}-{node_id}",
            match kind {
                MembershipKind::Worker => "worker",
                MembershipKind::ReverseProxy => "proxy",
            }
        );
        let membership = NodeMembership {
            id,
            kind,
            environment_id: environment_id.into(),
            application_id: application_id.into(),
            node_id: node_id.into(),
            enabled,
        };
        upsert_by(&mut state.memberships, membership.clone(), |item| &item.id);
        self.store.save(&state)?;
        self.record(
            context,
            "infrastructure.membership.configure",
            "configure",
            "membership",
            &membership.id,
            json!({"kind": kind, "enabled": enabled}),
            None,
            serde_json::to_value(&membership).ok(),
            "node membership configured",
        )?;
        Ok(membership)
    }

    pub async fn check_node_health(&self, id: &str) -> Result<NodeHealth> {
        validate_slug("node", id)?;
        let mut state = self.store.load()?;
        let node = state
            .nodes
            .iter_mut()
            .find(|node| node.identity.id == id && node.trust_status == TrustStatus::Trusted)
            .ok_or_else(|| not_found("trusted node", id))?;
        let (host, port) = endpoint_host_port(&node.endpoint)?;
        let result = timeout(
            Duration::from_secs(3),
            TcpStream::connect((host.as_str(), port)),
        )
        .await;
        let health = match result {
            Ok(Ok(_)) => NodeHealth {
                healthy: true,
                message: "remote Lumic endpoint accepted a TCP connection".into(),
                checked_at_unix_ms: unix_time_ms(),
            },
            Ok(Err(error)) => NodeHealth {
                healthy: false,
                message: format!("remote endpoint connection failed: {error}"),
                checked_at_unix_ms: unix_time_ms(),
            },
            Err(_) => NodeHealth {
                healthy: false,
                message: "remote endpoint connection timed out".into(),
                checked_at_unix_ms: unix_time_ms(),
            },
        };
        node.last_health = Some(health.clone());
        self.store.save(&state)?;
        Ok(health)
    }

    pub fn begin_coordination(
        &self,
        environment_id: &str,
        members: Vec<(String, String)>,
        context: &OperationContext,
    ) -> Result<CoordinatedDeployment> {
        authorize(context)?;
        validate_slug("environment", environment_id)?;
        if members.is_empty() {
            return Err(invalid(
                "members",
                "at least one deployment member is required",
            ));
        }
        let state = self.store.load()?;
        let mut unique_nodes = BTreeSet::new();
        for (node, application) in &members {
            validate_slug("node", node)?;
            validate_slug("application", application)?;
            if !unique_nodes.insert(node) {
                return Err(invalid(
                    "members",
                    "each node may appear only once in a coordination",
                ));
            }
            if !node_is_known(&state, node) {
                return Err(invalid(
                    "members",
                    "every member node must be local or trusted",
                ));
            }
        }
        drop(state);
        let deployment = CoordinatedDeployment {
            id: format!("coordination-{}-{}", unix_time_ms(), std::process::id()),
            environment_id: environment_id.into(),
            members: members
                .into_iter()
                .map(|(node_id, application_id)| DeploymentMember {
                    node_id,
                    application_id,
                    status: DeploymentMemberStatus::Pending,
                    healthy: None,
                    deployment_id: None,
                    message: "awaiting an explicit node-local deployment".into(),
                })
                .collect(),
            status: CoordinationStatus::Planned,
            failure_boundary: "Stop unstarted members after the first failure; rollback only members changed by this coordination, using each node's normal rollback contract.".into(),
            created_at_unix_ms: unix_time_ms(),
            finished_at_unix_ms: None,
        };
        let mut state = self.store.load()?;
        state.deployments.push(deployment.clone());
        self.store.save(&state)?;
        self.record(
            context,
            "infrastructure.deployment.coordinate",
            "begin",
            "coordinated_deployment",
            &deployment.id,
            json!({"environment_id": environment_id, "members": deployment.members.len()}),
            None,
            serde_json::to_value(&deployment).ok(),
            "coordinated deployment planned",
        )?;
        Ok(deployment)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn report_coordination_member(
        &self,
        coordination_id: &str,
        node_id: &str,
        status: DeploymentMemberStatus,
        healthy: Option<bool>,
        deployment_id: Option<String>,
        message: String,
        context: &OperationContext,
    ) -> Result<CoordinatedDeployment> {
        authorize(context)?;
        validate_slug("coordination", coordination_id)?;
        validate_slug("node", node_id)?;
        if let Some(deployment_id) = deployment_id.as_deref() {
            validate_slug("deployment", deployment_id)?;
        }
        if message.len() > 512 || message.contains(['\n', '\r']) {
            return Err(invalid("message", "must be a short single-line status"));
        }
        let mut state = self.store.load()?;
        let coordination = state
            .deployments
            .iter_mut()
            .find(|item| item.id == coordination_id)
            .ok_or_else(|| not_found("coordinated deployment", coordination_id))?;
        let member = coordination
            .members
            .iter_mut()
            .find(|item| item.node_id == node_id)
            .ok_or_else(|| not_found("deployment member", node_id))?;
        if coordination.status == CoordinationStatus::Failed
            && member.status == DeploymentMemberStatus::Pending
            && status != DeploymentMemberStatus::RolledBack
        {
            return Err(invalid(
                "status",
                "unstarted members cannot advance after the coordination has failed",
            ));
        }
        member.status = status;
        member.healthy = healthy;
        member.deployment_id = deployment_id;
        member.message = message;
        coordination.status = if coordination.members.iter().any(|item| {
            item.status == DeploymentMemberStatus::Failed || item.healthy == Some(false)
        }) {
            coordination.finished_at_unix_ms = Some(unix_time_ms());
            CoordinationStatus::Failed
        } else if coordination.members.iter().all(|item| {
            item.status == DeploymentMemberStatus::Succeeded && item.healthy == Some(true)
        }) {
            coordination.finished_at_unix_ms = Some(unix_time_ms());
            CoordinationStatus::Succeeded
        } else if coordination.status == CoordinationStatus::Failed {
            CoordinationStatus::Failed
        } else {
            CoordinationStatus::Running
        };
        let coordination = coordination.clone();
        self.store.save(&state)?;
        self.record(
            context,
            "infrastructure.deployment.coordinate",
            "report_member",
            "coordinated_deployment",
            coordination_id,
            json!({"node_id": node_id, "status": status, "healthy": healthy}),
            None,
            serde_json::to_value(&coordination).ok(),
            "coordinated deployment member updated",
        )?;
        Ok(coordination)
    }

    pub fn sign_remote_request(
        &self,
        target_node_id: &str,
        operation: RemoteOperation,
        ttl_seconds: u64,
    ) -> Result<SignedRemoteRequest> {
        validate_slug("target_node", target_node_id)?;
        validate_remote_operation(&operation)?;
        if ttl_seconds == 0 || ttl_seconds > 300 {
            return Err(invalid("ttl_seconds", "must be between 1 and 300"));
        }
        let state = self.store.load()?;
        if !state.nodes.iter().any(|node| {
            node.identity.id == target_node_id && node.trust_status == TrustStatus::Trusted
        }) {
            return Err(invalid(
                "target_node",
                "remote operations can only target an explicitly trusted node",
            ));
        }
        let local = self.local_identity()?;
        let random = self.random_nonce()?;
        let mut request = SignedRemoteRequest {
            origin_node_id: local.id,
            target_node_id: target_node_id.into(),
            nonce: random,
            expires_at_unix_ms: unix_time_ms() + u128::from(ttl_seconds) * 1_000,
            operation,
            signature: String::new(),
        };
        let payload = signing_payload(&request)?;
        request.signature = hex_encode(&self.signing_key()?.sign(&payload).to_bytes());
        Ok(request)
    }

    pub fn verify_remote_request(
        &self,
        request: &SignedRemoteRequest,
        context: &OperationContext,
    ) -> Result<()> {
        authorize(context)?;
        validate_remote_operation(&request.operation)?;
        validate_slug("origin_node", &request.origin_node_id)?;
        validate_slug("target_node", &request.target_node_id)?;
        if request.nonce.len() != 64 || !request.nonce.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(invalid("nonce", "must be a 32-byte hexadecimal nonce"));
        }
        if request.target_node_id != self.local_identity()?.id {
            return Err(invalid(
                "target_node_id",
                "request targets a different node",
            ));
        }
        let now = unix_time_ms();
        if request.expires_at_unix_ms < now || request.expires_at_unix_ms > now + 300_000 {
            return Err(invalid(
                "expires_at_unix_ms",
                "request is expired or too far in the future",
            ));
        }
        let mut state = self.store.load()?;
        state
            .consumed_nonces
            .retain(|_, expires_at| *expires_at >= now);
        if state.consumed_nonces.contains_key(&request.nonce) {
            return Err(invalid("nonce", "request has already been consumed"));
        }
        let node = state
            .nodes
            .iter()
            .find(|node| {
                node.identity.id == request.origin_node_id
                    && node.trust_status == TrustStatus::Trusted
            })
            .ok_or_else(|| not_found("trusted origin node", &request.origin_node_id))?;
        let key = decode_fixed::<32>(&node.verification_key, "verification_key")?;
        let verifying_key = VerifyingKey::from_bytes(&key)
            .map_err(|_| invalid("verification_key", "registered key is invalid"))?;
        let signature_bytes = decode_fixed::<64>(&request.signature, "signature")?;
        verifying_key
            .verify(
                &signing_payload(request)?,
                &Signature::from_bytes(&signature_bytes),
            )
            .map_err(|_| invalid("signature", "remote request signature is invalid"))?;
        if state.consumed_nonces.len() >= 10_000 {
            return Err(invalid(
                "nonce",
                "replay cache is full; retry after current requests expire",
            ));
        }
        state
            .consumed_nonces
            .insert(request.nonce.clone(), request.expires_at_unix_ms);
        self.store.save(&state)
    }

    pub async fn execute_remote_request(
        &self,
        request: &SignedRemoteRequest,
        context: &OperationContext,
    ) -> Result<Deployment> {
        self.verify_remote_request(request, context)?;
        let result = match request.operation.kind.as_str() {
            "application.deploy" => {
                self.applications
                    .deploy(&request.operation.resource_id, context)
                    .await
            }
            "application.rollback" => self
                .applications
                .rollback(&request.operation.resource_id, context),
            _ => Err(invalid("operation", "unsupported remote operation")),
        };
        self.audit.append(&AuditRecord::now(
            context,
            "infrastructure.remote.apply",
            &request.operation.kind,
            "application",
            &request.operation.resource_id,
            json!({"origin_node_id": request.origin_node_id, "arguments": request.operation.arguments}),
            None,
            result.as_ref().ok().and_then(|value| serde_json::to_value(value).ok()),
            result.is_ok(),
            result
                .as_ref()
                .map(|_| "signed remote operation applied".into())
                .unwrap_or_else(|error| error.to_string()),
        ))?;
        result
    }

    fn local_identity(&self) -> Result<NodeIdentity> {
        self.store
            .load()?
            .local_node
            .ok_or_else(|| invalid("node", "node identity has not been initialized"))
    }

    fn signing_key(&self) -> Result<SigningKey> {
        let secret = self.secrets.read(SIGNING_KEY_REFERENCE)?;
        let text = std::str::from_utf8(&secret)
            .map_err(|_| invalid("node_signing_key", "stored key is not UTF-8 hex"))?;
        let bytes = decode_fixed::<32>(text, "node_signing_key")?;
        Ok(SigningKey::from_bytes(&bytes))
    }

    fn random_nonce(&self) -> Result<String> {
        let reference = format!("nonce-{}-{}", unix_time_ms(), std::process::id());
        self.secrets.create(&reference)?;
        let secret = self.secrets.read(&reference)?;
        self.secrets.delete(&reference)?;
        String::from_utf8(secret).map_err(|_| invalid("nonce", "random source was not UTF-8"))
    }

    #[allow(clippy::too_many_arguments)]
    fn record(
        &self,
        context: &OperationContext,
        capability: &str,
        operation: &str,
        entity: &str,
        entity_id: &str,
        arguments: serde_json::Value,
        before: Option<serde_json::Value>,
        after: Option<serde_json::Value>,
        message: &str,
    ) -> Result<()> {
        self.audit.append(&AuditRecord::now(
            context, capability, operation, entity, entity_id, arguments, before, after, true,
            message,
        ))?;
        self.events.append(&Event::now(
            format!("{entity}.{operation}"),
            &context.actor,
            context.interface,
            entity,
            entity_id,
            &context.correlation_id,
            json!({"capability": capability, "message": message}),
        ))
    }
}

fn portable(application: &Application) -> PortableApplication {
    PortableApplication {
        id: application.id.clone(),
        name: application.name.clone(),
        domain: application.domain.clone(),
        www_alias: application.www_alias,
        runtime: application.runtime,
        repository: application.repository.clone(),
        environment_references: application.environment_references.clone(),
        service_references: application.service_references.clone(),
        health_check: application.health_check.clone(),
        processes: application.processes.clone(),
        release_retention: application.release_retention,
    }
}

fn validate_remote_operation(operation: &RemoteOperation) -> Result<()> {
    if !matches!(
        operation.kind.as_str(),
        "application.deploy" | "application.rollback"
    ) {
        return Err(invalid(
            "operation",
            "remote operations are limited to application.deploy and application.rollback",
        ));
    }
    validate_slug("resource_id", &operation.resource_id)?;
    if !operation.arguments.is_empty() {
        return Err(invalid(
            "arguments",
            "the supported remote operations do not accept arbitrary arguments",
        ));
    }
    Ok(())
}

fn signing_payload(request: &SignedRemoteRequest) -> Result<Vec<u8>> {
    serde_json::to_vec(&(
        &request.origin_node_id,
        &request.target_node_id,
        &request.nonce,
        request.expires_at_unix_ms,
        &request.operation,
    ))
    .map_err(state_json)
}

fn endpoint_host_port(endpoint: &str) -> Result<(String, u16)> {
    validate_endpoint(endpoint)?;
    let (scheme, rest) = endpoint
        .split_once("://")
        .ok_or_else(|| invalid("endpoint", "missing URL scheme"))?;
    let authority = rest.split('/').next().unwrap_or(rest);
    if let Some(bracketed) = authority.strip_prefix('[') {
        let (host, suffix) = bracketed
            .split_once(']')
            .ok_or_else(|| invalid("endpoint", "invalid IPv6 authority"))?;
        let port = suffix
            .strip_prefix(':')
            .map(str::parse)
            .transpose()
            .map_err(|_| invalid("endpoint", "invalid port"))?
            .unwrap_or(if scheme == "https" { 443 } else { 80 });
        return Ok((host.into(), port));
    }
    let (host, port) = authority
        .rsplit_once(':')
        .filter(|(_, port)| port.bytes().all(|byte| byte.is_ascii_digit()))
        .map_or_else(
            || {
                Ok((
                    authority.to_owned(),
                    if scheme == "https" { 443 } else { 80 },
                ))
            },
            |(host, port)| {
                let port = port
                    .parse()
                    .map_err(|_| invalid("endpoint", "invalid port"))?;
                Ok((host.to_owned(), port))
            },
        )?;
    if host.is_empty() || port == 0 {
        return Err(invalid("endpoint", "invalid host or port"));
    }
    Ok((host, port))
}

fn diff_value(
    diffs: &mut Vec<ConfigurationDiff>,
    field: &str,
    source: &str,
    target: &str,
    sensitive: bool,
) {
    if source != target {
        diffs.push(ConfigurationDiff {
            field: field.into(),
            source: Some(source.into()),
            target: Some(target.into()),
            sensitive,
        });
    }
}

fn runtime_name(runtime: lumic_core::application::ApplicationRuntime) -> &'static str {
    match runtime {
        lumic_core::application::ApplicationRuntime::Static => "static",
        lumic_core::application::ApplicationRuntime::Php => "php",
        lumic_core::application::ApplicationRuntime::Node => "node",
    }
}

fn fingerprint(bytes: &[u8]) -> String {
    format!("sha256:{}", hex_encode(&Sha256::digest(bytes)))
}

fn decode_fixed<const N: usize>(value: &str, field: &str) -> Result<[u8; N]> {
    if value.len() != N * 2 {
        return Err(invalid(field, "has an invalid hexadecimal length"));
    }
    let mut output = [0_u8; N];
    for (index, pair) in value.as_bytes().chunks_exact(2).enumerate() {
        let high = hex_digit(pair[0]).ok_or_else(|| invalid(field, "must be hexadecimal"))?;
        let low = hex_digit(pair[1]).ok_or_else(|| invalid(field, "must be hexadecimal"))?;
        output[index] = high << 4 | low;
    }
    Ok(output)
}

const fn hex_digit(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

fn shell_quote_path(path: &Path) -> Result<String> {
    let value = path_text(path)?;
    if value.contains(['\n', '\r', '\0', '\'']) {
        return Err(invalid(
            "credential",
            "credential path cannot be safely quoted",
        ));
    }
    Ok(format!("'{value}'"))
}

fn path_text(path: &Path) -> Result<String> {
    path.to_str()
        .map(str::to_owned)
        .ok_or_else(|| invalid("path", "must be valid UTF-8"))
}

fn ensure_success(executable: &str, output: &crate::ProcessOutput) -> Result<()> {
    if output.success() {
        Ok(())
    } else {
        Err(LumicError::Process {
            executable: executable.into(),
            message: String::from_utf8_lossy(&output.stderr).trim().into(),
        })
    }
}

fn upsert_by<T: Clone, K: PartialEq + ?Sized>(
    values: &mut Vec<T>,
    value: T,
    key: impl Fn(&T) -> &K,
) {
    if let Some(existing) = values
        .iter_mut()
        .find(|existing| key(existing) == key(&value))
    {
        *existing = value;
    } else {
        values.push(value);
    }
}

fn node_is_known(state: &InfrastructureState, id: &str) -> bool {
    state
        .local_node
        .as_ref()
        .is_some_and(|local| local.id == id)
        || state
            .nodes
            .iter()
            .any(|node| node.identity.id == id && node.trust_status == TrustStatus::Trusted)
}

fn authorize(context: &OperationContext) -> Result<()> {
    if context.approved && !context.dry_run {
        Ok(())
    } else {
        Err(LumicError::InvalidInput {
            field: "approval".into(),
            message: "infrastructure mutations require an approved non-dry-run context".into(),
        })
    }
}

fn invalid(field: &str, message: &str) -> LumicError {
    LumicError::InvalidInput {
        field: field.into(),
        message: message.into(),
    }
}

fn not_found(entity: &str, id: &str) -> LumicError {
    LumicError::InvalidInput {
        field: entity.into(),
        message: format!("{entity} '{id}' does not exist"),
    }
}

fn state_io(error: std::io::Error) -> LumicError {
    LumicError::Internal {
        message: format!("infrastructure state I/O failed: {error}"),
    }
}

fn state_json(error: serde_json::Error) -> LumicError {
    LumicError::Internal {
        message: format!("infrastructure state is invalid: {error}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumic_core::{OperationInterface, application::ApplicationRuntime};

    fn context() -> OperationContext {
        OperationContext {
            actor: "test".into(),
            interface: OperationInterface::Internal,
            correlation_id: "epic-d-test".into(),
            dry_run: false,
            approved: true,
        }
    }

    fn service(name: &str) -> (PathBuf, InfrastructureService) {
        let root = std::env::temp_dir().join(format!(
            "lumic-infrastructure-{name}-{}-{}",
            std::process::id(),
            unix_time_ms()
        ));
        fs::create_dir_all(&root).unwrap();
        let service = InfrastructureService::new(&root, root.join("apps"));
        (root, service)
    }

    #[test]
    fn clones_an_environment_with_explicit_domain_and_secret_transforms() {
        let (source_root, source) = service("clone-source");
        let (target_root, target) = service("clone-target");
        let context = context();
        source
            .initialize_node("production", "Production", vec![NodeRole::App], &context)
            .unwrap();
        target
            .initialize_node("staging", "Staging", vec![NodeRole::App], &context)
            .unwrap();
        let application = source
            .applications
            .create(
                "shop",
                "shop.example.test",
                ApplicationRuntime::Php,
                false,
                &context,
            )
            .unwrap();
        assert_eq!(application.id, "shop");
        source.secrets.put("production-key", b"production").unwrap();
        target.secrets.put("staging-key", b"staging").unwrap();
        source
            .applications
            .set_environment_reference("shop", "APP_KEY", "production-key", &context)
            .unwrap();
        let bundle = source
            .export_environment("shop", "production", EnvironmentTier::Production, &context)
            .unwrap();
        let mut transform = EnvironmentTransform {
            target_id: "shop-staging".into(),
            target_tier: EnvironmentTier::Staging,
            target_domain: "staging.shop.example.test".into(),
            environment_reference_overrides: BTreeMap::new(),
            service_id_overrides: BTreeMap::new(),
        };
        assert!(
            target
                .import_environment(&bundle, &transform, &context)
                .is_err(),
            "source secret references must not silently cross node boundaries"
        );
        transform
            .environment_reference_overrides
            .insert("APP_KEY".into(), "staging-key".into());
        let transformed = target
            .import_environment(&bundle, &transform, &context)
            .unwrap();
        assert_eq!(transformed.domain, "staging.shop.example.test");
        assert_eq!(
            transformed.environment_references.get("APP_KEY"),
            Some(&"staging-key".into())
        );
        assert_ne!(
            fs::read(source_root.join("secrets/production-key")).unwrap(),
            fs::read(target_root.join("secrets/staging-key")).unwrap()
        );
        fs::remove_dir_all(source_root).unwrap();
        fs::remove_dir_all(target_root).unwrap();
    }

    #[test]
    fn exchanges_public_enrollments_and_rejects_replay() {
        let (root_a, node_a) = service("node-a");
        let (root_b, node_b) = service("node-b");
        let context = context();
        node_a
            .initialize_node("node-a", "Node A", vec![NodeRole::App], &context)
            .unwrap();
        node_b
            .initialize_node("node-b", "Node B", vec![NodeRole::App], &context)
            .unwrap();
        node_a
            .register_node(
                node_b
                    .enrollment("https://node-b.example.test/mcp")
                    .unwrap(),
                &context,
            )
            .unwrap();
        node_b
            .register_node(
                node_a
                    .enrollment("https://node-a.example.test/mcp")
                    .unwrap(),
                &context,
            )
            .unwrap();
        let request = node_a
            .sign_remote_request(
                "node-b",
                RemoteOperation {
                    kind: "application.deploy".into(),
                    resource_id: "shop".into(),
                    arguments: BTreeMap::new(),
                },
                60,
            )
            .unwrap();
        node_b.verify_remote_request(&request, &context).unwrap();
        assert!(node_b.verify_remote_request(&request, &context).is_err());
        fs::remove_dir_all(root_a).unwrap();
        fs::remove_dir_all(root_b).unwrap();
    }

    #[test]
    fn read_model_reports_an_uninitialized_local_node() {
        let (root, service) = service("read-model-uninitialized");

        let model = service.read_model().unwrap();

        assert_eq!(model.local_node, None);
        fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn records_cross_node_tcp_health_evidence() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}/mcp", listener.local_addr().unwrap());
        let (root_a, node_a) = service("health-a");
        let (root_b, node_b) = service("health-b");
        let context = context();
        node_a
            .initialize_node("node-a", "Node A", vec![NodeRole::App], &context)
            .unwrap();
        node_b
            .initialize_node("node-b", "Node B", vec![NodeRole::App], &context)
            .unwrap();
        node_a
            .register_node(node_b.enrollment(&endpoint).unwrap(), &context)
            .unwrap();
        let accept = tokio::spawn(async move { listener.accept().await.unwrap() });

        let health = node_a.check_node_health("node-b").await.unwrap();
        assert!(health.healthy);
        accept.await.unwrap();
        assert_eq!(
            node_a
                .read_model()
                .unwrap()
                .nodes
                .first()
                .and_then(|node| node.last_health.as_ref())
                .map(|health| health.healthy),
            Some(true)
        );

        fs::remove_dir_all(root_a).unwrap();
        fs::remove_dir_all(root_b).unwrap();
    }

    #[test]
    fn stops_unstarted_members_after_a_coordination_failure() {
        let (root_a, node_a) = service("coordination-a");
        let (root_b, node_b) = service("coordination-b");
        let context = context();
        node_a
            .initialize_node("node-a", "Node A", vec![NodeRole::App], &context)
            .unwrap();
        node_b
            .initialize_node("node-b", "Node B", vec![NodeRole::App], &context)
            .unwrap();
        node_a
            .register_node(
                node_b
                    .enrollment("https://node-b.example.test/mcp")
                    .unwrap(),
                &context,
            )
            .unwrap();

        let coordination = node_a
            .begin_coordination(
                "staging",
                vec![
                    ("node-a".into(), "web".into()),
                    ("node-b".into(), "worker".into()),
                ],
                &context,
            )
            .unwrap();
        let failed = node_a
            .report_coordination_member(
                &coordination.id,
                "node-a",
                DeploymentMemberStatus::Failed,
                Some(false),
                None,
                "health check failed".into(),
                &context,
            )
            .unwrap();
        assert_eq!(failed.status, CoordinationStatus::Failed);
        assert!(
            node_a
                .report_coordination_member(
                    &coordination.id,
                    "node-b",
                    DeploymentMemberStatus::Succeeded,
                    Some(true),
                    Some("deployment-2".into()),
                    "deployed".into(),
                    &context,
                )
                .is_err()
        );

        fs::remove_dir_all(root_a).unwrap();
        fs::remove_dir_all(root_b).unwrap();
    }

    #[tokio::test]
    async fn creates_a_native_bare_repository_and_fixed_push_hook() {
        let (root, service) = service("git");
        let context = context();
        service
            .initialize_node("git-node", "Git node", vec![NodeRole::Git], &context)
            .unwrap();
        let repository = service
            .create_hosted_repository("shop", "main", &context)
            .await
            .unwrap();
        service
            .applications
            .create(
                "shop",
                "shop.example.test",
                ApplicationRuntime::Static,
                false,
                &context,
            )
            .unwrap();
        service
            .set_push_trigger("shop", "shop", "main", true, &context)
            .unwrap();
        let hook =
            fs::read_to_string(Path::new(&repository.path).join("hooks/post-receive")).unwrap();
        assert_eq!(
            hook,
            "#!/bin/sh\nexec /usr/local/bin/lumic git receive shop\n"
        );
        assert!(!hook.contains("sh -c"));
        fs::remove_dir_all(root).unwrap();
    }
}
