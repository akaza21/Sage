//! Paged native reads: a child's transcript, the history of a session to
//! import and the list of importable sessions. Each page is one native RPC
//! on the owner's live runtime, and a read follows at most `MAX_PAGES`.

use std::sync::Arc;

use crate::acp::{
    bindings::codex::{ChildTurn, MAX_ITEMS, MAX_TURNS},
    goal::{NativeRead, NativeRpcIntent, NativeRpcReply, NativeRpcRequest},
    runtime::{HostError, RuntimeHost},
    scheduler::RuntimeOwner,
};

use super::AcpService;

const MAX_PAGES: usize = 32;

/// Why a paged native read returned nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub(crate) enum NativeReadError {
    #[error("The harness declined to read this session.")]
    Rejected,
    #[error("This session is too large to read.")]
    TooLarge,
    #[error("The harness can't read sessions right now.")]
    Unavailable,
    #[error("The harness did not answer the read; try again.")]
    RecoveryRequired,
}

/// The Turns of a paged read, oldest first. `truncated` means older Turns
/// beyond the bounds were left out.
#[derive(Debug)]
pub(crate) struct PagedTurns {
    pub(crate) turns: Vec<ChildTurn>,
    pub(crate) truncated: bool,
}

impl AcpService {
    pub(super) fn native_page(
        &self,
        owner: &RuntimeOwner,
        host: &Arc<RuntimeHost>,
        read: NativeRead,
    ) -> Result<NativeRpcReply, NativeReadError> {
        // An import read only borrows the lister: its failure never puts the
        // lister's own chat into config recovery.
        let child = matches!(read, NativeRead::ChildTurns { .. });
        let result = host.native_rpc(NativeRpcIntent {
            owner: owner.clone(),
            request: NativeRpcRequest::Read(read),
        });
        if child
            && matches!(
                result,
                Err(HostError::Unavailable
                    | HostError::Closed
                    | HostError::Deadline
                    | HostError::DeliveryUnknown
                    | HostError::RecoveryRequired)
            )
        {
            self.retain_config_effect_recovery(owner.clone(), Arc::clone(host));
        }
        page_reply(result)
    }

    pub(super) fn read_turn_pages(
        &self,
        owner: &RuntimeOwner,
        host: &Arc<RuntimeHost>,
        read: impl Fn(Option<String>) -> NativeRead,
        newest_first: bool,
    ) -> Result<PagedTurns, NativeReadError> {
        follow_turn_pages(
            |cursor| self.native_page(owner, host, read(cursor)),
            newest_first,
        )
    }
}

/// Follows a Turn read's pages. A newest-first read keeps the newest Turns
/// within the bounds, or read before a page too large to take, and marks
/// the rest truncated; an oldest-first read past the bounds is too large.
fn follow_turn_pages(
    mut page: impl FnMut(Option<String>) -> Result<NativeRpcReply, NativeReadError>,
    newest_first: bool,
) -> Result<PagedTurns, NativeReadError> {
    let mut turns = Vec::new();
    let mut items = 0usize;
    let mut cursor = None;
    let mut truncated = true;
    for index in 0..MAX_PAGES {
        let page = match page(cursor) {
            Err(NativeReadError::TooLarge) if newest_first && index > 0 => break,
            reply => match reply? {
                NativeRpcReply::TurnsPage(page) => page,
                _ => return Err(NativeReadError::RecoveryRequired),
            },
        };
        if collect_turns(&mut turns, &mut items, page.turns) {
            break;
        }
        cursor = page.next_cursor;
        if cursor.is_none() {
            truncated = false;
            break;
        }
    }
    if truncated && !newest_first {
        return Err(NativeReadError::TooLarge);
    }
    if newest_first {
        turns.reverse();
    }
    Ok(PagedTurns { turns, truncated })
}

/// Appends a page's Turns while they fit; true once one did not.
fn collect_turns(turns: &mut Vec<ChildTurn>, items: &mut usize, page: Vec<ChildTurn>) -> bool {
    for turn in page {
        *items += turn.items.len();
        if turns.len() == MAX_TURNS || *items > MAX_ITEMS {
            return true;
        }
        turns.push(turn);
    }
    false
}

