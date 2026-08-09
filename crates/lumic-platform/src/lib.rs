use lumic_core::{
    Architecture, DiskFacts, Distribution, DistributionFacts, HostFacts, LumicError, MemoryFacts,
    OperatingSystem, Result,
};
use std::{
    collections::BTreeMap,
    ffi::CString,
    fs,
    path::Path,
    process::Stdio,
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    process::Command,
    time::timeout,
};

pub mod app_process;
pub mod application;
pub mod apt;
pub mod atomic_file;
pub mod audit_store;
pub mod diagnostics;
pub mod event_store;
pub mod infrastructure;
pub mod managed_service;
pub mod recipe;
pub mod runtime;
pub mod secret_store;
pub mod self_update;
pub mod server;
pub mod systemd;
pub mod web;

pub trait HostDataSource {
    fn os_release(&self) -> Result<String>;
    fn architecture(&self) -> Result<String>;
    fn hostname(&self) -> Result<String>;
    fn kernel_release(&self) -> Result<String>;
    fn cpu_count(&self) -> Result<usize>;
    fn meminfo(&self) -> Result<String>;
    fn root_disk(&self) -> Result<DiskFacts>;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct LinuxHostDataSource;

impl HostDataSource for LinuxHostDataSource {
    fn os_release(&self) -> Result<String> {
        read_fact("distribution", "/etc/os-release")
    }

    fn architecture(&self) -> Result<String> {
        Ok(std::env::consts::ARCH.to_owned())
    }

    fn hostname(&self) -> Result<String> {
        read_fact("hostname", "/proc/sys/kernel/hostname").map(|value| value.trim().to_owned())
    }

    fn kernel_release(&self) -> Result<String> {
        read_fact("kernel_release", "/proc/sys/kernel/osrelease")
            .map(|value| value.trim().to_owned())
    }

    fn cpu_count(&self) -> Result<usize> {
        std::thread::available_parallelism()
            .map(usize::from)
            .map_err(|error| inspection_error("cpu_count", error))
    }

    fn meminfo(&self) -> Result<String> {
        read_fact("memory", "/proc/meminfo")
    }

    fn root_disk(&self) -> Result<DiskFacts> {
        root_disk_facts()
    }
}

fn read_fact(fact: &str, path: impl AsRef<Path>) -> Result<String> {
    fs::read_to_string(path).map_err(|error| inspection_error(fact, error))
}

fn inspection_error(fact: &str, error: impl std::fmt::Display) -> LumicError {
    LumicError::Inspection {
        fact: fact.to_owned(),
        message: error.to_string(),
    }
}

#[allow(
    clippy::unnecessary_cast,
    reason = "libc statvfs integer widths differ between supported Unix targets"
)]
fn root_disk_facts() -> Result<DiskFacts> {
    let root = CString::new("/").expect("root path contains no NUL bytes");
    let mut stats = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    // SAFETY: `root` is a valid NUL-terminated path and `stats` points to writable memory.
    let result = unsafe { libc::statvfs(root.as_ptr(), stats.as_mut_ptr()) };
    if result != 0 {
        return Err(inspection_error(
            "root_disk",
            std::io::Error::last_os_error(),
        ));
    }
    // SAFETY: statvfs returned success and initialized `stats`.
    let stats = unsafe { stats.assume_init() };
    let fragment_size = stats.f_frsize as u64;

    Ok(DiskFacts {
        mount_point: "/".into(),
        filesystem: root_filesystem().unwrap_or_else(|| "unknown".into()),
        total_bytes: (stats.f_blocks as u64).saturating_mul(fragment_size),
        available_bytes: (stats.f_bavail as u64).saturating_mul(fragment_size),
    })
}

fn root_filesystem() -> Option<String> {
    let mounts = fs::read_to_string("/proc/self/mounts").ok()?;
    mounts.lines().find_map(|line| {
        let mut fields = line.split_whitespace();
        let _source = fields.next()?;
        let mount_point = fields.next()?;
        let filesystem = fields.next()?;
        (mount_point == "/").then(|| filesystem.to_owned())
    })
}

#[derive(Debug, Clone)]
pub struct HostStatusService<S> {
    source: S,
}

