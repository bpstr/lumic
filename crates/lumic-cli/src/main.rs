use clap::{Parser, Subcommand, ValueEnum};
use lumic_core::{
    Architecture, HostFacts, OperationContext, OperationInterface,
    application::{
        ApplicationProcess, ApplicationProcessKind, ApplicationRuntime, ApplicationSchedule,
        ApplicationServiceReference,
    },
    attention::NodePersonality,
    infrastructure::{
        DeploymentMemberStatus, EnvironmentBundle, EnvironmentTier, EnvironmentTransform,
        MembershipKind, NodeEnrollment, NodeRole, RemoteOperation, ResourceEndpoint,
        SignedRemoteRequest,
    },
    operations::{
        AutomationAction, AutomationRule, EventSubscription, SignalSeverity, TimelineQuery,
        WebhookDestination,
    },
    package::{PackageMutation, PackageName, PackageRecord},
    recipe::RecipeInstallRequest,
    server::{
        BackupSchedule, FirewallDecision, FirewallRule, NetworkProtocol, ProcessSignal,
        RemediationAction, UpdateScope,
    },
};
use lumic_platform::{
    application::ApplicationService,
    apt::AptPackageManager,
    attention::AttentionService,
    audit_store::AuditStore,
    diagnostics::diagnose_host,
    event_store::EventStore,
    infrastructure::InfrastructureService,
    intelligence::ApplicationIntelligence,
    managed_service::ManagedServiceManager,
    operations::OperationsService,
    recipe::RecipeManager,
    repository::RepositoryService,
    self_update::SelfUpdateManager,
    server::HostOperator,
    systemd::{ServiceAction, SystemdServiceManager},
};
use std::{collections::BTreeMap, fs, io::Read, path::PathBuf, time::Duration};

#[derive(Parser)]
#[command(
    name = "lumic",
    disable_version_flag = true,
    about = "Host-native Linux server management"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Print the deterministic Lumic package version.
    Version,
    /// Inspect live host status.
    Status {
        /// Emit the complete host facts as JSON.
        #[arg(long)]
        json: bool,
    },
    /// Diagnose host load, processes, memory pressure, and failed services.
    Diagnose {
        #[arg(long)]
        json: bool,
    },
    /// Answer how the node is doing, what changed, and what needs attention.
    HowAreYou {
        /// Relevant event history window, from 1 to 720 hours.
        #[arg(long, default_value_t = 24)]
        period_hours: u64,
        /// Emit the complete factual summary and rendered copy as JSON.
        #[arg(long)]
        json: bool,
    },
    /// Inspect or change the node's deterministic presentation personality.
    Personality {
        #[command(subcommand)]
        command: PersonalityCommand,
    },
    /// Inspect and operate validated systemd units.
    Service {
        #[command(subcommand)]
        command: ServiceCommand,
    },
    /// Manage native database, cache, and search resources through stable Lumic contracts.
    ManagedService {
        #[command(subcommand)]
        command: ManagedServiceCommand,
    },
    /// Search, inspect, install, or remove trusted native packages.
    Package {
        #[command(subcommand)]
        command: PackageCommand,
    },
    /// Show the local infrastructure event trail.
    Events {
        /// Maximum number of newest events to return.
        #[arg(long, default_value_t = 50)]
        limit: usize,
        /// Emit JSON instead of concise lines.
        #[arg(long)]
        json: bool,
    },
    /// Show the local before/after mutation audit trail.
    Audit {
        #[arg(long, default_value_t = 50)]
        limit: usize,
        #[arg(long)]
        json: bool,
    },
    /// Correlate operational history, configure notifications, and run bounded automation.
    Operations {
        #[command(subcommand)]
        command: OperationsCommand,
    },
    /// Detect application stacks, inspect configuration, graph dependencies, and apply typed integrations.
    Intelligence {
        #[command(subcommand)]
        command: IntelligenceCommand,
    },
    /// Apply or schedule checksum-verified nightly binary updates.
    SelfUpdate {
        #[command(subcommand)]
        command: SelfUpdateCommand,
    },
    /// Create, inspect, deploy, and roll back applications.
    App {
        #[command(subcommand)]
        command: AppCommand,
    },
    /// Host and mirror native Git repositories, or handle a validated push trigger.
    Git {
        #[command(subcommand)]
        command: GitCommand,
    },
    /// Manage provider-neutral local and remote Git repositories.
    Repo {
        #[command(subcommand)]
        command: GitRepositoryCommand,
    },
    /// Export, transform, import, and diff portable application environments.
    Environment {
        #[command(subcommand)]
        command: EnvironmentCommand,
    },
    /// Manage node identity, trust, topology, and coordinated deployment state.
    Infrastructure {
        #[command(subcommand)]
        command: InfrastructureCommand,
    },
    /// Catalog, plan, install, update, and uninstall versioned application recipes.
    Recipe {
        #[command(subcommand)]
        command: RecipeCommand,
    },
    /// Inspect and operate typed host resources without unrestricted shell execution.
    Server {
        #[command(subcommand)]
        command: ServerCommand,
    },
    /// Configure the authenticated local operator UI.
    Ui {
        #[command(subcommand)]
        command: UiCommand,
    },
    /// Serve or configure Lumic's Model Context Protocol adapter.
    Mcp {
        #[command(subcommand)]
        command: McpCommand,
    },
}

#[derive(Subcommand)]
enum McpCommand {
    /// Serve MCP over standard input/output for SSH and local agent clients.
    Serve,
    /// Rotate the bearer token accepted by lumicd's optional HTTP MCP listener.
    Token {
        #[command(subcommand)]
        command: McpTokenCommand,
    },
}

#[derive(Subcommand)]
enum McpTokenCommand {
    /// Generate a new token, store only its digest, and print it once.
    Rotate,
}

