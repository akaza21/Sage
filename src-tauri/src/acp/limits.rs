use serde::Serialize;
pub(crate) mod preview;
pub(crate) use preview::PREVIEW_LIMITS;
mod browser;
pub use browser::{BROWSER_LIMITS, BrowserLimits};
mod fixture;
pub use fixture::{FIXTURE_LIMITS, FixtureLimits};

pub(crate) const RETAINED_TOOL_OUTPUT_LINES: usize = 200;
/// Raw UTF-8 bytes admitted per immutable file-diff side, before JSON escaping.
pub(crate) const DIFF_SIDE_BYTES_MAX: usize = 262_144;
/// Retained immutable versions per Turn, including multiple versions of one item.
pub(crate) const DIFFS_PER_TURN: i64 = 32;
pub const COMMAND_RESOURCE_LIMIT: usize = 64;
/// A prompt carries only a small review set, not an unbounded annotation log.
pub const ATTACHED_CONTEXT_LIMIT: usize = 8;
/// One attached user note is capped at one quarter of the WebView composer budget.
pub const ATTACHED_CONTEXT_NOTE_BYTES: usize = 4 * 1024;
/// Images one prompt may carry; the composer attaches at most this many.
pub const PROMPT_IMAGE_COUNT: usize = 4;
/// An attached image as the user supplies it, before normalisation. A 4K or
/// 5K screenshot fits; decoding one holds at most `IMAGE_UPLOAD_PIXELS` * 4
/// bytes (64 MiB) natively for the duration of one upload.
pub const IMAGE_UPLOAD_BYTES: usize = 10 * 1024 * 1024;
pub const IMAGE_UPLOAD_EDGE_PX: u32 = 8192;
pub const IMAGE_UPLOAD_PIXELS: u64 = 16_777_216;
/// The long edge an attached image is normalised to. Claude's vision input
/// resizes anything longer than about 1568 px, so more pixels carry nothing.
pub const PROMPT_IMAGE_EDGE_PX: u32 = 1568;
/// A normalised image stays PNG (lossless, best for screenshots) up to this
/// size; past it the smaller of PNG and a quality-85 JPEG is kept. A typical
/// 1568 px UI screenshot is 150-500 KiB as PNG.
pub const PROMPT_IMAGE_PNG_BYTES: usize = 512 * 1024;
/// Decoded bytes of every normalised image in one prompt together, and so of
/// any one image. Roughly three or four normalised screenshots. Its base64
/// (1.33 MiB) plus the 120 KiB text-and-file dispatch budget fits one
/// `PROMPT_FRAME_BYTES` frame.
pub const PROMPT_IMAGE_TOTAL_BYTES: usize = 1024 * 1024;
/// The one outbound frame class larger than `WireLimits::frame_bytes`: an
/// ACP `session/prompt` request carrying images. It is bounded by the egress
/// queue's data capacity so a prompt can always be queued when the lane is
/// idle; inbound frames keep the 1 MiB bound.
pub const PROMPT_FRAME_BYTES: usize =
    (LIMITS.queues.egress_bytes - LIMITS.queues.egress_control_bytes_inside_total) as usize - 1;