impl<S> HostStatusService<S>
where
    S: HostDataSource,
{
    pub const fn new(source: S) -> Self {
        Self { source }
    }

    pub fn inspect(&self) -> Result<HostFacts> {
        let distribution = parse_os_release(&self.source.os_release()?)?;
        let architecture = parse_architecture(&self.source.architecture()?)?;
        let memory = parse_meminfo(&self.source.meminfo()?)?;
        let hostname = required("hostname", self.source.hostname()?)?;
        let kernel_release = required("kernel_release", self.source.kernel_release()?)?;
        let cpu_count = self.source.cpu_count()?;
        if cpu_count == 0 {
            return Err(inspection_error("cpu_count", "reported zero logical CPUs"));
        }

        Ok(HostFacts {
            operating_system: OperatingSystem::Linux,
            distribution,
            architecture,
            hostname,
            kernel_release,
            cpu_count,
            memory,
            disks: vec![self.source.root_disk()?],
        })
    }
}

impl HostStatusService<LinuxHostDataSource> {
    pub const fn system() -> Self {
        Self::new(LinuxHostDataSource)
    }
}

pub fn inspect_host() -> Result<HostFacts> {
    HostStatusService::system().inspect()
}

pub fn parse_os_release(input: &str) -> Result<DistributionFacts> {
    let values = parse_key_values(input);
    let id = values.get("ID").map(String::as_str).unwrap_or("unknown");
    let distribution = match id {
        "debian" => Distribution::Debian,
        "ubuntu" => Distribution::Ubuntu,
        other => {
            return Err(LumicError::UnsupportedPlatform {
                platform: other.to_owned(),
            });
        }
    };
    let version_id = values
        .get("VERSION_ID")
        .cloned()
        .ok_or_else(|| inspection_error("distribution", "VERSION_ID is missing"))?;
    let version_name = values
        .get("PRETTY_NAME")
        .or_else(|| values.get("VERSION"))
        .cloned()
        .unwrap_or_else(|| format!("{} {version_id}", distribution.id()));

    Ok(DistributionFacts {
        distribution,
        version_id,
        version_name,
    })
}

fn parse_key_values(input: &str) -> BTreeMap<String, String> {
    input
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                return None;
            }
            let (key, value) = line.split_once('=')?;
            let value = value.trim();
            let value = if value.len() >= 2
                && ((value.starts_with('"') && value.ends_with('"'))
                    || (value.starts_with('\'') && value.ends_with('\'')))
            {
                &value[1..value.len() - 1]
            } else {
                value
            };
            Some((key.trim().to_owned(), value.to_owned()))
        })
        .collect()
}

pub fn parse_architecture(input: &str) -> Result<Architecture> {
    match input.trim() {
        "x86_64" | "amd64" => Ok(Architecture::X86_64),
        "aarch64" | "arm64" => Ok(Architecture::Aarch64),
        architecture => Err(LumicError::UnsupportedPlatform {
            platform: format!("architecture {architecture}"),
        }),
    }
}

pub fn parse_meminfo(input: &str) -> Result<MemoryFacts> {
    let mut values = BTreeMap::new();
    for line in input.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let kib = value
            .split_whitespace()
            .next()
            .and_then(|value| value.parse::<u64>().ok());
        if let Some(kib) = kib {
            values.insert(key, kib.saturating_mul(1024));
        }
    }
    let get = |key: &'static str| {
        values
            .get(key)
            .copied()
            .ok_or_else(|| inspection_error("memory", format!("{key} is missing")))
    };

    Ok(MemoryFacts {
        total_bytes: get("MemTotal")?,
        available_bytes: get("MemAvailable")?,
        swap_total_bytes: get("SwapTotal")?,
        swap_free_bytes: get("SwapFree")?,
    })
}

