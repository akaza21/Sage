//! Why Sage did not let a bridged Pi edit reach the workspace. The extension
//! raises the message as the tool's error, so both the model and the user see
//! the reason Sage decided on rather than a generic failure.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Refusal {
    /// The user denied the approval card.
    Denied,
    /// Pi gave up on the dialog and Sage cancelled the undecided request.
    Withdrawn,
    /// No Turn is running, or the session can no longer take edits.
    Inactive,
    /// The Turn runs with read-only access.
    ReadOnly,
    /// The Turn already asked for its bounded number of approvals.
    WriteLimit,
    /// Another request in this Turn is still awaiting its decision.
    Busy,
    /// The command has hidden characters or blank lines, so a card could
    /// not show exactly what runs.
    Undisplayable,
    /// The request is not a well-formed write, edit or command of a known tool call.
    Unsupported,
    /// This tool call already had its one approval request.
    Replayed,
    /// A commit named no edit the user approved in this Turn.
    NotApproved,
    OutsideWorkspace,
    /// The path names a protected, reserved or ambiguous file.
    ProtectedPath,
    /// The parent folder is missing or is not a plain folder.
    MissingParent,
    /// The target is a link, not a regular file, or not UTF-8 text.
    Unreadable,
    /// An edit's `oldText` is empty or does not match exactly once.
    NoUniqueMatch,
    TooLarge,
    /// The file changed between approval and the write.
    Changed,
    /// The approved write could not be completed.
    WriteFailed,
}

impl Refusal {
    pub(crate) const fn message(self) -> &'static str {
        match self {
            Self::Denied => "Sage denied the operation",
            Self::Withdrawn => "Sage withdrew the unanswered approval request",
            Self::Inactive => "Sage refused: this session is not taking edits",
            Self::ReadOnly => "Sage refused: this chat has read-only access",
            Self::WriteLimit => "Sage refused: this Turn reached its approval limit",
            Self::Busy => "Sage refused: another request is awaiting approval",
            Self::Undisplayable => "Sage refused: the command cannot be shown as it runs",
            Self::Unsupported => "Sage refused: not a supported edit or command",
            Self::Replayed => "Sage refused: this tool call was already handled",
            Self::NotApproved => "Sage refused: the edit was not approved",
            Self::OutsideWorkspace => "Sage refused: the path is outside the workspace",
            Self::ProtectedPath => "Sage refused: the path is protected or reserved",
            Self::MissingParent => "Sage refused: the parent is not an existing folder",
            Self::Unreadable => "Sage refused: the file is not a regular text file",
            Self::NoUniqueMatch => "Sage refused: oldText must match exactly once",
            Self::TooLarge => "Sage refused: the file exceeds the size limit",
            Self::Changed => "Sage refused: the file changed after approval",
            Self::WriteFailed => "Sage could not write the approved file",
        }
    }
}

/// The longest reply the bridge sends; the extension and the fixture child
/// read at most this many bytes.
#[cfg(any(test, feature = "acp-fixture"))]
pub(crate) const MAX_REPLY_BYTES: usize = 64;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_refusal_fits_one_bridge_reply() {
        use Refusal::*;
        for refusal in [
            Denied,
            Withdrawn,
            Inactive,
            ReadOnly,
            WriteLimit,
            Busy,
            Unsupported,
            Undisplayable,
            Replayed,
            NotApproved,
            OutsideWorkspace,
            ProtectedPath,
            MissingParent,
            Unreadable,
            NoUniqueMatch,
            TooLarge,
            Changed,
            WriteFailed,
        ] {
            let message = refusal.message();
            assert!(message.len() <= MAX_REPLY_BYTES, "{message}");
            assert!(message.starts_with("Sage "), "{message}");
        }
    }
}
