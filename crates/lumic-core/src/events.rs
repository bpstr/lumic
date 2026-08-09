use crate::OperationInterface;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Event {
    pub timestamp_unix_ms: u128,
    pub event_type: String,
    pub actor: String,
    pub interface: OperationInterface,
    pub entity: String,
    pub entity_id: String,
    pub correlation_id: String,
    pub payload: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AuditRecord {
    pub timestamp_unix_ms: u128,
    pub actor: String,
    pub interface: OperationInterface,
    pub capability: String,
    pub operation: String,
    pub entity: String,
    pub entity_id: String,
    pub correlation_id: String,
    pub arguments: Value,
    pub before: Option<Value>,
    pub after: Option<Value>,
    pub succeeded: bool,
    pub message: String,
}

impl AuditRecord {
    #[allow(clippy::too_many_arguments)]
    pub fn now(
        context: &crate::OperationContext,
        capability: impl Into<String>,
        operation: impl Into<String>,
        entity: impl Into<String>,
        entity_id: impl Into<String>,
        arguments: Value,
        before: Option<Value>,
        after: Option<Value>,
        succeeded: bool,
        message: impl Into<String>,
    ) -> Self {
        Self {
            timestamp_unix_ms: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis(),
            actor: context.actor.clone(),
            interface: context.interface,
            capability: capability.into(),
            operation: operation.into(),
            entity: entity.into(),
            entity_id: entity_id.into(),
            correlation_id: context.correlation_id.clone(),
            arguments,
            before,
            after,
            succeeded,
            message: message.into(),
        }
    }
}

impl Event {
    pub fn now(
        event_type: impl Into<String>,
        actor: impl Into<String>,
        interface: OperationInterface,
        entity: impl Into<String>,
        entity_id: impl Into<String>,
        correlation_id: impl Into<String>,
        payload: Value,
    ) -> Self {
        Self {
            timestamp_unix_ms: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis(),
            event_type: event_type.into(),
            actor: actor.into(),
            interface,
            entity: entity.into(),
            entity_id: entity_id.into(),
            correlation_id: correlation_id.into(),
            payload,
        }
    }
}
