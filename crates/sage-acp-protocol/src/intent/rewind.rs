use serde::{Deserialize, Serialize};

/// A fixed-size offer. Missing offers in durable older negotiations fail closed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RewindCapability {
    pub effects: RewindEffects,
    pub targets: RewindTargets,
    pub fork_at: bool,
    /// Conversation cuts take effect on the next native resume.
    pub on_resume: bool,
    /// Only the last turn completed by this runtime has a native undo checkpoint.
    #[serde(default, skip_serializing_if = "is_false")]
    pub runtime_checkpoint: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RewindEffects {
    pub conversation: bool,
    /// The harness restores files with a rewind. Without `file_preview` this
    /// is the last-turn undo only.
    pub files: bool,
    /// A dry run lists the files a rewind to any admitted target would
    /// restore, so files rewind with the conversation after a preview.
    #[serde(default, skip_serializing_if = "is_false")]
    pub file_preview: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RewindTargets {
    #[default]
    LatestTurn,
    AnyTurn,
    AnyUserMessage,
}

impl RewindCapability {
    pub fn intersect(self, other: Self) -> Self {
        Self {
            effects: RewindEffects {
                conversation: self.effects.conversation
                    && other.effects.conversation
                    && self.on_resume == other.on_resume,
                files: self.effects.files && other.effects.files,
                file_preview: self.effects.file_preview && other.effects.file_preview,
            },
            targets: if self.targets == other.targets {
                self.targets
            } else {
                RewindTargets::LatestTurn
            },
            fork_at: self.fork_at && other.fork_at,
            on_resume: self.on_resume && other.on_resume,
            runtime_checkpoint: self.runtime_checkpoint || other.runtime_checkpoint,
        }
    }
    /// Standard ACP's characterised fork remains a latest-turn operation.
    pub fn latest_fork(fork_at: bool) -> Self {
        Self {
            fork_at,
            ..Self::default()
        }
    }
}

fn is_false(value: &bool) -> bool {
    !value
}