fn required(fact: &str, value: String) -> Result<String> {
    if value.trim().is_empty() {
        Err(inspection_error(fact, "value is empty"))
    } else {
        Ok(value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessSpec {
    pub executable: String,
    pub args: Vec<String>,
    pub timeout: Duration,
    pub stdout_limit: usize,
    pub stderr_limit: usize,
    pub current_dir: Option<std::path::PathBuf>,
    pub environment: BTreeMap<String, String>,
    /// Bytes written directly to the child stdin. Callers must never log this field.
    pub stdin: Option<Vec<u8>>,
}

impl ProcessSpec {
    pub fn new(executable: impl Into<String>) -> Self {
        Self {
            executable: executable.into(),
            args: Vec::new(),
            timeout: Duration::from_secs(30),
            stdout_limit: 64 * 1024,
            stderr_limit: 64 * 1024,
            current_dir: None,
            environment: BTreeMap::new(),
            stdin: None,
        }
    }

    pub fn args(mut self, args: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.args = args.into_iter().map(Into::into).collect();
        self
    }

    pub fn current_dir(mut self, path: impl Into<std::path::PathBuf>) -> Self {
        self.current_dir = Some(path.into());
        self
    }

    pub fn environment(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.environment.insert(key.into(), value.into());
        self
    }

    pub fn stdin(mut self, bytes: impl Into<Vec<u8>>) -> Self {
        self.stdin = Some(bytes.into());
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessOutput {
    pub exit_code: Option<i32>,
    pub signal: Option<i32>,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
    pub timed_out: bool,
    pub duration: Duration,
}

impl ProcessOutput {
    pub fn success(&self) -> bool {
        !self.timed_out && self.exit_code == Some(0)
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct ProcessRunner;

impl ProcessRunner {
    pub async fn run(&self, spec: &ProcessSpec) -> Result<ProcessOutput> {
        if spec.executable.trim().is_empty() {
            return Err(LumicError::InvalidInput {
                field: "executable".into(),
                message: "must not be empty".into(),
            });
        }

        let started = Instant::now();
        let mut command = Command::new(&spec.executable);
        command.args(&spec.args);
        command.envs(&spec.environment);
        if let Some(current_dir) = &spec.current_dir {
            command.current_dir(current_dir);
        }
        let mut child = command
            .stdin(if spec.stdin.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|error| LumicError::Process {
                executable: spec.executable.clone(),
                message: error.to_string(),
            })?;
        if let Some(input) = &spec.stdin {
            let mut stdin = child.stdin.take().expect("stdin was configured as piped");
            stdin
                .write_all(input)
                .await
                .map_err(|error| LumicError::Process {
                    executable: spec.executable.clone(),
                    message: format!("failed to write child input: {error}"),
                })?;
            stdin
                .shutdown()
                .await
                .map_err(|error| LumicError::Process {
                    executable: spec.executable.clone(),
                    message: format!("failed to close child input: {error}"),
                })?;
        }
        let stdout = child.stdout.take().expect("stdout was configured as piped");
        let stderr = child.stderr.take().expect("stderr was configured as piped");
        let stdout_task = tokio::spawn(read_bounded(stdout, spec.stdout_limit));
        let stderr_task = tokio::spawn(read_bounded(stderr, spec.stderr_limit));

        let (status, timed_out) = match timeout(spec.timeout, child.wait()).await {
            Ok(result) => (
                result.map_err(|error| LumicError::Process {
                    executable: spec.executable.clone(),
                    message: error.to_string(),
                })?,
                false,
            ),
            Err(_) => {
                child.kill().await.map_err(|error| LumicError::Process {
                    executable: spec.executable.clone(),
                    message: format!("failed to terminate timed-out process: {error}"),
                })?;
                let status = child.wait().await.map_err(|error| LumicError::Process {
                    executable: spec.executable.clone(),
                    message: format!("failed to reap timed-out process: {error}"),
                })?;
                (status, true)
            }
        };
        let (stdout, stdout_truncated) = join_reader(stdout_task, &spec.executable).await?;
        let (stderr, stderr_truncated) = join_reader(stderr_task, &spec.executable).await?;

        #[cfg(unix)]
        let signal = {
            use std::os::unix::process::ExitStatusExt;
            status.signal()
        };
        #[cfg(not(unix))]
        let signal = None;

        Ok(ProcessOutput {
            exit_code: status.code(),
            signal,
            stdout,
            stderr,
            stdout_truncated,
            stderr_truncated,
            timed_out,
            duration: started.elapsed(),
        })
    }
}

async fn read_bounded(
    mut reader: impl tokio::io::AsyncRead + Unpin,
    limit: usize,
) -> std::io::Result<(Vec<u8>, bool)> {
    let mut output = Vec::with_capacity(limit.min(8192));
    let mut truncated = false;
    let mut buffer = [0_u8; 8192];
    loop {
        let read = reader.read(&mut buffer).await?;
        if read == 0 {
            break;
        }
        let remaining = limit.saturating_sub(output.len());
        output.extend_from_slice(&buffer[..read.min(remaining)]);
        truncated |= read > remaining;
    }
    Ok((output, truncated))
}

async fn join_reader(
    task: tokio::task::JoinHandle<std::io::Result<(Vec<u8>, bool)>>,
    executable: &str,
) -> Result<(Vec<u8>, bool)> {
    task.await
        .map_err(|error| LumicError::Process {
            executable: executable.to_owned(),
            message: format!("output reader task failed: {error}"),
        })?
        .map_err(|error| LumicError::Process {
            executable: executable.to_owned(),
            message: format!("failed to read process output: {error}"),
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone)]
    struct FixtureSource {
        os_release: String,
        architecture: String,
    }

    impl HostDataSource for FixtureSource {
        fn os_release(&self) -> Result<String> {
            Ok(self.os_release.clone())
        }
        fn architecture(&self) -> Result<String> {
            Ok(self.architecture.clone())
        }
        fn hostname(&self) -> Result<String> {
            Ok("fixture-node".into())
        }
        fn kernel_release(&self) -> Result<String> {
            Ok("6.8.0-fixture".into())
        }
        fn cpu_count(&self) -> Result<usize> {
            Ok(4)
        }
        fn meminfo(&self) -> Result<String> {
            Ok(
                "MemTotal: 8192 kB\nMemAvailable: 4096 kB\nSwapTotal: 2048 kB\nSwapFree: 1024 kB\n"
                    .into(),
            )
        }
        fn root_disk(&self) -> Result<DiskFacts> {
            Ok(DiskFacts {
                mount_point: "/".into(),
                filesystem: "ext4".into(),
                total_bytes: 1000,
                available_bytes: 400,
            })
        }
    }

    #[test]
    fn inspects_ubuntu_fixture_without_using_developer_host() {
        let source = FixtureSource {
            os_release: "ID=ubuntu\nVERSION_ID=\"24.04\"\nPRETTY_NAME=\"Ubuntu 24.04 LTS\"\n"
                .into(),
            architecture: "x86_64".into(),
        };
        let facts = HostStatusService::new(source).inspect().unwrap();
        assert_eq!(facts.distribution.distribution, Distribution::Ubuntu);
        assert_eq!(facts.distribution.version_id, "24.04");
        assert_eq!(facts.architecture, Architecture::X86_64);
        assert_eq!(facts.hostname, "fixture-node");
        assert_eq!(facts.cpu_count, 4);
        assert_eq!(facts.memory.total_bytes, 8 * 1024 * 1024);
        assert_eq!(facts.disks[0].filesystem, "ext4");
    }

    #[test]
    fn inspects_debian_arm_fixture() {
        let source = FixtureSource {
            os_release: "ID=debian\nVERSION_ID='13'\nPRETTY_NAME=\"Debian GNU/Linux 13\"\n".into(),
            architecture: "aarch64".into(),
        };
        let facts = HostStatusService::new(source).inspect().unwrap();
        assert_eq!(facts.distribution.distribution, Distribution::Debian);
        assert_eq!(facts.architecture, Architecture::Aarch64);
    }

    #[test]
    fn rejects_unsupported_distribution() {
        let error = parse_os_release("ID=fedora\nVERSION_ID=42\n").unwrap_err();
        assert!(matches!(error, LumicError::UnsupportedPlatform { .. }));
    }

    #[tokio::test]
    async fn process_runner_keeps_arguments_separate_and_captures_exit_metadata() {
        let spec = ProcessSpec::new("sh").args([
            "-c",
            "printf '%s' \"$1\"; exit 7",
            "sh",
            "a; echo unsafe",
        ]);
        let output = ProcessRunner.run(&spec).await.unwrap();
        assert_eq!(output.stdout, b"a; echo unsafe");
        assert_eq!(output.exit_code, Some(7));
        assert!(!output.success());
    }

    #[tokio::test]
    async fn process_runner_bounds_output() {
        let mut spec = ProcessSpec::new("sh").args(["-c", "printf 123456789"]);
        spec.stdout_limit = 4;
        let output = ProcessRunner.run(&spec).await.unwrap();
        assert_eq!(output.stdout, b"1234");
        assert!(output.stdout_truncated);
    }

    #[tokio::test]
    async fn process_runner_sends_sensitive_input_over_stdin_not_arguments() {
        let spec = ProcessSpec::new("sh")
            .args(["-c", "read value; printf '%s' \"$value\""])
            .stdin(b"private-value\n".to_vec());
        assert!(
            !spec
                .args
                .iter()
                .any(|argument| argument.contains("private-value"))
        );
        let output = ProcessRunner.run(&spec).await.unwrap();
        assert_eq!(output.stdout, b"private-value");
    }

    #[tokio::test]
    async fn process_runner_terminates_on_timeout() {
        let mut spec = ProcessSpec::new("sh").args(["-c", "sleep 2"]);
        spec.timeout = Duration::from_millis(30);
        let output = ProcessRunner.run(&spec).await.unwrap();
        assert!(output.timed_out);
        assert!(!output.success());
        assert!(output.duration < Duration::from_secs(1));
    }
}