#[derive(Subcommand)]
enum PersonalityCommand {
    /// Show the configured personality.
    Show,
    /// Set the personality used by conversational status surfaces.
    Set {
        #[arg(value_enum)]
        personality: PersonalityArg,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum PersonalityArg {
    Professional,
    Dry,
    Grumpy,
    Paranoid,
    Cheerful,
    Idiot,
}

impl From<PersonalityArg> for NodePersonality {
    fn from(value: PersonalityArg) -> Self {
        match value {
            PersonalityArg::Professional => Self::Professional,
            PersonalityArg::Dry => Self::Dry,
            PersonalityArg::Grumpy => Self::Grumpy,
            PersonalityArg::Paranoid => Self::Paranoid,
            PersonalityArg::Cheerful => Self::Cheerful,
            PersonalityArg::Idiot => Self::Idiot,
        }
    }
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum EnvironmentTierArg {
    Production,
    Staging,
    Development,
}

impl From<EnvironmentTierArg> for EnvironmentTier {
    fn from(value: EnvironmentTierArg) -> Self {
        match value {
            EnvironmentTierArg::Production => Self::Production,
            EnvironmentTierArg::Staging => Self::Staging,
            EnvironmentTierArg::Development => Self::Development,
        }
    }
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum NodeRoleArg {
    App,
    Worker,
    Database,
    Cache,
    Git,
    Media,
    Backup,
    Edge,
}

impl From<NodeRoleArg> for NodeRole {
    fn from(value: NodeRoleArg) -> Self {
        match value {
            NodeRoleArg::App => Self::App,
            NodeRoleArg::Worker => Self::Worker,
            NodeRoleArg::Database => Self::Database,
            NodeRoleArg::Cache => Self::Cache,
            NodeRoleArg::Git => Self::Git,
            NodeRoleArg::Media => Self::Media,
            NodeRoleArg::Backup => Self::Backup,
            NodeRoleArg::Edge => Self::Edge,
        }
    }
}

#[derive(Subcommand)]
enum GitCommand {
    Host {
        repository: String,
        #[arg(long, default_value = "main")]
        branch: String,
    },
    Mirror {
        mirror: String,
        url: String,
        #[arg(long, default_value = "main")]
        branch: String,
        #[arg(long)]
        credential_reference: Option<String>,
    },
    Trigger {
        repository: String,
        application: String,
        #[arg(long, default_value = "main")]
        branch: String,
        #[arg(long, default_value_t = true, action = clap::ArgAction::Set)]
        enabled: bool,
    },
    #[command(hide = true)]
    Receive { repository: String },
}

#[derive(Subcommand)]
enum GitRepositoryCommand {
    List,
    Get {
        repository: String,
    },
    PlanCreate {
        name: String,
        #[arg(long)]
        namespace: Option<String>,
        #[arg(long)]
        branch: Option<String>,
    },
    Create {
        name: String,
        #[arg(long)]
        namespace: Option<String>,
        #[arg(long)]
        branch: Option<String>,
        #[arg(long)]
        dry_run: bool,
    },
    Import {
        name: String,
        url: String,
        #[arg(long)]
        namespace: Option<String>,
        #[arg(long)]
        credential_reference: Option<String>,
        #[arg(long)]
        dry_run: bool,
    },
    Register {
        name: String,
        path: PathBuf,
        #[arg(long)]
        namespace: Option<String>,
        #[arg(long)]
        dry_run: bool,
    },
    Adopt {
        repository: String,
        #[arg(long)]
        dry_run: bool,
    },
    Delete {
        repository: String,
        #[arg(long)]
        dry_run: bool,
    },
    Discover {
        root: PathBuf,
    },
    Status {
        repository: String,
    },
    Branches {
        repository: String,
    },
    Tags {
        repository: String,
    },
    RemoteAdd {
        repository: String,
        name: String,
        url: String,
        #[arg(long)]
        credential_reference: Option<String>,
        #[arg(long, default_value_t = true, action = clap::ArgAction::Set)]
        fetch: bool,
        #[arg(long, default_value_t = true, action = clap::ArgAction::Set)]
        push: bool,
        #[arg(long)]
        mirror: bool,
    },
    RemoteRemove {
        repository: String,
        name: String,
    },
    Fetch {
        repository: String,
        #[arg(default_value = "origin")]
        remote: String,
    },
    Push {
        repository: String,
        #[arg(default_value = "origin")]
        remote: String,
        #[arg(long)]
        mirror: bool,
    },
    CloneUrl {
        repository: String,
        origin: String,
    },
    PlanDeployment {
        repository: String,
        application: String,
        destination: PathBuf,
        #[arg(long, default_value = "main")]
        branch: String,
        #[arg(long, default_value = "atomic")]
        strategy: String,
        #[arg(long)]
        deploy_on_push: bool,
        #[arg(long, default_value_t = 5)]
        keep_releases: usize,
        #[arg(long)]
        health_url: Option<String>,
    },
    ConfigureDeployment {
        repository: String,
        application: String,
        destination: PathBuf,
        #[arg(long, default_value = "main")]
        branch: String,
        #[arg(long, default_value = "atomic")]
        strategy: String,
        #[arg(long)]
        deploy_on_push: bool,
        #[arg(long, default_value_t = 5)]
        keep_releases: usize,
        #[arg(long)]
        health_url: Option<String>,
        #[arg(long)]
        dry_run: bool,
    },
}

#[derive(Subcommand)]
enum EnvironmentCommand {
    SecretGenerate {
        reference: String,
    },
    ReferenceSet {
        application: String,
        name: String,
        reference: String,
    },
    /// Read one application environment value from stdin and store it encrypted at rest.
    SecretSet {
        application: String,
        name: String,
    },
    /// Replace an application-owned environment value with fresh random material.
    SecretRotate {
        application: String,
        name: String,
    },
    /// Remove an application environment key and its application-owned value.
    SecretDelete {
        application: String,
        name: String,
    },
    Export {
        application: String,
        environment: String,
        #[arg(long, value_enum)]
        tier: EnvironmentTierArg,
        #[arg(long)]
        output: Option<PathBuf>,
    },
    Import {
        bundle: PathBuf,
        #[arg(long)]
        target: String,
        #[arg(long, value_enum)]
        tier: EnvironmentTierArg,
        #[arg(long)]
        domain: String,
        #[arg(long = "env", value_parser = parse_key_value)]
        environment: Vec<(String, String)>,
        #[arg(long = "service", value_parser = parse_key_value)]
        services: Vec<(String, String)>,
    },
    Diff {
        source: PathBuf,
        target: PathBuf,
    },
}

#[derive(Subcommand)]
enum InfrastructureCommand {
    Status,
    Init {
        id: String,
        #[arg(long)]
        name: String,
        #[arg(long = "role", value_enum, required = true)]
        roles: Vec<NodeRoleArg>,
    },
    Enrollment {
        #[arg(long)]
        endpoint: String,
        #[arg(long)]
        output: Option<PathBuf>,
    },
    Register {
        enrollment: PathBuf,
    },
    Revoke {
        node: String,
    },
    Endpoint {
        id: String,
        #[arg(long)]
        provider_node: String,
        #[arg(long)]
        provider_kind: String,
        #[arg(long)]
        provider: String,
        #[arg(long)]
        consumer_node: String,
        #[arg(long)]
        consumer_kind: String,
        #[arg(long)]
        consumer: String,
        #[arg(long)]
        protocol: String,
        #[arg(long)]
        host: String,
        #[arg(long)]
        port: u16,
        #[arg(long)]
        health_path: Option<String>,
        #[arg(long)]
        secret_reference: Option<String>,
    },
    Membership {
        #[arg(long, value_parser = ["worker", "reverse_proxy"])]
        kind: String,
        #[arg(long)]
        environment: String,
        #[arg(long)]
        application: String,
        #[arg(long)]
        node: String,
        #[arg(long, default_value_t = true, action = clap::ArgAction::Set)]
        enabled: bool,
    },
    Health {
        node: String,
    },
    Coordinate {
        environment: String,
        #[arg(long = "member", value_parser = parse_key_value, required = true)]
        members: Vec<(String, String)>,
    },
    Report {
        coordination: String,
        #[arg(long)]
        node: String,
        #[arg(long, value_parser = ["pending", "running", "succeeded", "failed", "rolled_back"])]
        status: String,
        #[arg(long)]
        healthy: Option<bool>,
        #[arg(long)]
        deployment: Option<String>,
        #[arg(long)]
        message: String,
    },
    Sign {
        #[arg(long)]
        target: String,
        #[arg(long, value_parser = ["application.deploy", "application.rollback"])]
        operation: String,
        #[arg(long)]
        application: String,
        #[arg(long, default_value_t = 60)]
        ttl: u64,
        #[arg(long)]
        output: Option<PathBuf>,
    },
    Apply {
        request: PathBuf,
    },
}

#[derive(Subcommand)]
enum RecipeCommand {
    Catalog,
    List,
    Inspect { app: String },
    Plan(RecipeRequestArgs),
    Install(RecipeRequestArgs),
    Update { app: String },
    Uninstall { app: String },
}

#[derive(clap::Args)]
struct RecipeRequestArgs {
    recipe: String,
    app: String,
    domain: String,
    #[arg(long)]
    repository: Option<String>,
    #[arg(long, default_value = "main")]
    branch: String,
    #[arg(long)]
    tls_email: Option<String>,
    #[arg(long = "env", value_parser = parse_key_value)]
    environment: Vec<(String, String)>,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum ProtocolArg {
    Tcp,
    Udp,
}
impl From<ProtocolArg> for NetworkProtocol {
    fn from(value: ProtocolArg) -> Self {
        match value {
            ProtocolArg::Tcp => Self::Tcp,
            ProtocolArg::Udp => Self::Udp,
        }
    }
}
#[derive(Debug, Clone, Copy, ValueEnum)]
enum DecisionArg {
    Allow,
    Deny,
}
impl From<DecisionArg> for FirewallDecision {
    fn from(value: DecisionArg) -> Self {
        match value {
            DecisionArg::Allow => Self::Allow,
            DecisionArg::Deny => Self::Deny,
        }
    }
}
#[derive(Debug, Clone, Copy, ValueEnum)]
enum SignalArg {
    Terminate,
    Kill,
    Hangup,
}
impl From<SignalArg> for ProcessSignal {
    fn from(value: SignalArg) -> Self {
        match value {
            SignalArg::Terminate => Self::Terminate,
            SignalArg::Kill => Self::Kill,
            SignalArg::Hangup => Self::Hangup,
        }
    }
}
#[derive(Debug, Clone, Copy, ValueEnum)]
enum UpdateScopeArg {
    Security,
    All,
}
impl From<UpdateScopeArg> for UpdateScope {
    fn from(value: UpdateScopeArg) -> Self {
        match value {
            UpdateScopeArg::Security => Self::Security,
            UpdateScopeArg::All => Self::All,
        }
    }
}

#[derive(Subcommand)]
enum ServerCommand {
    Snapshot,
    UserCreate {
        name: String,
    },
    UserDelete {
        name: String,
    },
    GroupCreate {
        name: String,
    },
    GroupDelete {
        name: String,
    },
    GroupAddMember {
        group: String,
        user: String,
    },
    Permissions {
        path: PathBuf,
        owner: String,
        group: String,
        mode: String,
    },
    FirewallList,
    FirewallRule {
        #[arg(value_enum)]
        decision: DecisionArg,
        port: u16,
        #[arg(value_enum, default_value_t = ProtocolArg::Tcp)]
        protocol: ProtocolArg,
        #[arg(long, default_value = "any")]
        source: String,
        #[arg(long)]
        remove: bool,
    },
    Listeners,
    Mounts,
    Processes {
        #[arg(long, default_value_t = 25)]
        limit: usize,
    },
    ProcessSignal {
        pid: u32,
        #[arg(value_enum)]
        signal: SignalArg,
    },
    Timers,
    Updates,
    UpdateApply {
        #[arg(value_enum)]
        scope: UpdateScopeArg,
    },
    Logs {
        #[arg(long)]
        unit: Option<String>,
        #[arg(long)]
        priority: Option<String>,
        #[arg(long)]
        since: Option<String>,
        #[arg(long)]
        query: Option<String>,
        #[arg(long, default_value_t = 100)]
        lines: usize,
    },
    BackupSchedule {
        id: String,
        service: String,
        #[arg(long)]
        database: Option<String>,
        #[arg(long)]
        on_calendar: String,
        #[arg(long, default_value_t = true)]
        enabled: bool,
    },
    RemediateRestart {
        unit: String,
    },
    RemediateTerminate {
        pid: u32,
    },
    RemediateJournal {
        #[arg(long)]
        older_than_days: u16,
    },
}

#[derive(Subcommand)]
enum UiCommand {
    Token {
        #[command(subcommand)]
        command: UiTokenCommand,
    },
}

#[derive(Subcommand)]
enum UiTokenCommand {
    /// Create or rotate the admin token. The value is printed once; only its hash is stored.
    Rotate,
}

#[derive(Subcommand)]
enum PackageCommand {
    /// Search apt metadata. Search results are not automatically trusted.
    Search { query: PackageName },
    /// Inspect installed and candidate versions.
    Inspect { package: PackageName },
    /// Install a package allowed by Lumic policy.
    Install {
        package: PackageName,
        #[arg(long)]
        dry_run: bool,
    },
    /// Remove a package allowed by Lumic policy.
    Remove {
        package: PackageName,
        #[arg(long)]
        dry_run: bool,
    },
    /// Refresh the apt package index.
    UpdateIndex,
    /// List packages trusted by the built-in policy.
    Allowed,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum RuntimeArg {
    Static,
    Php,
    Node,
}

impl From<RuntimeArg> for ApplicationRuntime {
    fn from(value: RuntimeArg) -> Self {
        match value {
            RuntimeArg::Static => Self::Static,
            RuntimeArg::Php => Self::Php,
            RuntimeArg::Node => Self::Node,
        }
    }
}

#[derive(Subcommand)]
enum ServiceCommand {
    Inspect {
        unit: String,
        #[arg(long)]
        json: bool,
    },
    Start {
        unit: String,
    },
    Stop {
        unit: String,
    },
    Restart {
        unit: String,
    },
    Reload {
        unit: String,
    },
    Enable {
        unit: String,
    },
    Disable {
        unit: String,
    },
}

#[derive(Subcommand)]
enum ManagedServiceCommand {
    /// List trusted catalog definitions and their shared configuration schemas.
    Catalog,
    /// Inspect one trusted service definition and its configuration schema.
    Schema {
        definition: String,
    },
    List,
    Detect {
        definition: String,
    },
    Inspect {
        service: String,
    },
    PlanInstall {
        service: String,
        definition: String,
    },
    Install {
        service: String,
        definition: String,
        #[arg(long)]
        dry_run: bool,
    },
    Configure {
        service: String,
        #[arg(long)]
        bind: Option<String>,
        #[arg(long)]
        port: Option<u16>,
        #[arg(long = "setting")]
        settings: Vec<String>,
        #[arg(long)]
        dry_run: bool,
    },
    Start {
        service: String,
        #[arg(long)]
        dry_run: bool,
    },
    Stop {
        service: String,
        #[arg(long)]
        dry_run: bool,
    },
    Restart {
        service: String,
        #[arg(long)]
        dry_run: bool,
    },
    Update {
        service: String,
        #[arg(long)]
        dry_run: bool,
    },
    Remove {
        service: String,
        #[arg(long)]
        dry_run: bool,
    },
    Logs {
        service: String,
        #[arg(long, default_value_t = 100)]
        lines: usize,
    },
    DeclareDependency {
        service: String,
        dependency: String,
        #[arg(long)]
        purpose: String,
        #[arg(long, default_value_t = true)]
        required: bool,
        #[arg(long)]
        dry_run: bool,
    },
    DatabaseCreate {
        service: String,
        database: String,
        #[arg(long)]
        owner: Option<String>,
        #[arg(long)]
        dry_run: bool,
    },
    UserCreate {
        service: String,
        user: String,
        #[arg(long)]
        dry_run: bool,
    },
    Grant {
        service: String,
        database: String,
        user: String,
        #[arg(long)]
        dry_run: bool,
    },
    Backup {
        service: String,
        #[arg(long)]
        database: Option<String>,
        #[arg(long)]
        dry_run: bool,
    },
    BackupVerify {
        backup: String,
    },
    Restore {
        service: String,
        backup: String,
        #[arg(long)]
        dry_run: bool,
    },
    Attach {
        service: String,
        app: String,
        #[arg(long)]
        role: String,
        #[arg(long)]
        database: Option<String>,
        #[arg(long)]
        user: Option<String>,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum SignalSeverityArg {
    Info,
    Warning,
    Error,
    Critical,
}

impl From<SignalSeverityArg> for SignalSeverity {
    fn from(value: SignalSeverityArg) -> Self {
        match value {
            SignalSeverityArg::Info => Self::Info,
            SignalSeverityArg::Warning => Self::Warning,
            SignalSeverityArg::Error => Self::Error,
            SignalSeverityArg::Critical => Self::Critical,
        }
    }
}

#[derive(Subcommand)]
enum OperationsCommand {
    /// Import new durable Lumic events into the correlated timeline.
    Capture,
    /// Observe current host and resource state immediately, bypassing the sampling interval.
    Observe,
    /// Query newest operational evidence.
    Timeline {
        #[arg(long)]
        entity: Option<String>,
        #[arg(long)]
        entity_id: Option<String>,
        #[arg(long)]
        event_type: Option<String>,
        #[arg(long)]
        since_ms: Option<u128>,
        #[arg(long, default_value_t = 100)]
        limit: usize,
    },
    /// Reconstruct a factual incident report from a time window.
    Incident {
        #[arg(long)]
        entity_id: Option<String>,
        #[arg(long)]
        since_ms: Option<u128>,
        #[arg(long)]
        until_ms: Option<u128>,
        #[arg(long, default_value_t = 250)]
        limit: usize,
    },
    /// Record a typed signal from a provider integration.
    ProviderSignal {
        event_type: String,
        entity: String,
        entity_id: String,
        #[arg(long, value_enum, default_value = "info")]
        severity: SignalSeverityArg,
        #[arg(long)]
        summary: String,
        #[arg(long, default_value = "{}")]
        payload: String,
    },
    WebhookPlan {
        id: String,
        url: String,
        secret_reference: String,
        #[arg(long, default_value_t = 5_000)]
        timeout_ms: u64,
        #[arg(long, default_value_t = 3)]
        max_attempts: u8,
    },
    WebhookApply {
        id: String,
        url: String,
        secret_reference: String,
        #[arg(long, default_value_t = 5_000)]
        timeout_ms: u64,
        #[arg(long, default_value_t = 3)]
        max_attempts: u8,
    },
    Subscribe {
        id: String,
        destination: String,
        #[arg(long = "event", required = true)]
        event_types: Vec<String>,
        #[arg(long)]
        entity: Option<String>,
        #[arg(long)]
        entity_id: Option<String>,
    },
    RulePlan {
        id: String,
        event_type: String,
        unit: String,
        #[arg(long)]
        entity_id: Option<String>,
        #[arg(long, default_value_t = 60)]
        cooldown_seconds: u64,
        #[arg(long, default_value_t = 2)]
        max_attempts: u8,
    },
    RuleApply {
        id: String,
        event_type: String,
        unit: String,
        #[arg(long)]
        entity_id: Option<String>,
        #[arg(long, default_value_t = 60)]
        cooldown_seconds: u64,
        #[arg(long, default_value_t = 2)]
        max_attempts: u8,
    },
    /// Capture events and attempt all due notifications once.
    RunOnce,
    Deliveries {
        #[arg(long, default_value_t = 100)]
        limit: usize,
    },
    /// Restore the previous Lumic-managed operations configuration snapshot.
    RollbackConfiguration,
}

#[derive(Subcommand)]
enum IntelligenceCommand {
    Catalog,
    Fingerprint {
        app: String,
    },
    Config {
        app: String,
    },
    Graph {
        app: String,
    },
    Plan {
        app: String,
        #[arg(long, default_value = "laravel-redis@1")]
        integration: String,
        #[arg(long)]
        service: Option<String>,
    },
    Apply {
        app: String,
        #[arg(long, default_value = "laravel-redis@1")]
        integration: String,
        #[arg(long)]
        service: Option<String>,
    },
    Rollback {
        app: String,
        snapshot: String,
    },
    Incident {
        #[arg(long)]
        app: Option<String>,
        #[arg(long)]
        since_ms: Option<u128>,
        #[arg(long)]
        until_ms: Option<u128>,
        #[arg(long, default_value_t = 250)]
        limit: usize,
    },
    Analyze {
        destination: String,
        #[arg(long)]
        app: Option<String>,
        #[arg(long)]
        since_ms: Option<u128>,
        #[arg(long)]
        until_ms: Option<u128>,
        #[arg(long, default_value_t = 250)]
        limit: usize,
    },
}

#[derive(Subcommand)]
enum SelfUpdateCommand {
    /// Download, checksum, preflight, atomically replace, and postflight the nightly binary.
    Apply,
    /// Install and activate a persistent daily systemd update timer.
    EnableNightly,
}

#[derive(Subcommand)]
enum AppCommand {
    /// List configured applications.
    List {
        #[arg(long)]
        json: bool,
    },
    /// Create managed application directories and metadata.
    Create {
        name: String,
        #[arg(long)]
        domain: String,
        #[arg(long, value_enum)]
        runtime: RuntimeArg,
        #[arg(long)]
        www: bool,
        #[arg(long)]
        json: bool,
    },
    /// Inspect one application.
    Inspect {
        app: String,
        #[arg(long)]
        json: bool,
    },
    /// Inspect, plan, or apply the repository-owned lumic.toml contract.
    Manifest {
        #[command(subcommand)]
        command: ManifestCommand,
    },
    /// Configure the deployment repository.
    Repository {
        #[command(subcommand)]
        command: RepositoryCommand,
    },
    /// Import a named SSH private key into Lumic's private credential store.
    Credential {
        #[command(subcommand)]
        command: CredentialCommand,
    },
    /// Install the runtime and configure nginx for an application.
    Provision {
        app: String,
        /// Required PHP runtime version (supported: 8.1, 8.2, 8.3, 8.4).
        #[arg(long)]
        runtime_version: Option<String>,
        #[arg(long = "component")]
        components: Vec<String>,
        #[arg(long)]
        json: bool,
    },
    /// Configure the HTTP endpoint used to approve or roll back deployments.
    Health {
        app: String,
        #[arg(long, default_value = "/")]
        path: String,
        #[arg(long, default_value_t = 80)]
        port: u16,
    },
    /// Configure a supervised worker or systemd timer for an application.
    Process {
        #[command(subcommand)]
        command: ProcessCommand,
    },
    /// Issue a Let's Encrypt certificate and enable HTTPS redirects.
    Tls {
        app: String,
        #[arg(long)]
        email: String,
    },
    /// Show the exact deployment changes, risks, validation, and recovery path.
    Plan {
        app: String,
        #[arg(long)]
        json: bool,
    },
    /// Deploy a new isolated release and atomically activate it.
    Deploy {
        app: String,
        #[arg(long)]
        json: bool,
    },
    /// Configure explicit argv-only pre/build/migrate/post deployment phases.
    ConfigureDeployment {
        app: String,
        #[arg(long = "pre-deploy-command")]
        pre_deploy_commands: Vec<String>,
        #[arg(long = "build-command")]
        build_command: Option<String>,
        #[arg(long = "migrate-command")]
        migrate_command: Option<String>,
        #[arg(long = "post-deploy-command")]
        post_deploy_commands: Vec<String>,
        #[arg(long = "node-command", requires_all = ["primary_port", "secondary_port"])]
        node_command: Option<String>,
        #[arg(long)]
        primary_port: Option<u16>,
        #[arg(long)]
        secondary_port: Option<u16>,
        #[arg(long, default_value_t = 10)]
        drain_seconds: u64,
    },
    /// Request cooperative cancellation at the next safe phase boundary.
    Cancel {
        app: String,
        deployment: String,
        #[arg(long)]
        json: bool,
    },
    /// Deploy the exact commit recorded by an earlier deployment.
    Redeploy {
        app: String,
        deployment: String,
        #[arg(long)]
        json: bool,
    },
    /// Read persistent deployment output after a log cursor.
    Logs {
        app: String,
        deployment: String,
        #[arg(long, default_value_t = 0)]
        after: u64,
        #[arg(long)]
        json: bool,
        #[arg(long)]
        follow: bool,
    },
    /// List application deployment history.
    Deployments {
        app: String,
        #[arg(long)]
        json: bool,
    },
    /// Activate the previous known-good release.
    Rollback {
        app: String,
        #[arg(long)]
        json: bool,
    },
    /// Remove metadata and move application files into Lumic trash.
    Delete { app: String },
}

#[derive(Debug, Subcommand)]
enum ManifestCommand {
    /// Parse and validate lumic.toml without server-state changes.
    Inspect {
        #[arg(long, default_value = ".")]
        repository_root: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Show the exact state changes, risks, and preconditions.
    Plan {
        app: String,
        #[arg(long, default_value = ".")]
        repository_root: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Apply the validated contract to an existing application.
    Apply {
        app: String,
        #[arg(long, default_value = ".")]
        repository_root: PathBuf,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
enum CredentialCommand {
    Import { name: String, source: PathBuf },
}

#[derive(Subcommand)]
enum ProcessCommand {
    Worker {
        app: String,
        name: String,
        #[arg(required = true, num_args = 1.., trailing_var_arg = true)]
        command: Vec<String>,
    },
    Schedule {
        app: String,
        name: String,
        #[arg(long)]
        on_calendar: String,
        #[arg(required = true, num_args = 1.., trailing_var_arg = true)]
        command: Vec<String>,
    },
}

#[derive(Subcommand)]
enum RepositoryCommand {
    /// Set a Git HTTPS/SSH source and branch.
    Set {
        app: String,
        url: String,
        #[arg(long, default_value = "main")]
        branch: String,
        #[arg(long)]
        credential_reference: Option<String>,
    },
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    match Cli::parse()
        .command
        .unwrap_or(Command::Status { json: false })
    {
        Command::Version => println!("lumic {}", env!("CARGO_PKG_VERSION")),
        Command::Status { json } => {
            let facts = lumic_platform::inspect_host()?;
            if json {
                println!("{}", serde_json::to_string_pretty(&facts)?);
            } else {
                print!("{}", render_status(&facts));
            }
        }
        Command::Diagnose { json } => {
            let report = diagnose_host().await?;
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                println!(
                    "Load: {:.2} {:.2} {:.2}; uptime: {}s",
                    report.load.one_minute,
                    report.load.five_minutes,
                    report.load.fifteen_minutes,
                    report.load.uptime_seconds
                );
                if report.findings.is_empty() {
                    println!("No actionable host findings.");
                } else {
                    for finding in report.findings {
                        println!(
                            "{}: {} — {} ({})",
                            finding.severity,
                            finding.summary,
                            finding.evidence,
                            finding.recommendation
                        );
                    }
                }
            }
        }
        Command::HowAreYou { period_hours, json } => {
            let report = attention_service().report(period_hours).await?;
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                println!("{}", report.rendered);
            }
        }
        Command::Personality { command } => match command {
            PersonalityCommand::Show => println!("{}", attention_service().personality()?),
            PersonalityCommand::Set { personality, json } => {
                let result = attention_service()
                    .set_personality(personality.into(), &operation_context(false))?;
                if json {
                    println!("{}", serde_json::to_string_pretty(&result)?);
                } else {
                    println!("{}", result.message);
                }
            }
        },
        Command::Service { command } => run_service(command).await?,
        Command::ManagedService { command } => run_managed_service(command).await?,
        Command::Package { command } => run_package(command).await?,
        Command::Events { limit, json } => {
            let events = event_store().list(limit)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&events)?);
            } else if events.is_empty() {
                println!("No events recorded.");
            } else {
                for event in events {
                    println!(
                        "{} {} {}:{} actor={}",
                        event.timestamp_unix_ms,
                        event.event_type,
                        event.entity,
                        event.entity_id,
                        event.actor
                    );
                }
            }
        }
        Command::Audit { limit, json } => {
            let records = audit_store().list(limit)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&records)?);
            } else if records.is_empty() {
                println!("No audit records recorded.");
            } else {
                for record in records {
                    println!(
                        "{} {} {}:{} actor={} succeeded={}",
                        record.timestamp_unix_ms,
                        record.operation,
                        record.entity,
                        record.entity_id,
                        record.actor,
                        record.succeeded
                    );
                }
            }
        }
        Command::Operations { command } => run_operations(command).await?,
        Command::Intelligence { command } => run_intelligence(command).await?,
        Command::SelfUpdate { command } => {
            let manager = SelfUpdateManager::system(state_directory());
            match command {
                SelfUpdateCommand::Apply => {
                    let result = manager.apply(&operation_context(false)).await?;
                    println!(
                        "{} at {} changed={}",
                        result.version, result.destination, result.changed
                    );
                    if let Some(recovery) = result.recovery_binary {
                        println!("Recovery binary: {recovery}");
                    }
                }
                SelfUpdateCommand::EnableNightly => {
                    let units = manager
                        .enable_nightly_timer(&operation_context(false))
                        .await?;
                    println!("Enabled {}.", units.join(", "));
                }
            }
        }
        Command::App { command } => run_app(command).await?,
        Command::Git { command } => run_git(command).await?,
        Command::Repo { command } => run_repository(command).await?,
        Command::Environment { command } => run_environment(command)?,
        Command::Infrastructure { command } => run_infrastructure(command).await?,
        Command::Recipe { command } => run_recipe(command).await?,
        Command::Server { command } => run_server(command).await?,
        Command::Ui { command } => match command {
            UiCommand::Token {
                command: UiTokenCommand::Rotate,
            } => {
                let token =
                    lumic_ui::UiCredentialStore::at_state_dir(state_directory()).rotate()?;
                println!("Lumic UI admin token (shown once): {token}");
            }
        },
        Command::Mcp { command } => match command {
            McpCommand::Serve => lumic_mcp::serve_stdio().await?,
            McpCommand::Token {
                command: McpTokenCommand::Rotate,
            } => {
                let token =
                    lumic_mcp::McpHttpCredentialStore::at_state_dir(state_directory()).rotate()?;
                println!("Lumic MCP HTTP bearer token (shown once): {token}");
            }
        },
    }
    Ok(())
}

async fn run_intelligence(command: IntelligenceCommand) -> Result<(), Box<dyn std::error::Error>> {
    let service = intelligence_service();
    let value = match command {
        IntelligenceCommand::Catalog => serde_json::to_value(service.catalog())?,
        IntelligenceCommand::Fingerprint { app } => {
            serde_json::to_value(service.fingerprint(&app)?)?
        }
        IntelligenceCommand::Config { app } => {
            serde_json::to_value(service.inspect_configuration(&app)?)?
        }
        IntelligenceCommand::Graph { app } => {
            serde_json::to_value(service.dependency_graph(&app)?)?
        }
        IntelligenceCommand::Plan {
            app,
            integration,
            service: selected,
        } => serde_json::to_value(service.plan_integration(
            &integration,
            &app,
            selected.as_deref(),
        )?)?,
        IntelligenceCommand::Apply {
            app,
            integration,
            service: selected,
        } => serde_json::to_value(
            service
                .apply_integration(
                    &integration,
                    &app,
                    selected.as_deref(),
                    &operation_context(false),
                )
                .await?,
        )?,
        IntelligenceCommand::Rollback { app, snapshot } => {
            service.restore_snapshot(&app, &snapshot, &operation_context(false))?;
            serde_json::json!({"application_id": app, "snapshot_id": snapshot, "restored": true})
        }
        IntelligenceCommand::Incident {
            app,
            since_ms,
            until_ms,
            limit,
        } => serde_json::to_value(service.incident_context(
            TimelineQuery {
                entity: None,
                entity_id: app.clone(),
                event_type: None,
                since_unix_ms: since_ms,
                until_unix_ms: until_ms,
                limit,
            },
            app.as_deref(),
        )?)?,
        IntelligenceCommand::Analyze {
            destination,
            app,
            since_ms,
            until_ms,
            limit,
        } => {
            let context = service.incident_context(
                TimelineQuery {
                    entity: None,
                    entity_id: app.clone(),
                    event_type: None,
                    since_unix_ms: since_ms,
                    until_unix_ms: until_ms,
                    limit,
                },
                app.as_deref(),
            )?;
            serde_json::to_value(service.analyze_incident(&context, &destination).await?)?
        }
    };
    println!("{}", serde_json::to_string_pretty(&value)?);
    Ok(())
}

async fn run_operations(command: OperationsCommand) -> Result<(), Box<dyn std::error::Error>> {
    let service = OperationsService::at_state_dir(state_directory());
    let value = match command {
        OperationsCommand::Capture => serde_json::to_value(service.capture_events().await?)?,
        OperationsCommand::Observe => serde_json::to_value(service.observe_now().await?)?,
        OperationsCommand::Timeline {
            entity,
            entity_id,
            event_type,
            since_ms,
            limit,
        } => serde_json::to_value(service.timeline(&TimelineQuery {
            entity,
            entity_id,
            event_type,
            since_unix_ms: since_ms,
            until_unix_ms: None,
            limit,
        })?)?,
        OperationsCommand::Incident {
            entity_id,
            since_ms,
            until_ms,
            limit,
        } => serde_json::to_value(service.incident(&TimelineQuery {
            entity: None,
            entity_id,
            event_type: None,
            since_unix_ms: since_ms,
            until_unix_ms: until_ms,
            limit,
        })?)?,
        OperationsCommand::ProviderSignal {
            event_type,
            entity,
            entity_id,
            severity,
            summary,
            payload,
        } => {
            let payload = serde_json::from_str(&payload)?;
            serde_json::to_value(
                service
                    .record_provider_signal(
                        &event_type,
                        &entity,
                        &entity_id,
                        severity.into(),
                        &summary,
                        payload,
                    )
                    .await?,
            )?
        }
        OperationsCommand::WebhookPlan {
            id,
            url,
            secret_reference,
            timeout_ms,
            max_attempts,
        } => serde_json::to_value(service.plan_destination(&WebhookDestination {
            id,
            url,
            secret_reference,
            timeout_ms,
            max_attempts,
            enabled: true,
        })?)?,
        OperationsCommand::WebhookApply {
            id,
            url,
            secret_reference,
            timeout_ms,
            max_attempts,
        } => serde_json::to_value(service.apply_destination(
            WebhookDestination {
                id,
                url,
                secret_reference,
                timeout_ms,
                max_attempts,
                enabled: true,
            },
            &operation_context(false),
        )?)?,
        OperationsCommand::Subscribe {
            id,
            destination,
            event_types,
            entity,
            entity_id,
        } => serde_json::to_value(service.apply_subscription(
            EventSubscription {
                id,
                destination_id: destination,
                event_types,
                entity,
                entity_id,
                enabled: true,
            },
            &operation_context(false),
        )?)?,
        OperationsCommand::RulePlan {
            id,
            event_type,
            unit,
            entity_id,
            cooldown_seconds,
            max_attempts,
        } => serde_json::to_value(service.plan_rule(&automation_rule(
            id,
            event_type,
            unit,
            entity_id,
            cooldown_seconds,
            max_attempts,
        ))?)?,
        OperationsCommand::RuleApply {
            id,
            event_type,
            unit,
            entity_id,
            cooldown_seconds,
            max_attempts,
        } => serde_json::to_value(service.apply_rule(
            automation_rule(
                id,
                event_type,
                unit,
                entity_id,
                cooldown_seconds,
                max_attempts,
            ),
            &operation_context(false),
        )?)?,
        OperationsCommand::RunOnce => service.run_once().await?,
        OperationsCommand::Deliveries { limit } => {
            serde_json::to_value(service.deliveries(limit)?)?
        }
        OperationsCommand::RollbackConfiguration => {
            service.rollback_configuration(&operation_context(false))?;
            serde_json::json!({"restored": true})
        }
    };
    println!("{}", serde_json::to_string_pretty(&value)?);
    Ok(())
}

fn automation_rule(
    id: String,
    event_type: String,
    unit: String,
    entity_id: Option<String>,
    cooldown_seconds: u64,
    max_attempts: u8,
) -> AutomationRule {
    AutomationRule {
        id,
        event_type,
        entity_id,
        action: AutomationAction::RestartService { unit },
        cooldown_seconds,
        max_attempts,
        enabled: true,
        last_applied_unix_ms: None,
        attempt_count: 0,
    }
}

async fn run_git(command: GitCommand) -> Result<(), Box<dyn std::error::Error>> {
    let infrastructure = infrastructure_service();
    let context = operation_context(false);
    let value = match command {
        GitCommand::Host { repository, branch } => serde_json::to_value(
            infrastructure
                .create_hosted_repository(&repository, &branch, &context)
                .await?,
        )?,
        GitCommand::Mirror {
            mirror,
            url,
            branch,
            credential_reference,
        } => serde_json::to_value(
            infrastructure
                .sync_mirror(&mirror, &url, &branch, credential_reference, &context)
                .await?,
        )?,
        GitCommand::Trigger {
            repository,
            application,
            branch,
            enabled,
        } => serde_json::to_value(infrastructure.set_push_trigger(
            &repository,
            &application,
            &branch,
            enabled,
            &context,
        )?)?,
        GitCommand::Receive { repository } => {
            let mut updates = String::new();
            std::io::stdin().read_to_string(&mut updates)?;
            serde_json::to_value(
                infrastructure
                    .receive_push(&repository, &updates, &context)
                    .await?,
            )?
        }
    };
    println!("{}", serde_json::to_string_pretty(&value)?);
    Ok(())
}

async fn run_repository(command: GitRepositoryCommand) -> Result<(), Box<dyn std::error::Error>> {
    let service = RepositoryService::new(state_directory())?;
    let value = match command {
        GitRepositoryCommand::List => serde_json::to_value(service.list()?)?,
        GitRepositoryCommand::Get { repository } => {
            serde_json::to_value(service.get(&repository)?)?
        }
        GitRepositoryCommand::PlanCreate {
            name,
            namespace,
            branch,
        } => serde_json::to_value(service.plan_create(
            namespace.as_deref(),
            &name,
            branch.as_deref(),
        )?)?,
        GitRepositoryCommand::Create {
            name,
            namespace,
            branch,
            dry_run,
        } => serde_json::to_value(
            service
                .create(
                    namespace.as_deref(),
                    &name,
                    branch.as_deref(),
                    &operation_context(dry_run),
                )
                .await?,
        )?,
        GitRepositoryCommand::Import {
            name,
            url,
            namespace,
            credential_reference,
            dry_run,
        } => serde_json::to_value(
            service
                .import(
                    namespace.as_deref(),
                    &name,
                    &url,
                    credential_reference,
                    &operation_context(dry_run),
                )
                .await?,
        )?,
        GitRepositoryCommand::Register {
            name,
            path,
            namespace,
            dry_run,
        } => serde_json::to_value(service.register_external(
            namespace.as_deref(),
            &name,
            &path,
            &operation_context(dry_run),
        )?)?,
        GitRepositoryCommand::Adopt {
            repository,
            dry_run,
        } => serde_json::to_value(
            service
                .adopt(&repository, &operation_context(dry_run))
                .await?,
        )?,
        GitRepositoryCommand::Delete {
            repository,
            dry_run,
        } => serde_json::to_value(service.delete(&repository, &operation_context(dry_run))?)?,
        GitRepositoryCommand::Discover { root } => serde_json::to_value(service.discover(&root)?)?,
        GitRepositoryCommand::Status { repository } => {
            serde_json::to_value(service.status(&repository).await?)?
        }
        GitRepositoryCommand::Branches { repository } => {
            serde_json::to_value(service.branches(&repository).await?)?
        }
        GitRepositoryCommand::Tags { repository } => {
            serde_json::to_value(service.tags(&repository).await?)?
        }
        GitRepositoryCommand::RemoteAdd {
            repository,
            name,
            url,
            credential_reference,
            fetch,
            push,
            mirror,
        } => serde_json::to_value(service.add_remote(
            &repository,
            lumic_core::repository::RepositoryRemoteInput {
                name,
                url,
                credential_reference,
                fetch_enabled: fetch,
                push_enabled: push,
                mirror,
            },
            &operation_context(false),
        )?)?,
        GitRepositoryCommand::RemoteRemove { repository, name } => serde_json::to_value(
            service.remove_remote(&repository, &name, &operation_context(false))?,
        )?,
        GitRepositoryCommand::Fetch { repository, remote } => serde_json::to_value(
            service
                .fetch(&repository, &remote, &operation_context(false))
                .await?,
        )?,
        GitRepositoryCommand::Push {
            repository,
            remote,
            mirror,
        } => serde_json::to_value(
            service
                .push(&repository, &remote, mirror, &operation_context(false))
                .await?,
        )?,
        GitRepositoryCommand::CloneUrl { repository, origin } => serde_json::json!({
            "url": service.clone_url(&repository, &origin)?
        }),
        GitRepositoryCommand::PlanDeployment {
            repository,
            application,
            destination,
            branch,
            strategy,
            deploy_on_push,
            keep_releases,
            health_url,
        } => {
            let configuration = repository_deployment_configuration(
                application,
                branch,
                destination,
                &strategy,
                deploy_on_push,
                keep_releases,
                health_url,
            )?;
            serde_json::to_value(
                service.plan_deployment_configuration(&repository, &configuration)?,
            )?
        }
        GitRepositoryCommand::ConfigureDeployment {
            repository,
            application,
            destination,
            branch,
            strategy,
            deploy_on_push,
            keep_releases,
            health_url,
            dry_run,
        } => {
            let configuration = repository_deployment_configuration(
                application,
                branch,
                destination,
                &strategy,
                deploy_on_push,
                keep_releases,
                health_url,
            )?;
            serde_json::to_value(service.configure_deployment(
                &repository,
                configuration,
                &operation_context(dry_run),
            )?)?
        }
    };
    println!("{}", serde_json::to_string_pretty(&value)?);
    Ok(())
}

fn repository_deployment_configuration(
    application_id: String,
    branch: String,
    destination: PathBuf,
    strategy: &str,
    deploy_on_push: bool,
    keep_releases: usize,
    health_url: Option<String>,
) -> Result<lumic_core::repository::RepositoryDeploymentConfiguration, Box<dyn std::error::Error>> {
    use lumic_core::repository::{
        DeploymentHealthConfiguration, DeploymentStrategy, RepositoryDeploymentConfiguration,
    };
    let strategy = match strategy {
        "atomic" => DeploymentStrategy::Atomic,
        "in_place" => DeploymentStrategy::InPlace,
        _ => return Err("strategy must be `atomic` or `in_place`".into()),
    };
    let health = health_url.map_or_else(DeploymentHealthConfiguration::default, |url| {
        DeploymentHealthConfiguration {
            enabled: true,
            url,
            ..DeploymentHealthConfiguration::default()
        }
    });
    Ok(RepositoryDeploymentConfiguration {
        enabled: true,
        application_id,
        branch,
        destination,
        strategy,
        deploy_on_push,
        keep_releases,
        install_command: None,
        build_command: None,
        migrate_command: None,
        hooks: Vec::new(),
        shared_directories: Vec::new(),
        shared_files: Vec::new(),
        health,
    })
}

fn run_environment(command: EnvironmentCommand) -> Result<(), Box<dyn std::error::Error>> {
    let infrastructure = infrastructure_service();
    let context = operation_context(false);
    match command {
        EnvironmentCommand::SecretGenerate { reference } => {
            let reference = infrastructure.generate_secret(&reference, &context)?;
            println!("Generated target-local secret reference: {reference}");
        }
        EnvironmentCommand::ReferenceSet {
            application,
            name,
            reference,
        } => {
            let application = application_service().set_environment_reference(
                &application,
                &name,
                &reference,
                &context,
            )?;
            println!("{}", serde_json::to_string_pretty(&application)?);
        }
        EnvironmentCommand::SecretSet { application, name } => {
            let mut value = Vec::new();
            std::io::stdin().read_to_end(&mut value)?;
            if value.last() == Some(&b'\n') {
                value.pop();
                if value.last() == Some(&b'\r') {
                    value.pop();
                }
            }
            let application = application_service().set_environment_secret(
                &application,
                &name,
                &value,
                &context,
            )?;
            println!("{}", serde_json::to_string_pretty(&application)?);
        }
        EnvironmentCommand::SecretRotate { application, name } => {
            let application =
                application_service().rotate_environment_secret(&application, &name, &context)?;
            println!("{}", serde_json::to_string_pretty(&application)?);
        }
        EnvironmentCommand::SecretDelete { application, name } => {
            let application =
                application_service().delete_environment_secret(&application, &name, &context)?;
            println!("{}", serde_json::to_string_pretty(&application)?);
        }
        EnvironmentCommand::Export {
            application,
            environment,
            tier,
            output,
        } => {
            let bundle = infrastructure.export_environment(
                &application,
                &environment,
                tier.into(),
                &context,
            )?;
            write_or_print_json(&bundle, output.as_deref())?;
        }
        EnvironmentCommand::Import {
            bundle,
            target,
            tier,
            domain,
            environment,
            services,
        } => {
            let bundle: EnvironmentBundle = read_json(&bundle)?;
            let application = infrastructure.import_environment(
                &bundle,
                &EnvironmentTransform {
                    target_id: target,
                    target_tier: tier.into(),
                    target_domain: domain,
                    environment_reference_overrides: environment.into_iter().collect(),
                    service_id_overrides: services.into_iter().collect(),
                },
                &context,
            )?;
            println!("{}", serde_json::to_string_pretty(&application)?);
        }
        EnvironmentCommand::Diff { source, target } => {
            let source: EnvironmentBundle = read_json(&source)?;
            let target: EnvironmentBundle = read_json(&target)?;
            println!(
                "{}",
                serde_json::to_string_pretty(&infrastructure.diff_environments(&source, &target))?
            );
        }
    }
    Ok(())
}

async fn run_infrastructure(
    command: InfrastructureCommand,
) -> Result<(), Box<dyn std::error::Error>> {
    let infrastructure = infrastructure_service();
    let context = operation_context(false);
    let value = match command {
        InfrastructureCommand::Status => serde_json::to_value(infrastructure.read_model()?)?,
        InfrastructureCommand::Init { id, name, roles } => {
            serde_json::to_value(infrastructure.initialize_node(
                &id,
                &name,
                roles.into_iter().map(Into::into).collect(),
                &context,
            )?)?
        }
        InfrastructureCommand::Enrollment { endpoint, output } => {
            let enrollment = infrastructure.enrollment(&endpoint)?;
            write_or_print_json(&enrollment, output.as_deref())?;
            return Ok(());
        }
        InfrastructureCommand::Register { enrollment } => {
            let enrollment: NodeEnrollment = read_json(&enrollment)?;
            serde_json::to_value(infrastructure.register_node(enrollment, &context)?)?
        }
        InfrastructureCommand::Revoke { node } => {
            serde_json::to_value(infrastructure.revoke_node(&node, &context)?)?
        }
        InfrastructureCommand::Endpoint {
            id,
            provider_node,
            provider_kind,
            provider,
            consumer_node,
            consumer_kind,
            consumer,
            protocol,
            host,
            port,
            health_path,
            secret_reference,
        } => serde_json::to_value(infrastructure.register_endpoint(
            ResourceEndpoint {
                id,
                provider_node_id: provider_node,
                provider_kind,
                provider_id: provider,
                consumer_node_id: consumer_node,
                consumer_kind,
                consumer_id: consumer,
                protocol,
                host,
                port,
                health_path,
                secret_reference,
            },
            &context,
        )?)?,
        InfrastructureCommand::Membership {
            kind,
            environment,
            application,
            node,
            enabled,
        } => serde_json::to_value(infrastructure.register_membership(
            MembershipKind::parse(&kind)?,
            &environment,
            &application,
            &node,
            enabled,
            &context,
        )?)?,
        InfrastructureCommand::Health { node } => {
            serde_json::to_value(infrastructure.check_node_health(&node).await?)?
        }
        InfrastructureCommand::Coordinate {
            environment,
            members,
        } => serde_json::to_value(infrastructure.begin_coordination(
            &environment,
            members,
            &context,
        )?)?,
        InfrastructureCommand::Report {
            coordination,
            node,
            status,
            healthy,
            deployment,
            message,
        } => serde_json::to_value(infrastructure.report_coordination_member(
            &coordination,
            &node,
            deployment_status(&status)?,
            healthy,
            deployment,
            message,
            &context,
        )?)?,
        InfrastructureCommand::Sign {
            target,
            operation,
            application,
            ttl,
            output,
        } => {
            let request = infrastructure.sign_remote_request(
                &target,
                RemoteOperation {
                    kind: operation,
                    resource_id: application,
                    arguments: BTreeMap::new(),
                },
                ttl,
            )?;
            write_or_print_json(&request, output.as_deref())?;
            return Ok(());
        }
        InfrastructureCommand::Apply { request } => {
            let request: SignedRemoteRequest = read_json(&request)?;
            serde_json::to_value(
                infrastructure
                    .execute_remote_request(&request, &context)
                    .await?,
            )?
        }
    };
    println!("{}", serde_json::to_string_pretty(&value)?);
    Ok(())
}

fn deployment_status(value: &str) -> Result<DeploymentMemberStatus, lumic_core::LumicError> {
    match value {
        "pending" => Ok(DeploymentMemberStatus::Pending),
        "running" => Ok(DeploymentMemberStatus::Running),
        "succeeded" => Ok(DeploymentMemberStatus::Succeeded),
        "failed" => Ok(DeploymentMemberStatus::Failed),
        "rolled_back" => Ok(DeploymentMemberStatus::RolledBack),
        _ => Err(lumic_core::LumicError::InvalidInput {
            field: "status".into(),
            message: "unsupported deployment member status".into(),
        }),
    }
}

fn read_json<T: serde::de::DeserializeOwned>(
    path: &PathBuf,
) -> Result<T, Box<dyn std::error::Error>> {
    Ok(serde_json::from_slice(&fs::read(path)?)?)
}

fn write_or_print_json(
    value: &impl serde::Serialize,
    output: Option<&std::path::Path>,
) -> Result<(), Box<dyn std::error::Error>> {
    let rendered = format!("{}\n", serde_json::to_string_pretty(value)?);
    if let Some(output) = output {
        fs::write(output, rendered)?;
        println!("Wrote {}", output.display());
    } else {
        print!("{rendered}");
    }
    Ok(())
}

async fn run_recipe(command: RecipeCommand) -> Result<(), Box<dyn std::error::Error>> {
    let manager = recipe_manager();
    let value = match command {
        RecipeCommand::Catalog => serde_json::to_value(manager.catalog())?,
        RecipeCommand::List => serde_json::to_value(manager.list()?)?,
        RecipeCommand::Inspect { app } => serde_json::to_value(manager.inspect(&app)?)?,
        RecipeCommand::Plan(args) => {
            serde_json::to_value(manager.plan_install(&recipe_request(args))?)?
        }
        RecipeCommand::Install(args) => serde_json::to_value(
            manager
                .install(&recipe_request(args), &operation_context(false))
                .await?,
        )?,
        RecipeCommand::Update { app } => {
            serde_json::to_value(manager.update(&app, &operation_context(false)).await?)?
        }
        RecipeCommand::Uninstall { app } => {
            serde_json::to_value(manager.uninstall(&app, &operation_context(false))?)?
        }
    };
    println!("{}", serde_json::to_string_pretty(&value)?);
    Ok(())
}

async fn run_server(command: ServerCommand) -> Result<(), Box<dyn std::error::Error>> {
    let operator = HostOperator::at_state_dir(state_directory());
    let context = operation_context(false);
    let value = match command {
        ServerCommand::Snapshot => serde_json::to_value(operator.snapshot().await?)?,
        ServerCommand::UserCreate { name } => {
            serde_json::to_value(operator.create_user(&name, &context).await?)?
        }
        ServerCommand::UserDelete { name } => {
            serde_json::to_value(operator.delete_user(&name, &context).await?)?
        }
        ServerCommand::GroupCreate { name } => {
            serde_json::to_value(operator.create_group(&name, &context).await?)?
        }
        ServerCommand::GroupDelete { name } => {
            serde_json::to_value(operator.delete_group(&name, &context).await?)?
        }
        ServerCommand::GroupAddMember { group, user } => {
            serde_json::to_value(operator.add_group_member(&group, &user, &context).await?)?
        }
        ServerCommand::Permissions {
            path,
            owner,
            group,
            mode,
        } => {
            let mode = u32::from_str_radix(mode.trim_start_matches("0o"), 8)
                .map_err(|_| "mode must be an octal value such as 0750")?;
            serde_json::to_value(
                operator
                    .set_permissions(&path, &owner, &group, mode, &context)
                    .await?,
            )?
        }
        ServerCommand::FirewallList => serde_json::to_value(operator.firewall_status().await?)?,
        ServerCommand::FirewallRule {
            decision,
            port,
            protocol,
            source,
            remove,
        } => serde_json::to_value(
            operator
                .apply_firewall_rule(
                    &FirewallRule {
                        decision: decision.into(),
                        port,
                        protocol: protocol.into(),
                        source,
                    },
                    remove,
                    &context,
                )
                .await?,
        )?,
        ServerCommand::Listeners => serde_json::to_value(operator.listeners().await?)?,
        ServerCommand::Mounts => serde_json::to_value(operator.mounts()?)?,
        ServerCommand::Processes { limit } => serde_json::to_value(operator.processes(limit)?)?,
        ServerCommand::ProcessSignal { pid, signal } => {
            serde_json::to_value(operator.signal_process(pid, signal.into(), &context)?)?
        }
        ServerCommand::Timers => serde_json::to_value(operator.timers().await?)?,
        ServerCommand::Updates => serde_json::to_value(operator.updates().await?)?,
        ServerCommand::UpdateApply { scope } => {
            serde_json::to_value(operator.apply_updates(scope.into(), &context).await?)?
        }
        ServerCommand::Logs {
            unit,
            priority,
            since,
            query,
            lines,
        } => serde_json::to_value(
            operator
                .search_journal(
                    unit.as_deref(),
                    priority.as_deref(),
                    since.as_deref(),
                    query.as_deref(),
                    lines,
                )
                .await?,
        )?,
        ServerCommand::BackupSchedule {
            id,
            service,
            database,
            on_calendar,
            enabled,
        } => serde_json::to_value(
            operator
                .schedule_backup(
                    BackupSchedule {
                        id,
                        service_id: service,
                        database,
                        on_calendar,
                        enabled,
                    },
                    &context,
                )
                .await?,
        )?,
        ServerCommand::RemediateRestart { unit } => serde_json::to_value(
            operator
                .remediate(RemediationAction::RestartService { unit }, &context)
                .await?,
        )?,
        ServerCommand::RemediateTerminate { pid } => serde_json::to_value(
            operator
                .remediate(RemediationAction::TerminateProcess { pid }, &context)
                .await?,
        )?,
        ServerCommand::RemediateJournal { older_than_days } => serde_json::to_value(
            operator
                .remediate(
                    RemediationAction::VacuumJournal { older_than_days },
                    &context,
                )
                .await?,
        )?,
    };
    println!("{}", serde_json::to_string_pretty(&value)?);
    Ok(())
}

fn recipe_request(args: RecipeRequestArgs) -> RecipeInstallRequest {
    RecipeInstallRequest {
        recipe_id: args.recipe,
        application_id: args.app,
        domain: args.domain,
        repository_url: args.repository,
        branch: args.branch,
        tls_email: args.tls_email,
        environment: args.environment.into_iter().collect(),
    }
}

fn parse_key_value(value: &str) -> Result<(String, String), String> {
    value
        .split_once('=')
        .map(|(key, value)| (key.into(), value.into()))
        .ok_or_else(|| "environment values must use NAME=VALUE".into())
}

fn parse_argv_command(value: &str) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let command: Vec<String> = serde_json::from_str(value)?;
    lumic_core::application::validate_command(&command)?;
    Ok(command)
}

fn parse_argv_commands(values: &[String]) -> Result<Vec<Vec<String>>, Box<dyn std::error::Error>> {
    values
        .iter()
        .map(|value| parse_argv_command(value))
        .collect()
}

fn render_json_or_deployment(
    deployment: &lumic_core::application::Deployment,
    json: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    if json {
        println!("{}", serde_json::to_string_pretty(deployment)?);
    } else {
        println!(
            "{} {:?} {}",
            deployment.id, deployment.status, deployment.commit
        );
    }
    Ok(())
}

async fn run_app(command: AppCommand) -> Result<(), Box<dyn std::error::Error>> {
    let service = application_service();
    match command {
        AppCommand::List { json } => {
            let applications = service.list()?;
            if json {
                println!("{}", serde_json::to_string_pretty(&applications)?);
            } else if applications.is_empty() {
                println!("No applications configured.");
            } else {
                for app in applications {
                    println!(
                        "{} {} {:?} {}",
                        app.id, app.domain, app.runtime, app.health_status
                    );
                }
            }
        }
        AppCommand::Create {
            name,
            domain,
            runtime,
            www,
            json,
        } => {
            let app = service.create(
                &name,
                &domain,
                runtime.into(),
                www,
                &operation_context(false),
            )?;
            render_json_or_app(&app, json)?;
        }
        AppCommand::Inspect { app, json } => render_json_or_app(&service.inspect(&app)?, json)?,
        AppCommand::Manifest { command } => match command {
            ManifestCommand::Inspect {
                repository_root,
                json,
            } => {
                let manifest = service.inspect_manifest(&repository_root)?;
                if json {
                    println!("{}", serde_json::to_string_pretty(&manifest)?);
                } else {
                    println!(
                        "Valid repository application schema {} for {}.",
                        manifest.schema_version, manifest.name
                    );
                }
            }
            ManifestCommand::Plan {
                app,
                repository_root,
                json,
            } => {
                let plan = service.plan_manifest(&app, &repository_root)?;
                if json {
                    println!("{}", serde_json::to_string_pretty(&plan)?);
                } else {
                    println!("{}", plan.summary);
                    for change in plan.changes {
                        println!(
                            "change: {} (reversible={})",
                            change.summary, change.reversible
                        );
                    }
                    for risk in plan.risks {
                        println!("risk {:?}: {}", risk.level, risk.summary);
                    }
                }
            }
            ManifestCommand::Apply {
                app,
                repository_root,
                json,
            } => {
                let application = service
                    .apply_manifest(&app, &repository_root, &operation_context(false))
                    .await?;
                render_json_or_app(&application, json)?;
            }
        },
        AppCommand::Repository { command } => match command {
            RepositoryCommand::Set {
                app,
                url,
                branch,
                credential_reference,
            } => {
                let application = service.set_repository(
                    &app,
                    &url,
                    &branch,
                    credential_reference,
                    &operation_context(false),
                )?;
                println!(
                    "Repository configured for {} on branch {}.",
                    application.id, branch
                );
            }
        },
        AppCommand::Credential { command } => match command {
            CredentialCommand::Import { name, source } => {
                let reference =
                    service.import_ssh_credential(&name, &source, &operation_context(false))?;
                println!("Imported SSH credential reference {reference}.");
            }
        },
        AppCommand::Provision {
            app,
            runtime_version,
            components,
            json,
        } => {
            let result = service
                .provision_versioned(
                    &app,
                    runtime_version.as_deref(),
                    &components,
                    &operation_context(false),
                )
                .await?;
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("Provisioned the selected runtime and owned nginx web host for {app}.");
            }
        }
        AppCommand::Health { app, path, port } => {
            service.set_health_check(&app, &path, port, &operation_context(false))?;
            println!("Health check for {app}: http://127.0.0.1:{port}{path}");
        }
        AppCommand::Process { command } => {
            let (app, process) = match command {
                ProcessCommand::Worker { app, name, command } => (
                    app,
                    ApplicationProcess {
                        name,
                        kind: ApplicationProcessKind::Worker,
                        command,
                        schedule: None,
                        enabled: true,
                        environment: Default::default(),
                        working_directory: None,
                        restart_policy: Default::default(),
                        health_check: None,
                    },
                ),
                ProcessCommand::Schedule {
                    app,
                    name,
                    on_calendar,
                    command,
                } => (
                    app,
                    ApplicationProcess {
                        name,
                        kind: ApplicationProcessKind::Schedule,
                        command,
                        schedule: Some(ApplicationSchedule::calendar(on_calendar)),
                        enabled: true,
                        environment: Default::default(),
                        working_directory: None,
                        restart_policy: Default::default(),
                        health_check: None,
                    },
                ),
            };
            let configured = service
                .add_process(&app, process, &operation_context(false))
                .await?;
            println!(
                "Configured {}: {}.",
                configured.process,
                configured.units.join(", ")
            );
        }
        AppCommand::Tls { app, email } => {
            service
                .enable_tls(&app, &email, &operation_context(false))
                .await?;
            println!("HTTPS enabled for {app}.");
        }
        AppCommand::Plan { app, json } => {
            let plan = service.plan_deployment(&app)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&plan)?);
            } else {
                println!("{}", plan.summary);
                for change in plan.changes {
                    println!(
                        "change: {} (reversible={})",
                        change.summary, change.reversible
                    );
                }
                for risk in plan.risks {
                    println!("risk {:?}: {}", risk.level, risk.summary);
                }
            }
        }
        AppCommand::Deploy { app, json } => {
            let deployment = service.deploy(&app, &operation_context(false)).await?;
            if json {
                println!("{}", serde_json::to_string_pretty(&deployment)?);
            } else {
                println!("Deployed {} at commit {}.", app, deployment.commit);
            }
        }
        AppCommand::ConfigureDeployment {
            app,
            pre_deploy_commands,
            build_command,
            migrate_command,
            post_deploy_commands,
            node_command,
            primary_port,
            secondary_port,
            drain_seconds,
        } => {
            use lumic_core::application::{DeploymentWorkflow, NodeHandoff};
            let node_handoff = node_command
                .as_deref()
                .map(
                    |command| -> Result<NodeHandoff, Box<dyn std::error::Error>> {
                        Ok(NodeHandoff {
                            command: parse_argv_command(command)?,
                            primary_port: primary_port.ok_or_else(|| {
                                std::io::Error::new(
                                    std::io::ErrorKind::InvalidInput,
                                    "--primary-port is required",
                                )
                            })?,
                            secondary_port: secondary_port.ok_or_else(|| {
                                std::io::Error::new(
                                    std::io::ErrorKind::InvalidInput,
                                    "--secondary-port is required",
                                )
                            })?,
                            drain_seconds,
                        })
                    },
                )
                .transpose()?;
            let workflow = DeploymentWorkflow {
                pre_deploy: parse_argv_commands(&pre_deploy_commands)?,
                build: build_command
                    .as_deref()
                    .map(parse_argv_command)
                    .transpose()?,
                migrate: migrate_command
                    .as_deref()
                    .map(parse_argv_command)
                    .transpose()?,
                post_deploy: parse_argv_commands(&post_deploy_commands)?,
                node_handoff,
            };
            service.configure_deployment(&app, workflow, &operation_context(false))?;
            println!("Deployment workflow configured for {app}.");
        }
        AppCommand::Cancel {
            app,
            deployment,
            json,
        } => {
            let result = service.cancel_deployment(&app, &deployment, &operation_context(false))?;
            render_json_or_deployment(&result, json)?;
        }
        AppCommand::Redeploy {
            app,
            deployment,
            json,
        } => {
            let result = service
                .redeploy(&app, &deployment, &operation_context(false))
                .await?;
            render_json_or_deployment(&result, json)?;
        }
        AppCommand::Logs {
            app,
            deployment,
            after,
            json,
            follow,
        } => {
            let mut cursor = after;
            loop {
                let entries = service.deployment_logs(&app, &deployment, cursor)?;
                if json {
                    for entry in &entries {
                        println!("{}", serde_json::to_string(entry)?);
                    }
                } else {
                    for entry in &entries {
                        println!("{} {:?} {}", entry.sequence, entry.stream, entry.message);
                    }
                }
                if let Some(last) = entries.last() {
                    cursor = last.sequence;
                }
                if !follow {
                    break;
                }
                let active = service.deployments(&app)?.into_iter().any(|item| {
                    item.id == deployment
                        && matches!(
                            item.status,
                            lumic_core::application::DeploymentStatus::Started
                                | lumic_core::application::DeploymentStatus::Cancelling
                        )
                });
                if !active {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
        }
        AppCommand::Deployments { app, json } => {
            let deployments = service.deployments(&app)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&deployments)?);
            } else {
                for deployment in deployments {
                    println!(
                        "{} {:?} {}",
                        deployment.id, deployment.status, deployment.commit
                    );
                }
            }
        }
        AppCommand::Rollback { app, json } => {
            let deployment = service.rollback(&app, &operation_context(false)).await?;
            if json {
                println!("{}", serde_json::to_string_pretty(&deployment)?);
            } else {
                println!("Rolled back {} to {}.", app, deployment.release_path);
            }
        }
        AppCommand::Delete { app } => {
            service.delete(&app, &operation_context(false))?;
            println!("Deleted {app}; files were moved to Lumic trash.");
        }
    }
    Ok(())
}