/// Decoded bytes of the transcript copy of one sent image. Its base64 must fit
/// one normalized item (`WireLimits::normalized_item_bytes`).
pub const IMAGE_PREVIEW_BYTES: usize = 192 * 1024 - 512;
/// A one-byte sensitive token may expand to this replacement in command output.
pub(crate) const COMMAND_OUTPUT_REDACTION_BYTES: usize = "[redacted]".len();

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Limits {
    pub wire: WireLimits,
    pub codex: CodexLimits,
    pub queues: QueueLimits,
    pub process: ProcessLimits,
    pub deadlines_ms: DeadlineLimits,
    pub durable: DurableLimits,
    pub native_thread_projection: NativeThreadLimits,
    pub migration: MigrationLimits,
    pub fixture: FixtureLimits,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WireLimits {
    pub frame_bytes: u64,
    pub json_batch_accepted: bool,
    pub json_depth: u64,
    pub json_nodes: u64,
    pub object_members: u64,
    pub array_entries: u64,
    pub decoded_key_bytes: u64,
    pub method_bytes: u64,
    pub numeric_token_bytes: u64,
    pub external_id_bytes: u64,
    pub page_cursor_bytes: u64,
    pub metadata_entries: u64,
    pub metadata_bytes: u64,
    pub absolute_path_bytes_native_only: u64,
    pub relative_path_bytes_native_only: u64,
    pub prompt_bytes: u64,
    pub capability_metadata_bytes: u64,
    pub models_or_config_entries: u64,
    pub models_or_config_bytes: u64,
    pub normalized_item_bytes: u64,
    pub retained_tool_input_bytes: u64,
    pub retained_tool_output_bytes: u64,
    pub permission_payload_bytes: u64,
    pub error_bytes: u64,
    pub unsupported_summary_bytes: u64,
    pub items_per_turn: u64,
    pub item_bytes_per_turn: u64,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexLimits {
    pub model_catalog_pages: u64,
    pub reasoning_efforts_per_model: u64,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QueueLimits {
    pub driver_data_messages: u64,
    pub driver_data_bytes: u64,
    pub driver_control_messages: u64,
    pub driver_control_bytes: u64,
    pub egress_frames: u64,
    pub egress_bytes: u64,
    pub egress_control_frames_inside_total: u64,
    pub egress_control_bytes_inside_total: u64,
    pub journal_batches: u64,
    pub journal_bytes: u64,
    pub journal_control_batches_inside_total: u64,
    pub journal_control_bytes_inside_total: u64,
    pub journal_facts_per_transaction: u64,
    pub journal_bytes_per_transaction: u64,
    pub outstanding_rpc_per_session: u64,
    pub outstanding_rpc_global: u64,
    pub server_requests_per_session: u64,
    pub server_requests_global: u64,
    pub runtime_tasks_per_session: u64,
    pub runtime_tasks_global: u64,
    pub queued_turns_per_session: u64,
    pub queued_turns_global: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessLimits {
    pub arguments: u64,
    pub argument_bytes_each: u64,
    pub argument_bytes_total: u64,
    pub environment_entries: u64,
    pub environment_key_bytes: u64,
    pub environment_value_bytes: u64,
    pub environment_bytes_total: u64,
    pub descendants_per_tree: u64,
    pub descendants_global: u64,
    pub windows_job_root_processes: u64,
    pub windows_job_active_process_limit: u64,
    pub stderr_ring_bytes: u64,
    pub stderr_line_bytes: u64,
    pub terminal_tail_bytes: u64,
    pub operational_live_trees: u64,
    pub operational_running_turns: u64,
    pub disabled_contract_tree_ceiling: u64,
    pub disabled_contract_turn_ceiling: u64,
    pub warm_trees: u64,
    pub warm_idle_ttl_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeadlineLimits {
    pub spawn: u64,
    pub initialize: u64,
    pub ordinary_rpc: u64,
    pub cancel_dispatch: u64,
    pub permission_decline_commit_and_queue: u64,
    pub resume_or_load: u64,
    pub explicit_close: u64,
    pub tree_termination: u64,
    pub wal_convergence: u64,
    pub fixture_delay_maximum: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DurableLimits {
    pub chats_per_project: u64,
    pub chats_global: u64,
    pub sessions_per_chat: u64,
    pub sessions_per_project: u64,
    pub sessions_global: u64,
    pub turns_per_session: u64,
    pub turns_per_project: u64,
    pub turns_global: u64,
    pub requests_per_session: u64,
    pub requests_global: u64,
    pub request_bytes_global: u64,
    pub items_per_session: u64,
    pub items_global: u64,
    pub item_bytes_per_session: u64,
    pub item_bytes_global: u64,
    pub facts_per_session: u64,
    pub facts_global: u64,
    pub fact_bytes_each: u64,
    pub fact_bytes_global: u64,
    pub shared_payload_bytes_global: u64,
    pub chat_metadata_bytes_each: u64,
    pub chat_metadata_bytes_per_project: u64,
    pub chat_metadata_bytes_global: u64,
    pub session_metadata_bytes_each: u64,
    pub session_metadata_bytes_global: u64,
    pub history_page_turns: u64,
    pub history_page_items: u64,
    pub history_page_bytes: u64,
    pub outstanding_page_reads_per_session: u64,
    pub outstanding_page_reads_global: u64,
    pub frontend_retained_turns: u64,
    pub frontend_retained_items: u64,
    pub frontend_retained_bytes: u64,
    pub project_or_request_intent_entries: u64,
    pub projection_mailbox_slots: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeThreadLimits {
    pub enabled_operations_in_ade0006_a: u64,
    pub observed_depth: u64,
    pub activity_groups_per_turn: u64,
    pub receivers_per_collaboration_item: u64,
    pub activity_nodes_per_page: u64,
    pub activity_label_bytes: u64,
    pub activity_message_bytes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MigrationLimits {
    pub legacy_projects: u64,
    pub legacy_roots: u64,
    pub legacy_roots_per_project: u64,
    pub legacy_tabs: u64,
    pub legacy_tabs_per_project: u64,
    pub legacy_buffers: u64,
    pub legacy_buffers_per_project: u64,
    pub legacy_sessions: u64,
    pub legacy_sessions_per_chat: u64,
    pub legacy_events: u64,
    pub legacy_events_per_session: u64,
    pub legacy_session_metadata_bytes: u64,
    pub legacy_payload_bytes_per_session: u64,
    pub legacy_payload_bytes_global: u64,
    pub conversion_rows_per_page: u64,
    pub conversion_bytes_per_page: u64,
    pub legacy_items_per_turn: u64,
    pub legacy_bytes_per_turn: u64,
    pub live_sqlite_bytes: u64,
    pub backup_or_temporary_bytes: u64,
    pub wal_checkpoint_target_bytes: u64,
    pub wal_hard_stop_bytes: u64,
    pub journal_size_limit_bytes: u64,
    pub shm_and_manifest_bytes: u64,
    pub manifest_bytes_each: u64,
    pub migration_free_bytes: u64,
    pub offline_restore_free_bytes: u64,
    pub retained_backups: u64,
    pub retained_quarantines: u64,
}

pub const LIMITS: Limits = Limits {
    wire: WireLimits {
        frame_bytes: 1_048_576,
        json_batch_accepted: false,
        json_depth: 32,
        json_nodes: 16_384,
        object_members: 256,
        array_entries: 512,
        decoded_key_bytes: 256,
        method_bytes: 128,
        numeric_token_bytes: 64,
        external_id_bytes: 512,
        page_cursor_bytes: 512,
        metadata_entries: 64,
        metadata_bytes: 65_536,
        absolute_path_bytes_native_only: 32_768,
        relative_path_bytes_native_only: 16_384,
        prompt_bytes: 100_000,
        capability_metadata_bytes: 65_536,
        models_or_config_entries: 128,
        models_or_config_bytes: 131_072,
        normalized_item_bytes: 262_144,
        retained_tool_input_bytes: 65_536,
        retained_tool_output_bytes: 65_536,
        permission_payload_bytes: 65_536,
        error_bytes: 8_192,
        unsupported_summary_bytes: 16_384,
        items_per_turn: 512,
        item_bytes_per_turn: 8_388_608,
    },
    codex: CodexLimits {
        model_catalog_pages: 4,
        reasoning_efforts_per_model: 32,
    },
    queues: QueueLimits {
        driver_data_messages: 256,
        driver_data_bytes: 2_097_152,
        driver_control_messages: 8,
        driver_control_bytes: 262_144,
        egress_frames: 64,
        egress_bytes: 2_097_152,
        egress_control_frames_inside_total: 8,
        egress_control_bytes_inside_total: 262_144,
        journal_batches: 128,
        journal_bytes: 4_194_304,
        journal_control_batches_inside_total: 8,
        journal_control_bytes_inside_total: 262_144,
        journal_facts_per_transaction: 128,
        journal_bytes_per_transaction: 4_194_304,
        outstanding_rpc_per_session: 16,
        outstanding_rpc_global: 32,
        server_requests_per_session: 8,
        server_requests_global: 16,
        runtime_tasks_per_session: 16,
        runtime_tasks_global: 64,
        queued_turns_per_session: 1,
        queued_turns_global: 1,
    },
    process: ProcessLimits {
        arguments: 32,
        argument_bytes_each: 8_192,
        argument_bytes_total: 32_768,
        environment_entries: 64,
        environment_key_bytes: 256,
        environment_value_bytes: 8_192,
        environment_bytes_total: 32_768,
        descendants_per_tree: 32,
        descendants_global: 128,
        windows_job_root_processes: 1,
        windows_job_active_process_limit: 33,
        stderr_ring_bytes: 262_144,
        stderr_line_bytes: 16_384,
        terminal_tail_bytes: 32_768,
        operational_live_trees: 4,
        operational_running_turns: 4,
        disabled_contract_tree_ceiling: 4,
        disabled_contract_turn_ceiling: 2,
        warm_trees: 0,
        warm_idle_ttl_ms: 0,
    },
    deadlines_ms: DeadlineLimits {
        spawn: 10_000,
        initialize: 30_000,
        ordinary_rpc: 30_000,
        cancel_dispatch: 100,
        permission_decline_commit_and_queue: 1_000,
        resume_or_load: 30_000,
        explicit_close: 5_000,
        tree_termination: 5_000,
        wal_convergence: 5_000,
        fixture_delay_maximum: 30_000,
    },
    durable: DurableLimits {
        chats_per_project: 64,
        chats_global: 4_096,
        sessions_per_chat: 32,
        sessions_per_project: 256,
        sessions_global: 4_096,
        turns_per_session: 5_000,
        turns_per_project: 8_192,
        turns_global: 32_768,
        requests_per_session: 512,
        requests_global: 4_096,
        request_bytes_global: 16_777_216,
        items_per_session: 5_000,
        items_global: 50_000,
        item_bytes_per_session: 67_108_864,
        item_bytes_global: 268_435_456,
        facts_per_session: 20_000,
        facts_global: 200_000,
        fact_bytes_each: 8_192,
        fact_bytes_global: 67_108_864,
        shared_payload_bytes_global: 536_870_912,
        chat_metadata_bytes_each: 8_192,
        chat_metadata_bytes_per_project: 524_288,
        chat_metadata_bytes_global: 33_554_432,
        session_metadata_bytes_each: 16_384,
        session_metadata_bytes_global: 67_108_864,
        history_page_turns: 20,
        history_page_items: 256,
        history_page_bytes: 2_097_152,
        outstanding_page_reads_per_session: 4,
        outstanding_page_reads_global: 8,
        frontend_retained_turns: 100,
        frontend_retained_items: 1_000,
        frontend_retained_bytes: 8_388_608,
        project_or_request_intent_entries: 128,
        projection_mailbox_slots: 1,
    },
    native_thread_projection: NativeThreadLimits {
        enabled_operations_in_ade0006_a: 0,
        observed_depth: 4,
        activity_groups_per_turn: 32,
        receivers_per_collaboration_item: 32,
        activity_nodes_per_page: 128,
        activity_label_bytes: 512,
        activity_message_bytes: 16_384,
    },
    migration: MigrationLimits {
        legacy_projects: 64,
        legacy_roots: 512,
        legacy_roots_per_project: 8,
        legacy_tabs: 4_096,
        legacy_tabs_per_project: 64,
        legacy_buffers: 1_024,
        legacy_buffers_per_project: 16,
        legacy_sessions: 4_096,
        legacy_sessions_per_chat: 32,
        legacy_events: 50_000,
        legacy_events_per_session: 5_000,
        legacy_session_metadata_bytes: 16_777_216,
        legacy_payload_bytes_per_session: 67_108_864,
        legacy_payload_bytes_global: 268_435_456,
        conversion_rows_per_page: 256,
        conversion_bytes_per_page: 1_048_576,
        legacy_items_per_turn: 256,
        legacy_bytes_per_turn: 4_194_304,
        live_sqlite_bytes: 1_073_741_824,
        backup_or_temporary_bytes: 1_073_741_824,
        wal_checkpoint_target_bytes: 16_777_216,
        wal_hard_stop_bytes: 33_554_432,
        journal_size_limit_bytes: 8_388_608,
        shm_and_manifest_bytes: 1_048_576,
        manifest_bytes_each: 65_536,
        migration_free_bytes: 2_254_857_831,
        offline_restore_free_bytes: 3_328_599_654,
        retained_backups: 1,
        retained_quarantines: 1,
    },
    fixture: FIXTURE_LIMITS,
};

// Prompt images ride in one prompt frame beside the base64 of the 96 KiB file
// budget and the prompt text (escaped up to twice its 100 KB bound); the
// images, their transcript previews and that text fit the driver's data lane;
// and each transcript copy fits one normalized item with its JSON envelope.
const PROMPT_FRAME_NEED: usize = (PROMPT_IMAGE_TOTAL_BYTES + 96 * 1024).div_ceil(3) * 4 + 4096;
const _: () = assert!(
    PROMPT_FRAME_NEED + 2 * (LIMITS.wire.prompt_bytes as usize) < PROMPT_FRAME_BYTES
        && PROMPT_FRAME_BYTES > LIMITS.wire.frame_bytes as usize
        && PROMPT_IMAGE_TOTAL_BYTES + 4 * IMAGE_PREVIEW_BYTES + 128 * 1024
            < LIMITS.queues.driver_data_bytes as usize
        && IMAGE_PREVIEW_BYTES.div_ceil(3) * 4 + 256 < LIMITS.wire.normalized_item_bytes as usize
);
