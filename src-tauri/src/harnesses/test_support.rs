use sage_domain::harness::HarnessDefinition;

pub(super) fn by_id(definitions: &[HarnessDefinition], id: &str) -> HarnessDefinition {
    definitions
        .iter()
        .find(|definition| definition.id == id)
        .unwrap()
        .clone()
}
