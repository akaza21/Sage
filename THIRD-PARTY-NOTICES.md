# Third-party notices

## pingdotgg/t3code

Source: pingdotgg/t3code, commit `08a463e`. License: MIT.

Adapted and ported modules:
- `packages/provider-core/src/server/handoffBudget.ts` -> `src-tauri/src/acp/context_handoff/{budget,history,render}.rs`.
- `apps/server/src/orchestration-v2/ProviderSessionTransitionPolicy.ts` -> `src-tauri/src/acp/service/session_transition.rs`.
- `apps/server/src/orchestration-v2/ContextHandoffDelivery.ts` -> `src-tauri/src/acp/context_handoff/delivery.rs` and `src-tauri/src/acp/store/context_handoff.rs`.
- `apps/server/src/orchestration-v2/ContextHandoffBudget.test.ts` -> `src-tauri/src/acp/context_handoff/tests.rs`, `src-tauri/src/acp/store/session_replace_tests.rs` and `src-tauri/src/acp/fixture/handoff_external_tests.rs`.
- `src-tauri/src/chat_attention/snapshot.rs` and `src-tauri/src/chat_attention/store.rs`: quiet semantic summaries and owned child-input rollup, from `packages/client-runtime/src/state/threadShell.ts` (`sameApartFromStreaming`, `quietThreadShellAtomFamily`, `childThreadInputsAtomFamily`).
- `src/domain/chatAttention.ts`: visit/unseen terminal rules and request/running/failure precedence, from `apps/web/src/components/Sidebar.logic.ts` (`resolveThreadLastVisitedAt`, `hasUnseenCompletion`, `hasUnseenFailure`, `resolveSidebarThreadStatus`).
- `src-tauri/src/chat_attention/store.rs`: the corresponding native sidebar-status projection from `apps/web/src/components/Sidebar.logic.ts`.
- `tests/ui/fixtures/agentTurnJourneyAttention.ts`: the bounded browser-service fixture counterpart of quiet semantic summaries from `packages/client-runtime/src/state/threadShell.ts`.

Full upstream LICENSE:

MIT License

Copyright (c) 2026 T3 Tools Inc.

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