fn page_reply(
    result: Result<NativeRpcReply, HostError>,
) -> Result<NativeRpcReply, NativeReadError> {
    match result {
        Ok(reply @ (NativeRpcReply::TurnsPage(_) | NativeRpcReply::SessionsPage(_))) => Ok(reply),
        Ok(NativeRpcReply::TooLarge) => Err(NativeReadError::TooLarge),
        Ok(NativeRpcReply::Rejected) => Err(NativeReadError::Rejected),
        Ok(
            NativeRpcReply::GoalApplied
            | NativeRpcReply::Forked(_)
            | NativeRpcReply::Released
            | NativeRpcReply::UsageRead
            | NativeRpcReply::Controlled(_)
            | NativeRpcReply::ControlRefused(_)
            | NativeRpcReply::McpServers(_)
            | NativeRpcReply::McpSignIn(_),
        ) => Err(NativeReadError::RecoveryRequired),
        Err(HostError::Intent | HostError::State) => Err(NativeReadError::Unavailable),
        Err(_) => Err(NativeReadError::RecoveryRequired),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::acp::{
        bindings::codex::{ChildItem, NativeTurnsPage},
        lifecycle::ItemLifecycle,
    };

    fn turn(id: usize, items: usize) -> ChildTurn {
        let item = ChildItem {
            id: "item".into(),
            role: None,
            observed_sequence: None,
            lifecycle: ItemLifecycle::Completed,
            tool_state: None,
            payload: crate::acp::intent::NormalizedPayload::Text {
                text: crate::acp::contract::SafeText::new("text", "x").unwrap(),
            },
        };
        ChildTurn {
            id: format!("turn-{id}"),
            status: "completed".into(),
            items: vec![item; items],
        }
    }

    #[test]
    fn pages_collect_turns_until_the_turn_or_item_bound() {
        let (mut turns, mut items) = (Vec::new(), 0);
        assert!(!collect_turns(
            &mut turns,
            &mut items,
            vec![turn(0, 2), turn(1, 2)]
        ));
        assert_eq!((turns.len(), items), (2, 4));
        let full: Vec<_> = (0..MAX_TURNS).map(|id| turn(id, 0)).collect();
        assert!(collect_turns(&mut turns, &mut items, full));
        assert_eq!(turns.len(), MAX_TURNS);

        let (mut turns, mut items) = (Vec::new(), 0);
        assert!(collect_turns(
            &mut turns,
            &mut items,
            vec![turn(0, MAX_ITEMS), turn(1, 1)]
        ));
        assert_eq!(turns.len(), 1);
    }

    #[test]
    fn a_newest_first_read_keeps_its_pages_before_one_too_large() {
        let mut pages = vec![
            Ok(NativeRpcReply::TurnsPage(NativeTurnsPage {
                turns: vec![turn(2, 1), turn(1, 1)],
                next_cursor: Some("older".into()),
            })),
            Err(NativeReadError::TooLarge),
        ]
        .into_iter();
        let paged = follow_turn_pages(|_| pages.next().unwrap(), true).unwrap();
        let ids: Vec<_> = paged.turns.iter().map(|turn| turn.id.as_str()).collect();
        assert_eq!((ids, paged.truncated), (vec!["turn-1", "turn-2"], true));

        let too_large = || Err(NativeReadError::TooLarge);
        for newest_first in [true, false] {
            assert_eq!(
                follow_turn_pages(|_| too_large(), newest_first).unwrap_err(),
                NativeReadError::TooLarge,
                "a first page too large is never a history"
            );
        }
        let mut pages = vec![
            Ok(NativeRpcReply::TurnsPage(NativeTurnsPage {
                turns: vec![turn(1, 1)],
                next_cursor: Some("next".into()),
            })),
            Err(NativeReadError::TooLarge),
        ]
        .into_iter();
        assert_eq!(
            follow_turn_pages(|_| pages.next().unwrap(), false).unwrap_err(),
            NativeReadError::TooLarge
        );
    }

    #[test]
    fn only_a_page_reply_is_a_page() {
        assert_eq!(
            page_reply(Ok(NativeRpcReply::TooLarge)).unwrap_err(),
            NativeReadError::TooLarge
        );
        assert_eq!(
            page_reply(Ok(NativeRpcReply::GoalApplied)).unwrap_err(),
            NativeReadError::RecoveryRequired
        );
        assert_eq!(
            page_reply(Err(HostError::State)).unwrap_err(),
            NativeReadError::Unavailable
        );
        assert_eq!(
            page_reply(Err(HostError::Deadline)).unwrap_err(),
            NativeReadError::RecoveryRequired
        );
    }
}
