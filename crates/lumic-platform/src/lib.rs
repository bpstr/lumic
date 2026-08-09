use lumic_core::HostFacts;

pub fn inspect_host() -> HostFacts {
    HostFacts::new(std::env::consts::OS, std::env::consts::ARCH)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_non_empty_platform_facts() {
        let facts = inspect_host();
        assert!(!facts.os.is_empty());
        assert!(!facts.architecture.is_empty());
    }
}
