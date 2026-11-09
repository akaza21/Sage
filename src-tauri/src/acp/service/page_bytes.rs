//! Sizes a chat page against its byte budget while only the fields around
//! its transcript change.

use crate::acp::{encoded_len::encoded_len, intent::AcpChatPage, store::StoreError};

/// `[]`, the encoding of the empty turn list a page is measured with.
const EMPTY_TURNS: usize = 2;

/// A page's transcript measured once. Fitting the session config and goal
/// to the budget only changes the fields around it, so each check encodes
/// those fields alone rather than the whole page again.
pub(super) struct PageBytes {
    turns: usize,
}

impl PageBytes {
    pub(super) fn measure(page: &AcpChatPage) -> Result<Self, StoreError> {
        let turns = encoded_len(&page.turns).map_err(|_| StoreError::Corrupt("page turns"))?;
        Ok(Self { turns })
    }

    /// The page's encoded length. Its turns must be the ones measured.
    pub(super) fn of(&self, page: &mut AcpChatPage) -> Result<usize, StoreError> {
        let turns = std::mem::take(&mut page.turns);
        let shell = encoded_len(page);
        page.turns = turns;
        let bytes =
            shell.map_err(|_| StoreError::Corrupt("page fields"))? - EMPTY_TURNS + self.turns;
        debug_assert_eq!(
            encoded_len(page).ok(),
            Some(bytes),
            "page turns changed after they were measured"
        );
        Ok(bytes)
    }
}
