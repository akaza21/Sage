//! Curated drift metadata for an admitted binding: the compatibility family
//! of the admitted build, the family of a build a session recorded, and the
//! runtime that continues a session across a proven-compatible drift.

use crate::acp::bindings::{compatibility::CompatibilityFamily, runtime::RuntimeFactory};

use super::{AdmittedBinding, AdmittedBindingRecord, BindingProjectRoot};

/// How one binding's builds relate across a binding change. Only bindings
/// with a curated compatibility table have one; their sessions are the only
/// ones that may continue under a different exact build.
#[derive(Clone, Copy)]
pub(crate) struct DriftLineage {
    /// The family of the admitted build; `None` when it is uncharacterised.
    pub(crate) admitted: Option<CompatibilityFamily>,
    /// Looks up the family of a harness version a session recorded.
    pub(crate) recorded: fn(&str) -> Option<CompatibilityFamily>,
}

impl AdmittedBindingRecord<'_> {
    pub(crate) fn drift_lineage(&self) -> Option<DriftLineage> {
        match &self.binding {
            AdmittedBinding::Claude(binding) => Some(DriftLineage {
                admitted: binding.family(),
                recorded: crate::acp::bindings::compatibility::claude_recorded_family,
            }),
            _ => None,
        }
    }

    /// The runtime factory for a session whose durable binding scope is
    /// `session_scope` while this exact build runs it. `None` for a binding
    /// without drift lineage, which therefore never continues a session.
    pub(crate) fn into_continued_factory(
        self,
        root: BindingProjectRoot,
        mcp: crate::acp::mcp::McpAdmission,
        session_scope: [u8; 32],
    ) -> Option<Box<dyn RuntimeFactory>> {
        match (self.binding, root) {
            (AdmittedBinding::Claude(binding), BindingProjectRoot::Claude(root)) => Some(Box::new(
                crate::acp::bindings::claude::ClaudeRuntimeFactory::continuing(
                    binding,
                    root,
                    mcp,
                    session_scope,
                ),
            )),
            _ => None,
        }
    }
}
