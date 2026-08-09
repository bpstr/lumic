//! MCP adapter boundary.
//!
//! Protocol transport is intentionally deferred until Phase 0 validates the current Rust MCP ecosystem.
//! MCP must expose typed Lumic capabilities rather than generic shell execution.

use lumic_core::HostFacts;

pub fn host_status() -> HostFacts {
    lumic_platform::inspect_host()
}
