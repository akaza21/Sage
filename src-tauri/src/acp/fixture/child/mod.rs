mod activity_fixture_adapter;
mod activity_observer;
mod item_observer;
mod permission_fixture_adapter;
mod permission_observer;
mod permission_store_fixture;
pub(in crate::acp::fixture) mod runtime_fixture;
mod semantic_observer;
mod stale_projection_observer;
mod stderr_observer;
mod unknown_observer;
mod wire_observer;

#[cfg(windows)]
pub(super) use activity_fixture_adapter::ActivityFixtureAdapter;
pub(super) use activity_observer::{CASE_IDS as ACTIVITY_CASE_IDS, execute as execute_activity};
pub(super) use item_observer::{CASE_IDS as ITEM_CASE_IDS, execute as execute_item};
pub(super) use permission_observer::{
    CASE_IDS as PERMISSION_CASE_IDS, execute as execute_permission,
};
#[cfg(windows)]
pub(super) use permission_store_fixture::{BINDING_DIGEST, PermissionStoreFixture};
pub(super) use semantic_observer::{CASE_IDS as SEMANTIC_CASE_IDS, execute as execute_semantic};
pub(super) use stale_projection_observer::{
    CASE_IDS as STALE_PROJECTION_CASE_IDS, execute as execute_stale_projection,
};
pub(super) use stderr_observer::{CASE_IDS as STDERR_CASE_IDS, execute as execute_stderr};
pub(super) use unknown_observer::{CASE_IDS as UNKNOWN_CASE_IDS, execute as execute_unknown};
pub(super) use wire_observer::{CASE_IDS as WIRE_CASE_IDS, execute as execute_wire};
