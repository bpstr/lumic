//! Explicit dependency bindings between resource outputs and consumer inputs.

use crate::{
    LumicError, Result,
    resource::{ResourceRef, validate_output_name, validate_resource_id},
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Binding {
    pub id: String,
    pub producer: ResourceRef,
    pub output: String,
    pub consumer: ResourceRef,
    pub input: String,
    pub created_at_unix_ms: u64,
}

impl Binding {
    pub fn validate(&self) -> Result<()> {
        validate_resource_id("binding.id", &self.id)?;
        self.producer.validate()?;
        self.consumer.validate()?;
        validate_output_name("binding.output", &self.output)?;
        validate_output_name("binding.input", &self.input)?;
        if self.producer == self.consumer {
            return Err(invalid(
                "binding.consumer",
                "cannot bind a resource to itself",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct BindingGraph(pub Vec<Binding>);

impl BindingGraph {
    pub fn validate(&self) -> Result<()> {
        let mut ids = BTreeSet::new();
        let mut inputs = BTreeSet::new();
        for binding in &self.0 {
            binding.validate()?;
            if !ids.insert(binding.id.as_str()) {
                return Err(invalid("binding.id", "duplicate binding id"));
            }
            if !inputs.insert((&binding.consumer, binding.input.as_str())) {
                return Err(invalid(
                    "binding.input",
                    "a consumer input may have only one producer",
                ));
            }
        }
        self.reject_cycles()
    }

    pub fn consumers_of(&self, resource: &ResourceRef) -> Vec<&ResourceRef> {
        self.0
            .iter()
            .filter(|binding| &binding.producer == resource)
            .map(|binding| &binding.consumer)
            .collect()
    }

    pub fn assert_removable(&self, resource: &ResourceRef) -> Result<()> {
        let consumers = self.consumers_of(resource);
        if consumers.is_empty() {
            Ok(())
        } else {
            Err(invalid(
                "resource.id",
                &format!("resource has {} dependent binding(s)", consumers.len()),
            ))
        }
    }

    fn reject_cycles(&self) -> Result<()> {
        let mut edges: BTreeMap<&ResourceRef, Vec<&ResourceRef>> = BTreeMap::new();
        for binding in &self.0 {
            edges
                .entry(&binding.producer)
                .or_default()
                .push(&binding.consumer);
        }
        let mut visiting = BTreeSet::new();
        let mut visited = BTreeSet::new();
        for node in edges.keys().copied() {
            if has_cycle(node, &edges, &mut visiting, &mut visited) {
                return Err(invalid("bindings", "dependency cycle detected"));
            }
        }
        Ok(())
    }
}

fn has_cycle<'a>(
    node: &'a ResourceRef,
    edges: &BTreeMap<&'a ResourceRef, Vec<&'a ResourceRef>>,
    visiting: &mut BTreeSet<&'a ResourceRef>,
    visited: &mut BTreeSet<&'a ResourceRef>,
) -> bool {
    if visited.contains(node) {
        return false;
    }
    if !visiting.insert(node) {
        return true;
    }
    if edges.get(node).is_some_and(|next| {
        next.iter()
            .any(|next_node| has_cycle(next_node, edges, visiting, visited))
    }) {
        return true;
    }
    visiting.remove(node);
    visited.insert(node);
    false
}

fn invalid(field: &str, message: &str) -> LumicError {
    LumicError::InvalidInput {
        field: field.into(),
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resource::ResourceKind;

    fn resource(id: &str) -> ResourceRef {
        ResourceRef::new(ResourceKind::ManagedService, id).unwrap()
    }

    fn binding(id: &str, producer: &str, consumer: &str) -> Binding {
        Binding {
            id: id.into(),
            producer: resource(producer),
            output: "endpoint".into(),
            consumer: resource(consumer),
            input: format!("{producer}_endpoint"),
            created_at_unix_ms: 1,
        }
    }

    #[test]
    fn rejects_cycles_and_protects_dependencies() {
        let cycle = BindingGraph(vec![
            binding("a-to-b", "a", "b"),
            binding("b-to-a", "b", "a"),
        ]);
        assert!(cycle.validate().is_err());

        let graph = BindingGraph(vec![binding("a-to-b", "a", "b")]);
        assert!(graph.validate().is_ok());
        assert!(graph.assert_removable(&resource("a")).is_err());
        assert!(graph.assert_removable(&resource("b")).is_ok());
    }
}