async fn run_service(command: ServiceCommand) -> Result<(), Box<dyn std::error::Error>> {
    let manager = SystemdServiceManager::at_state_dir(state_directory());
    match command {
        ServiceCommand::Inspect { unit, json } => {
            let status = manager.inspect(&unit).await?;
            if json {
                println!("{}", serde_json::to_string_pretty(&status)?);
            } else {
                println!(
                    "{} loaded={} active={} sub={} enabled={}",
                    status.unit,
                    status.load_state,
                    status.active_state,
                    status.sub_state,
                    status.enabled
                );
            }
        }
        other => {
            let (unit, action) = match other {
                ServiceCommand::Start { unit } => (unit, ServiceAction::Start),
                ServiceCommand::Stop { unit } => (unit, ServiceAction::Stop),
                ServiceCommand::Restart { unit } => (unit, ServiceAction::Restart),
                ServiceCommand::Reload { unit } => (unit, ServiceAction::Reload),
                ServiceCommand::Enable { unit } => (unit, ServiceAction::Enable),
                ServiceCommand::Disable { unit } => (unit, ServiceAction::Disable),
                ServiceCommand::Inspect { .. } => unreachable!(),
            };
            let mutation = manager
                .apply(&unit, action, &operation_context(false))
                .await?;
            println!("{:?} {} changed={}", action, unit, mutation.changed);
        }
    }
    Ok(())
}

