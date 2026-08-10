use crate::{ProcessRunner, ProcessSpec, artifact::ArtifactManager};
use lumic_core::{LumicError, Result, recipe::RecipeArtifact};
#[cfg(unix)]
use std::os::unix::fs::symlink;
use std::{
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

#[derive(Debug, Clone)]
pub struct WordPressApplyInput<'a> {
    pub application_id: &'a str,
    pub domain: &'a str,
    pub site_title: &'a str,
    pub admin_user: &'a str,
    pub admin_email: &'a str,
    pub admin_password: &'a [u8],
    pub database: &'a str,
    pub database_user: &'a str,
    pub database_password: &'a [u8],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WordPressApplyResult {
    pub release_path: PathBuf,
    pub source_artifact: PathBuf,
    pub wp_cli_artifact: PathBuf,
}

#[derive(Debug, Clone)]
pub struct WordPressInstaller {
    artifacts: ArtifactManager,
    apps_root: PathBuf,
    runner: ProcessRunner,
}

impl WordPressInstaller {
    pub fn new(state_dir: impl AsRef<Path>, apps_root: impl Into<PathBuf>) -> Self {
        Self {
            artifacts: ArtifactManager::at_state_dir(state_dir),
            apps_root: apps_root.into(),
            runner: ProcessRunner,
        }
    }

    pub async fn apply(
        &self,
        input: &WordPressApplyInput<'_>,
        source: &RecipeArtifact,
        wp_cli: &RecipeArtifact,
    ) -> Result<WordPressApplyResult> {
        source.validate()?;
        wp_cli.validate()?;
        let source_path = self.artifacts.ensure(source, "tar.gz").await?.artifact.path;
        let wp_cli_path = self.artifacts.ensure(wp_cli, "phar").await?.artifact.path;
        let root = self.apps_root.join(input.application_id);
        let release = root
            .join("releases")
            .join(format!("wordpress-{}", source.version));
        if !release.join("wp-settings.php").is_file() {
            self.extract_release(&source_path, &release).await?;
        }
        let previous = current_target(&root)?;
        activate_release(&root, &release)?;
        let result = async {
            self.configure_and_install(input, &release, &wp_cli_path)
                .await?;
            self.run_checked(
                ProcessSpec::new("chown")
                    .args([
                        "-R",
                        "www-data:www-data",
                        release.to_string_lossy().as_ref(),
                    ])
                    .current_dir(&release),
            )
            .await
        }
        .await;
        if let Err(error) = result {
            restore_current(&root, previous.as_deref())?;
            return Err(error);
        }
        Ok(WordPressApplyResult {
            release_path: release,
            source_artifact: source_path,
            wp_cli_artifact: wp_cli_path,
        })
    }

    pub async fn is_installed(
        &self,
        application_id: &str,
        wp_cli: &RecipeArtifact,
    ) -> Result<bool> {
        let release = self.apps_root.join(application_id).join("current");
        if !release.join("wp-settings.php").is_file() {
            return Ok(false);
        }
        let Some(inspection) = self.artifacts.inspect(wp_cli, "phar")? else {
            return Ok(false);
        };
        if !inspection.verified {
            return Ok(false);
        }
        let wp_cli_path = inspection.path;
        let output = self
            .runner
            .run(&wp_command(
                &wp_cli_path,
                &release,
                ["core", "is-installed"],
            ))
            .await?;
        Ok(output.success())
    }

    async fn extract_release(&self, archive: &Path, release: &Path) -> Result<()> {
        let releases = release
            .parent()
            .ok_or_else(|| invalid("release", "release path has no parent"))?;
        fs::create_dir_all(releases).map_err(io)?;
        let staging = releases.join(format!(".wordpress-{}-staging", std::process::id()));
        if staging.exists() {
            fs::remove_dir_all(&staging).map_err(io)?;
        }
        fs::create_dir_all(&staging).map_err(io)?;
        let extraction = self
            .run_checked(ProcessSpec::new("tar").args([
                "--extract",
                "--gzip",
                "--file",
                archive.to_string_lossy().as_ref(),
                "--directory",
                staging.to_string_lossy().as_ref(),
                "--no-same-owner",
                "--no-same-permissions",
            ]))
            .await;
        if let Err(error) = extraction {
            let _ = fs::remove_dir_all(&staging);
            return Err(error);
        }
        let unpacked = staging.join("wordpress");
        if !unpacked.join("wp-settings.php").is_file()
            || !unpacked.join("wp-includes/version.php").is_file()
        {
            let _ = fs::remove_dir_all(&staging);
            return Err(invalid(
                "artifact",
                "WordPress archive is missing required files",
            ));
        }
        if release.exists() {
            fs::remove_dir_all(release).map_err(io)?;
        }
        fs::rename(&unpacked, release).map_err(io)?;
        fs::remove_dir_all(&staging).map_err(io)?;
        Ok(())
    }

