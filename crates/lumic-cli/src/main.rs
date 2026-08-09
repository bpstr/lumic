use clap::{Parser, Subcommand, ValueEnum};
use lumic_core::{
    Architecture, HostFacts, OperationContext, OperationInterface,
    application::{
        ApplicationProcess, ApplicationProcessKind, ApplicationRuntime, ApplicationServiceReference,
    },
    managed_service::ManagedServiceKind,
    package::{PackageMutation, PackageName, PackageRecord},
};
use lumic_platform::{
    application::ApplicationService,
    apt::AptPackageManager,
    audit_store::AuditStore,
    diagnostics::diagnose_host,
    event_store::EventStore,
    managed_service::ManagedServiceManager,
    self_update::SelfUpdateManager,
    systemd::{ServiceAction, SystemdServiceManager},
};
use std::{collections::BTreeMap, path::PathBuf};

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
    /// Inspect and operate validated systemd units.
    Service {
        #[command(subcommand)]
        command: ServiceCommand,
    },
    /// Manage native PostgreSQL and Redis resources through stable Lumic contracts.
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
    /// Configure the authenticated local operator UI.
    Ui {
        #[command(subcommand)]
        command: UiCommand,
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

#[derive(Debug, Clone, Copy, ValueEnum)]
enum ManagedServiceKindArg {
    Postgresql,
    Redis,
}

impl From<ManagedServiceKindArg> for ManagedServiceKind {
    fn from(value: ManagedServiceKindArg) -> Self {
        match value {
            ManagedServiceKindArg::Postgresql => Self::Postgresql,
            ManagedServiceKindArg::Redis => Self::Redis,
        }
    }
}

#[derive(Subcommand)]
enum ManagedServiceCommand {
    List,
    Detect {
        #[arg(value_enum)]
        kind: ManagedServiceKindArg,
    },
    Inspect {
        service: String,
    },
    PlanInstall {
        service: String,
        #[arg(value_enum)]
        kind: ManagedServiceKindArg,
    },
    Install {
        service: String,
        #[arg(value_enum)]
        kind: ManagedServiceKindArg,
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
    },
    Stop {
        service: String,
    },
    Restart {
        service: String,
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
        Command::SelfUpdate { command } => {
            let manager = SelfUpdateManager::system(state_directory());
            match command {
                SelfUpdateCommand::Apply => {
                    let result = manager.apply_nightly(&operation_context(false)).await?;
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
        Command::Ui { command } => match command {
            UiCommand::Token {
                command: UiTokenCommand::Rotate,
            } => {
                let token =
                    lumic_ui::UiCredentialStore::at_state_dir(state_directory()).rotate()?;
                println!("Lumic UI admin token (shown once): {token}");
            }
        },
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
            components,
            json,
        } => {
            let result = service
                .provision(&app, &components, &operation_context(false))
                .await?;
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("Provisioned runtime and nginx for {app}.");
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
                        schedule: Some(on_calendar),
                        enabled: true,
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
            let deployment = service.rollback(&app, &operation_context(false))?;
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
        ManagedServiceCommand::List => serde_json::to_value(manager.list()?)?,
        ManagedServiceCommand::Detect { kind } => {
            serde_json::to_value(manager.detect(kind.into()).await?)?
        }
        ManagedServiceCommand::Inspect { service } => {
            serde_json::to_value(manager.inspect(&service).await?)?
        }
        ManagedServiceCommand::PlanInstall { service, kind } => {
            serde_json::to_value(manager.plan_install(&service, kind.into())?)?
        }
        ManagedServiceCommand::Install {
            service,
            kind,
            dry_run,
        } => serde_json::to_value(
            manager
                .install(&service, kind.into(), &operation_context(dry_run))
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
        ManagedServiceCommand::Start { service } => serde_json::to_value(
            manager
                .lifecycle(&service, ServiceAction::Start, &operation_context(false))
                .await?,
        )?,
        ManagedServiceCommand::Stop { service } => serde_json::to_value(
            manager
                .lifecycle(&service, ServiceAction::Stop, &operation_context(false))
                .await?,
        )?,
        ManagedServiceCommand::Restart { service } => serde_json::to_value(
            manager
                .lifecycle(&service, ServiceAction::Restart, &operation_context(false))
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