async fn run_managed_service(
    command: ManagedServiceCommand,
) -> Result<(), Box<dyn std::error::Error>> {
    let manager = ManagedServiceManager::at_state_dir(state_directory());
    let value = match command {
        ManagedServiceCommand::Catalog => serde_json::to_value(manager.catalog()?)?,
        ManagedServiceCommand::Schema { definition } => {
            serde_json::to_value(manager.schema(&definition)?)?
        }
        ManagedServiceCommand::List => serde_json::to_value(manager.list()?)?,
        ManagedServiceCommand::Detect { definition } => {
            serde_json::to_value(manager.detect_catalog(&definition).await?)?
        }
        ManagedServiceCommand::Inspect { service } => {
            serde_json::to_value(manager.inspect(&service).await?)?
        }
        ManagedServiceCommand::PlanInstall {
            service,
            definition,
        } => serde_json::to_value(manager.plan_catalog_install(&service, &definition)?)?,
        ManagedServiceCommand::Install {
            service,
            definition,
            dry_run,
        } => serde_json::to_value(
            manager
                .install_catalog(&service, &definition, &operation_context(dry_run))
                .await?,
        )?,
        ManagedServiceCommand::Configure {
            service,
            bind,
            port,
            settings,
            dry_run,
        } => {
            let mut configuration = manager.inspect(&service).await?.service.configuration;
            if let Some(bind) = bind {
                configuration.bind_address = bind;
            }
            if let Some(port) = port {
                configuration.port = port;
            }
            configuration.settings = parse_settings(settings)?;
            serde_json::to_value(
                manager
                    .configure(&service, configuration, &operation_context(dry_run))
                    .await?,
            )?
        }
        ManagedServiceCommand::Start { service, dry_run } => serde_json::to_value(
            manager
                .lifecycle(&service, ServiceAction::Start, &operation_context(dry_run))
                .await?,
        )?,
        ManagedServiceCommand::Stop { service, dry_run } => serde_json::to_value(
            manager
                .lifecycle(&service, ServiceAction::Stop, &operation_context(dry_run))
                .await?,
        )?,
        ManagedServiceCommand::Restart { service, dry_run } => serde_json::to_value(
            manager
                .lifecycle(
                    &service,
                    ServiceAction::Restart,
                    &operation_context(dry_run),
                )
                .await?,
        )?,
        ManagedServiceCommand::Update { service, dry_run } => serde_json::to_value(
            manager
                .update(&service, &operation_context(dry_run))
                .await?,
        )?,
        ManagedServiceCommand::Remove { service, dry_run } => serde_json::to_value(
            manager
                .remove(&service, false, &operation_context(dry_run))
                .await?,
        )?,
        ManagedServiceCommand::Logs { service, lines } => {
            serde_json::to_value(manager.logs(&service, lines).await?)?
        }
        ManagedServiceCommand::DeclareDependency {
            service,
            dependency,
            purpose,
            required,
            dry_run,
        } => serde_json::to_value(manager.declare_dependency(
            &service,
            &dependency,
            &purpose,
            required,
            &operation_context(dry_run),
        )?)?,
        ManagedServiceCommand::DatabaseCreate {
            service,
            database,
            owner,
            dry_run,
        } => serde_json::to_value(
            manager
                .create_database(
                    &service,
                    &database,
                    owner.as_deref(),
                    &operation_context(dry_run),
                )
                .await?,
        )?,
        ManagedServiceCommand::UserCreate {
            service,
            user,
            dry_run,
        } => serde_json::to_value(
            manager
                .create_database_user(&service, &user, &operation_context(dry_run))
                .await?,
        )?,
        ManagedServiceCommand::Grant {
            service,
            database,
            user,
            dry_run,
        } => serde_json::to_value(
            manager
                .grant_database(&service, &database, &user, &operation_context(dry_run))
                .await?,
        )?,
        ManagedServiceCommand::Backup {
            service,
            database,
            dry_run,
        } => serde_json::to_value(
            manager
                .backup(&service, database.as_deref(), &operation_context(dry_run))
                .await?,
        )?,
        ManagedServiceCommand::BackupVerify { backup } => {
            serde_json::to_value(manager.verify_backup(&backup)?)?
        }
        ManagedServiceCommand::Restore {
            service,
            backup,
            dry_run,
        } => serde_json::to_value(
            manager
                .restore(&service, &backup, &operation_context(dry_run))
                .await?,
        )?,
        ManagedServiceCommand::Attach {
            service,
            app,
            role,
            database,
            user,
        } => serde_json::to_value(manager.attach_to_application(
            &application_service(),
            &app,
            ApplicationServiceReference {
                service_id: service,
                role,
                service_type: None,
                database,
                user,
                secret_reference: None,
            },
            &operation_context(false),
        )?)?,
    };
    println!("{}", serde_json::to_string_pretty(&value)?);
    Ok(())
}

