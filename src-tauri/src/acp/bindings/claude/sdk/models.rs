//! The models a Claude Code session can start with.
//!
//! Claude Code reports its model list only once a session runs, so before the
//! first prompt Sage offers the aliases every 2.1 release accepts for
//! `--model`. The running session then reports its own list, which replaces
//! these in the picker.

use crate::domain::ModelOption;

/// `(alias, name, description)`; the first is the default.
const LAUNCH_MODELS: [(&str, &str, &str); 4] = [
    (
        "default",
        "Default",
        "Claude Code's recommended model for your account.",
    ),
    ("opus", "Opus", "The most capable Claude model."),
    ("sonnet", "Sonnet", "Fast and capable for everyday work."),
    ("haiku", "Haiku", "The fastest Claude model."),
];

pub(crate) fn launch_models() -> Vec<ModelOption> {
    LAUNCH_MODELS
        .iter()
        .enumerate()
        .map(|(index, (id, name, description))| ModelOption {
            id: (*id).into(),
            name: (*name).into(),
            description: Some((*description).into()),
            is_default: Some(index == 0),
            context_window: None,
        })
        .collect()
}

/// Whether a session may be launched with `model`.
pub(crate) fn is_launch_model(model: &str) -> bool {
    LAUNCH_MODELS.iter().any(|(id, ..)| *id == model)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_launch_models_lead_with_the_default_and_admit_only_their_aliases() {
        let models = launch_models();
        assert_eq!(models[0].id, "default");
        assert_eq!(
            models
                .iter()
                .filter(|model| model.is_default == Some(true))
                .count(),
            1
        );
        assert!(models.iter().all(|model| is_launch_model(&model.id)));
        assert!(!is_launch_model("claude-anything"));
        assert!(!is_launch_model(""));
    }
}
