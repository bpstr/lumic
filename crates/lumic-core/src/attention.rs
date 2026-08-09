use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodePersonality {
    #[default]
    Professional,
    Dry,
    Grumpy,
    Paranoid,
    Cheerful,
    Idiot,
}

impl NodePersonality {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Professional => "professional",
            Self::Dry => "dry",
            Self::Grumpy => "grumpy",
            Self::Paranoid => "paranoid",
            Self::Cheerful => "cheerful",
            Self::Idiot => "idiot",
        }
    }
}

impl std::fmt::Display for NodePersonality {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttentionSeverity {
    #[default]
    Healthy,
    Notice,
    Warning,
    Critical,
}

impl AttentionSeverity {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Healthy => "healthy",
            Self::Notice => "notice",
            Self::Warning => "warning",
            Self::Critical => "critical",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttentionFact {
    pub key: String,
    pub label: String,
    pub value: String,
    pub evidence: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttentionChange {
    pub timestamp_unix_ms: u128,
    pub event_type: String,
    pub resource: String,
    pub summary: String,
    pub evidence: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttentionItem {
    pub id: String,
    pub severity: AttentionSeverity,
    pub summary: String,
    pub evidence: String,
    pub recommendation: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttentionSummary {
    pub generated_at_unix_ms: u128,
    pub period_start_unix_ms: u128,
    pub period_end_unix_ms: u128,
    pub severity: AttentionSeverity,
    pub facts: Vec<AttentionFact>,
    pub changes: Vec<AttentionChange>,
    pub active_incidents: Vec<AttentionItem>,
    pub upcoming_attention: Vec<AttentionItem>,
    pub recommendations: Vec<AttentionItem>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttentionReport {
    pub personality: NodePersonality,
    pub rendered: String,
    pub summary: AttentionSummary,
}

pub fn render_attention(summary: &AttentionSummary, personality: NodePersonality) -> String {
    let mut lines = vec![format!(
        "HEALTH: {}",
        summary.severity.as_str().to_uppercase()
    )];
    lines.push(opening(summary.severity, personality).into());

    lines.push("\nFacts:".into());
    for fact in &summary.facts {
        lines.push(format!(
            "- {}: {} ({})",
            fact.label, fact.value, fact.evidence
        ));
    }
    render_items(&mut lines, "Active incidents", &summary.active_incidents);
    lines.push("\nRecent changes:".into());
    if summary.changes.is_empty() {
        lines.push(format!("- {}", no_changes(personality)));
    } else {
        for change in &summary.changes {
            lines.push(format!(
                "- {} {} — {}",
                change_prefix(personality),
                change.summary,
                change.evidence
            ));
        }
    }
    render_items(
        &mut lines,
        "Upcoming attention",
        &summary.upcoming_attention,
    );
    render_items(&mut lines, "Recommendations", &summary.recommendations);
    lines.join("\n")
}

const fn change_prefix(personality: NodePersonality) -> &'static str {
    match personality {
        NodePersonality::Professional => "Recorded:",
        NodePersonality::Dry => "Log says:",
        NodePersonality::Grumpy => "Oh joy:",
        NodePersonality::Paranoid => "Evidence:",
        NodePersonality::Cheerful => "Update:",
        NodePersonality::Idiot => "Thing happened:",
    }
}

fn render_items(lines: &mut Vec<String>, title: &str, items: &[AttentionItem]) {
    lines.push(format!("\n{title}:"));
    if items.is_empty() {
        lines.push("- None detected.".into());
        return;
    }
    for item in items {
        lines.push(format!(
            "- [{}] {} — Evidence: {} — Action: {}",
            item.severity.as_str().to_uppercase(),
            item.summary,
            item.evidence,
            item.recommendation
        ));
    }
}

const fn opening(severity: AttentionSeverity, personality: NodePersonality) -> &'static str {
    match (personality, severity) {
        (_, AttentionSeverity::Critical) => "Immediate attention is required.",
        (NodePersonality::Professional, AttentionSeverity::Healthy) => {
            "The node is operating normally based on the available evidence."
        }
        (NodePersonality::Professional, _) => "The node is operating, with items to review.",
        (NodePersonality::Dry, AttentionSeverity::Healthy) => "No drama detected.",
        (NodePersonality::Dry, _) => "There is paperwork.",
        (NodePersonality::Grumpy, AttentionSeverity::Healthy) => "Everything works. For now.",
        (NodePersonality::Grumpy, _) => "Something needs fixing. Again.",
        (NodePersonality::Paranoid, AttentionSeverity::Healthy) => {
            "No current warning evidence. Continue monitoring."
        }
        (NodePersonality::Paranoid, _) => "Review every flagged item and verify the evidence.",
        (NodePersonality::Cheerful, AttentionSeverity::Healthy) => "Everything looks good!",
        (NodePersonality::Cheerful, _) => "A few things need attention; they are listed below.",
        (NodePersonality::Idiot, AttentionSeverity::Healthy) => {
            "Server good. Lights metaphorically green."
        }
        (NodePersonality::Idiot, _) => "Server has things. Look at the list.",
    }
}

const fn no_changes(personality: NodePersonality) -> &'static str {
    match personality {
        NodePersonality::Professional => "No relevant changes were recorded in this period.",
        NodePersonality::Dry => "Nothing happened. Allegedly.",
        NodePersonality::Grumpy => "No recorded changes. One less thing to complain about.",
        NodePersonality::Paranoid => {
            "No recorded changes; absence of evidence is not evidence of absence."
        }
        NodePersonality::Cheerful => "No recorded changes in this period.",
        NodePersonality::Idiot => "No change things found.",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn summary() -> AttentionSummary {
        let item = AttentionItem {
            id: "disk-root".into(),
            severity: AttentionSeverity::Critical,
            summary: "root disk is nearly full".into(),
            evidence: "4% available".into(),
            recommendation: "remove bounded, reviewed data".into(),
        };
        AttentionSummary {
            generated_at_unix_ms: 2,
            period_start_unix_ms: 1,
            period_end_unix_ms: 2,
            severity: AttentionSeverity::Critical,
            facts: vec![AttentionFact {
                key: "hostname".into(),
                label: "Hostname".into(),
                value: "example".into(),
                evidence: "/proc/sys/kernel/hostname".into(),
            }],
            changes: Vec::new(),
            active_incidents: vec![item.clone()],
            upcoming_attention: Vec::new(),
            recommendations: vec![item],
        }
    }

    #[test]
    fn every_personality_keeps_critical_facts_and_actions_obvious() {
        for personality in [
            NodePersonality::Professional,
            NodePersonality::Dry,
            NodePersonality::Grumpy,
            NodePersonality::Paranoid,
            NodePersonality::Cheerful,
            NodePersonality::Idiot,
        ] {
            let rendered = render_attention(&summary(), personality);
            assert!(rendered.starts_with("HEALTH: CRITICAL"));
            assert!(rendered.contains("root disk is nearly full"));
            assert!(rendered.contains("4% available"));
            assert!(rendered.contains("remove bounded, reviewed data"));
        }
    }
}