fn parse_settings(values: Vec<String>) -> Result<BTreeMap<String, String>, lumic_core::LumicError> {
    values
        .into_iter()
        .map(|value| {
            let (key, value) =
                value
                    .split_once('=')
                    .ok_or_else(|| lumic_core::LumicError::InvalidInput {
                        field: "setting".into(),
                        message: "settings must use key=value syntax".into(),
                    })?;
            Ok((key.to_owned(), value.to_owned()))
        })
        .collect()
}

fn render_json_or_app(
    application: &lumic_core::application::Application,
    json: bool,
) -> Result<(), serde_json::Error> {
    if json {
        println!("{}", serde_json::to_string_pretty(application)?);
    } else {
        println!(
            "{} {} {:?} {}",
            application.id, application.domain, application.runtime, application.health_status
        );
    }
    Ok(())
}

async fn run_package(command: PackageCommand) -> Result<(), Box<dyn std::error::Error>> {
    let manager = AptPackageManager::system(event_store());
    match command {
        PackageCommand::Search { query } => {
            for package in manager.search(&query).await? {
                render_package(&package);
            }
        }
        PackageCommand::Inspect { package } => render_package(&manager.inspect(&package).await?),
        PackageCommand::Install { package, dry_run } => render_mutation(
            &manager
                .install(&package, &operation_context(dry_run))
                .await?,
        ),
        PackageCommand::Remove { package, dry_run } => render_mutation(
            &manager
                .remove(&package, &operation_context(dry_run))
                .await?,
        ),
        PackageCommand::UpdateIndex => {
            render_mutation(&manager.update_index(&operation_context(false)).await?)
        }
        PackageCommand::Allowed => {
            for package in manager.policy().allowed() {
                println!("{package}");
            }
        }
    }
    Ok(())
}