    async fn configure_and_install(
        &self,
        input: &WordPressApplyInput<'_>,
        release: &Path,
        wp_cli: &Path,
    ) -> Result<()> {
        if !release.join("wp-config.php").is_file() {
            let mut password = input.database_password.to_vec();
            password.push(b'\n');
            self.run_checked(
                wp_command(
                    wp_cli,
                    release,
                    vec![
                        "config".into(),
                        "create".into(),
                        format!("--dbname={}", input.database),
                        format!("--dbuser={}", input.database_user),
                        "--dbhost=localhost".into(),
                        "--prompt=dbpass".into(),
                        "--skip-check".into(),
                    ],
                )
                .stdin(password),
            )
            .await?;
        }
        let installed = self
            .runner
            .run(&wp_command(wp_cli, release, ["core", "is-installed"]))
            .await?
            .success();
        if !installed {
            let mut password = input.admin_password.to_vec();
            password.push(b'\n');
            self.run_checked(
                wp_command(
                    wp_cli,
                    release,
                    vec![
                        "core".into(),
                        "install".into(),
                        format!("--url=http://{}", input.domain),
                        format!("--title={}", input.site_title),
                        format!("--admin_user={}", input.admin_user),
                        format!("--admin_email={}", input.admin_email),
                        "--prompt=admin_password".into(),
                        "--skip-email".into(),
                    ],
                )
                .stdin(password),
            )
            .await?;
        }
        Ok(())
    }

    async fn run_checked(&self, mut spec: ProcessSpec) -> Result<()> {
        spec.timeout = Duration::from_secs(300);
        let executable = spec.executable.clone();
        let output = self.runner.run(&spec).await?;
        if output.success() {
            Ok(())
        } else {
            Err(LumicError::Process {
                executable,
                message: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
            })
        }
    }
}

fn wp_command(
    wp_cli: &Path,
    release: &Path,
    args: impl IntoIterator<Item = impl Into<String>>,
) -> ProcessSpec {
    let mut command = vec![
        wp_cli.to_string_lossy().into_owned(),
        format!("--path={}", release.display()),
        "--allow-root".into(),
    ];
    command.extend(args.into_iter().map(Into::into));
    ProcessSpec::new("php8.3")
        .args(command)
        .current_dir(release)
}

fn current_target(root: &Path) -> Result<Option<PathBuf>> {
    let current = root.join("current");
    match fs::read_link(&current) {
        Ok(target) => Ok(Some(target)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(io(error)),
    }
}

#[cfg(unix)]
fn activate_release(root: &Path, release: &Path) -> Result<()> {
    let current = root.join("current");
    if current.exists() && !current.is_symlink() {
        return Err(invalid(
            "release",
            "managed current path is not a symbolic link",
        ));
    }
    let temporary = root.join(format!(".current-{}", std::process::id()));
    if temporary.symlink_metadata().is_ok() {
        fs::remove_file(&temporary).map_err(io)?;
    }
    symlink(release, &temporary).map_err(io)?;
    fs::rename(&temporary, &current).map_err(io)
}

#[cfg(not(unix))]
fn activate_release(_root: &Path, _release: &Path) -> Result<()> {
    Err(invalid(
        "release",
        "WordPress activation requires Unix symbolic links",
    ))
}

fn restore_current(root: &Path, previous: Option<&Path>) -> Result<()> {
    match previous {
        Some(target) => activate_release(root, target),
        None => {
            let current = root.join("current");
            if current.symlink_metadata().is_ok() {
                fs::remove_file(current).map_err(io)?;
            }
            Ok(())
        }
    }
}

fn invalid(field: &str, message: &str) -> LumicError {
    LumicError::InvalidInput {
        field: field.into(),
        message: message.into(),
    }
}

fn io(error: std::io::Error) -> LumicError {
    LumicError::Internal {
        message: format!("WordPress lifecycle I/O failed: {error}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let path =
            std::env::temp_dir().join(format!("lumic-wordpress-{name}-{}", std::process::id()));
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[cfg(unix)]
    #[test]
    fn activation_can_restore_the_previous_release() {
        let root = temp_dir("rollback");
        let first = root.join("releases/first");
        let second = root.join("releases/second");
        fs::create_dir_all(&first).unwrap();
        fs::create_dir_all(&second).unwrap();
        activate_release(&root, &first).unwrap();
        let previous = current_target(&root).unwrap();
        activate_release(&root, &second).unwrap();
        restore_current(&root, previous.as_deref()).unwrap();
        assert_eq!(fs::read_link(root.join("current")).unwrap(), first);
        fs::remove_dir_all(root).unwrap();
    }
}
