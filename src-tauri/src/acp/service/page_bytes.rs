use sage_core::encoded_len::encoded_len;

use crate::acp::{intent::AcpChatPage, store::StoreError};

const EMPTY_TURNS: usize = "[]".len();

pub(super) struct PageBytes {
    turns: usize,
}

impl PageBytes {
    pub(super) fn measure(page: &AcpChatPage) -> Result<Self, StoreError> {
        let turns = encoded_len(&page.turns).map_err(|_| StoreError::Corrupt("page turns"))?;
        Ok(Self { turns })
    }

    /// Its turns must be the ones measured.
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
