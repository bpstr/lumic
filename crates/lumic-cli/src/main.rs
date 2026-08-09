use clap::{Parser, Subcommand, ValueEnum};
use lumic_core::{
    Architecture, HostFacts, OperationContext, OperationInterface,
    application::ApplicationRuntime,
    package::{PackageMutation, PackageName, PackageRecord},
};
use lumic_platform::{
    application::ApplicationService, apt::AptPackageManager, event_store::EventStore,
};
use std::path::PathBuf;

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
    /// Create, inspect, deploy, and roll back applications.
    App {
        #[command(subcommand)]
        command: AppCommand,
    },
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
}

impl From<RuntimeArg> for ApplicationRuntime {
    fn from(value: RuntimeArg) -> Self {
        match value {
            RuntimeArg::Static => Self::Static,
            RuntimeArg::Php => Self::Php,
        }
    }
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
        Command::App { command } => run_app(command).await?,
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
    let state_directory = std::env::var_os("LUMIC_STATE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/var/lib/lumic"));
    EventStore::at_state_dir(state_directory)
}

fn application_service() -> ApplicationService {
    let state_directory = std::env::var_os("LUMIC_STATE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/var/lib/lumic"));
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