fn render_package(package: &PackageRecord) {
    println!(
        "{} installed={} candidate={}{}",
        package.name,
        package.installed_version.as_deref().unwrap_or("no"),
        package.candidate_version.as_deref().unwrap_or("unknown"),
        package
            .summary
            .as_deref()
            .map(|summary| format!(" — {summary}"))
            .unwrap_or_default()
    );
}

fn render_mutation(mutation: &PackageMutation) {
    println!(
        "{} {}: {}",
        mutation.action,
        mutation.package,
        if mutation.changed {
            "changed"
        } else {
            "unchanged"
        }
    );
    if !mutation.output.is_empty() {
        println!("{}", mutation.output);
    }
}

fn operation_context(dry_run: bool) -> OperationContext {
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    OperationContext {
        actor: std::env::var("SUDO_USER")
            .or_else(|_| std::env::var("USER"))
            .unwrap_or_else(|_| "local".into()),
        interface: OperationInterface::Cli,
        correlation_id: format!("cli-{}-{timestamp}", std::process::id()),
        dry_run,
        approved: !dry_run,
    }
}

fn event_store() -> EventStore {
    EventStore::at_state_dir(state_directory())
}

fn audit_store() -> AuditStore {
    AuditStore::at_state_dir(state_directory())
}

fn state_directory() -> PathBuf {
    std::env::var_os("LUMIC_STATE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/var/lib/lumic"))
}

