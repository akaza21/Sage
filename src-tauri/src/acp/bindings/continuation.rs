#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ContinuationCompatibility {
    Compatible,
    Fresh,
    EarlierVersion,
}

pub(crate) fn classify(
    previous: (&str, &str, Option<&str>),
    target: (&str, &str, &str),
) -> ContinuationCompatibility {
    if previous.0 == "codex-app-server"
        && target.0 == "codex-app-server"
        && previous.2 == Some("0.145.0")
        && target.2 == "0.146.0"
    {
        return ContinuationCompatibility::EarlierVersion;
    }
    if previous.0 == target.0 && previous.1 == target.1 && previous.2 == Some(target.2) {
        ContinuationCompatibility::Compatible
    } else {
        ContinuationCompatibility::Fresh
    }
}

pub(crate) fn model_selection(
    previous: Option<&str>,
    target: Option<&str>,
) -> crate::acp::service::session_transition::SelectionChange {
    use crate::acp::service::session_transition::SelectionChange;
    SelectionChange {
        unchanged: previous == target,
        fresh: previous != target,
        classified: true,
        ..Default::default()
    }
}
