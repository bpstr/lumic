use crate::{apt::AptPackageManager, event_store::EventStore};
use lumic_core::{
    LumicError, OperationContext, Result,
    application::ApplicationRuntime,
    package::{PackageMutation, PackageName},
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeInstallResult {
    pub runtime: ApplicationRuntime,
    pub components: Vec<String>,
    pub packages: Vec<PackageMutation>,
}

#[derive(Debug, Clone)]
pub struct RuntimeManager {
    packages: AptPackageManager,
}

impl RuntimeManager {
    pub fn at_state_dir(state_dir: impl AsRef<std::path::Path>) -> Self {
        Self {
            packages: AptPackageManager::system(EventStore::at_state_dir(state_dir)),
        }
    }

    pub async fn install(
        &self,
        runtime: ApplicationRuntime,
        components: &[String],
        context: &OperationContext,
    ) -> Result<RuntimeInstallResult> {
        let mut names = match runtime {
            ApplicationRuntime::Static => vec!["nginx"],
            ApplicationRuntime::Php => vec!["php-fpm", "php-cli", "composer", "nginx"],
            ApplicationRuntime::Node => vec!["nodejs", "nginx"],
        };
        for component in components {
            names.push(component_package(runtime, component)?);
        }
        names.sort_unstable();
        names.dedup();
        let mut packages = Vec::new();
        for name in names {
            packages.push(
                self.packages
                    .install(&PackageName::parse(name)?, context)
                    .await?,
            );
        }
        Ok(RuntimeInstallResult {
            runtime,
            components: components.to_vec(),
            packages,
        })
    }
}

fn component_package(runtime: ApplicationRuntime, component: &str) -> Result<&'static str> {
    let package = match (runtime, component) {
        (ApplicationRuntime::Php, "curl") => "php-curl",
        (ApplicationRuntime::Php, "intl") => "php-intl",
        (ApplicationRuntime::Php, "mbstring") => "php-mbstring",
        (ApplicationRuntime::Php, "xml") => "php-xml",
        (ApplicationRuntime::Php, "zip") => "php-zip",
        _ => {
            return Err(LumicError::InvalidInput {
                field: "component".into(),
                message: format!("{component} is not in the trusted catalog for {runtime:?}"),
            });
        }
    };
    Ok(package)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn component_catalog_is_explicit() {
        assert_eq!(
            component_package(ApplicationRuntime::Php, "intl").unwrap(),
            "php-intl"
        );
        assert!(component_package(ApplicationRuntime::Node, "npm-shell-plugin").is_err());
    }
}