fn application_service() -> ApplicationService {
    let state_directory = state_directory();
    let apps_root = std::env::var_os("LUMIC_APPS_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| state_directory.join("apps"));
    ApplicationService::new(state_directory, apps_root)
}

fn attention_service() -> AttentionService {
    let state_directory = state_directory();
    let apps_root = std::env::var_os("LUMIC_APPS_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| state_directory.join("apps"));
    AttentionService::new(state_directory, apps_root)
}

fn intelligence_service() -> ApplicationIntelligence {
    let state_directory = state_directory();
    let apps_root = std::env::var_os("LUMIC_APPS_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| state_directory.join("apps"));
    ApplicationIntelligence::new(state_directory, apps_root)
}

fn infrastructure_service() -> InfrastructureService {
    let state_directory = state_directory();
    let apps_root = std::env::var_os("LUMIC_APPS_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| state_directory.join("apps"));
    InfrastructureService::new(state_directory, apps_root)
}

fn recipe_manager() -> RecipeManager {
    let state_directory = state_directory();
    let apps_root = std::env::var_os("LUMIC_APPS_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| state_directory.join("apps"));
    RecipeManager::at_state_dir(state_directory, apps_root)
}

fn render_status(facts: &HostFacts) -> String {
    let architecture = match facts.architecture {
        Architecture::X86_64 => "x86_64",
        Architecture::Aarch64 => "aarch64",
    };
    let root_disk = facts.disks.first();
    format!(
        "Lumic {}\nNode: {}\nOS: {}\nKernel: {}\nArchitecture: {}\nCPU: {} logical cores\nMemory: {} total, {} available\nRoot disk: {} total, {} available\n",
        env!("CARGO_PKG_VERSION"),
        facts.hostname,
        facts.distribution.version_name,
        facts.kernel_release,
        architecture,
        facts.cpu_count,
        format_bytes(facts.memory.total_bytes),
        format_bytes(facts.memory.available_bytes),
        format_bytes(root_disk.map_or(0, |disk| disk.total_bytes)),
        format_bytes(root_disk.map_or(0, |disk| disk.available_bytes)),
    )
}

fn format_bytes(bytes: u64) -> String {
    const GIB: u64 = 1024 * 1024 * 1024;
    const MIB: u64 = 1024 * 1024;
    if bytes >= GIB {
        format!("{:.1} GiB", bytes as f64 / GIB as f64)
    } else {
        format!("{:.1} MiB", bytes as f64 / MIB as f64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumic_core::{DiskFacts, Distribution, DistributionFacts, MemoryFacts, OperatingSystem};

    #[test]
    fn human_status_contains_node_and_resources() {
        let facts = HostFacts {
            operating_system: OperatingSystem::Linux,
            distribution: DistributionFacts {
                distribution: Distribution::Debian,
                version_id: "12".into(),
                version_name: "Debian GNU/Linux 12".into(),
            },
            architecture: Architecture::X86_64,
            hostname: "node-01".into(),
            kernel_release: "6.1.0".into(),
            cpu_count: 2,
            memory: MemoryFacts {
                total_bytes: 2 * 1024 * 1024 * 1024,
                available_bytes: 1024 * 1024 * 1024,
                swap_total_bytes: 0,
                swap_free_bytes: 0,
            },
            disks: vec![DiskFacts {
                mount_point: "/".into(),
                filesystem: "ext4".into(),
                total_bytes: 20 * 1024 * 1024 * 1024,
                available_bytes: 10 * 1024 * 1024 * 1024,
            }],
        };
        let rendered = render_status(&facts);
        assert!(rendered.contains("Node: node-01"));
        assert!(rendered.contains("CPU: 2 logical cores"));
        assert!(rendered.contains("Memory: 2.0 GiB total"));
    }
}
