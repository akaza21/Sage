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
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RewindEffects {
    pub conversation: bool,
    /// Existing last-turn file undo only; arbitrary file rewind is reserved.
    pub files: bool,
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
            },
            targets: if self.targets == other.targets {
                self.targets
            } else {
                RewindTargets::LatestTurn
            },
            fork_at: self.fork_at && other.fork_at,
            on_resume: self.on_resume && other.on_resume,
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
