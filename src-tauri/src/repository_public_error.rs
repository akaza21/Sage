use crate::error::AppError;

pub(crate) const MAX_PUBLIC_REPOSITORY_ERROR_BYTES: usize = 512;

pub(crate) fn commit(error: &AppError) -> String {
    let message = match error {
        AppError::InvalidRequest(message) | AppError::ResourceLimit(message) => message.as_str(),
        AppError::Git(_) => {
            "Git could not complete the commit. Refresh Source Control, resolve the repository error, and retry."
        }
        AppError::Io(_) => "Git could not access the selected repository.",
        AppError::Database(_) => "Sage could not load the active Project for this Git commit.",
        AppError::Unsupported(_) => "Git commits are unavailable on this platform.",
        AppError::GitEffectUnknown(_) => {
            "The Git commit outcome is uncertain. Refresh Source Control before continuing."
        }
        AppError::GitContainment { .. } => containment(),
        AppError::UnknownHarness(_) | AppError::HarnessUnavailable(_) => {
            "Sage could not complete the Git commit."
        }
    };
    bounded(message)
}

pub(crate) fn projection(error: &AppError) -> String {
    let message = match error {
        AppError::InvalidRequest(_) => {
            "Repository status is unavailable because the selected root changed or is no longer valid."
        }
        AppError::ResourceLimit(_) => {
            "Repository status exceeded its bounded safety limit. Refresh Source Control after reducing the repository change set."
        }
        AppError::Git(_) => {
            "Git could not produce the bounded repository projection. Refresh Source Control after resolving the repository state."
        }
        AppError::Io(_) => "Repository status could not access the selected root.",
        AppError::Database(_) => "Repository status could not load the active Project.",
        AppError::Unsupported(_) => "Repository status is unavailable on this platform.",
        AppError::GitEffectUnknown(_) => {
            "Repository status is unavailable while a Git outcome remains uncertain. Refresh Source Control."
        }
        AppError::GitContainment { .. } => containment(),
        AppError::UnknownHarness(_) | AppError::HarnessUnavailable(_) => {
            "Repository status is unavailable."
        }
    };
    bounded(message)
}

pub(crate) fn projection_task() -> String {
    "Repository status task did not complete. Refresh Source Control.".into()
}

pub(crate) fn containment() -> &'static str {
    "A prior Git process has not finished exiting. Wait for Source Control recovery before retrying."
}

fn bounded(message: &str) -> String {
    if message.len() <= MAX_PUBLIC_REPOSITORY_ERROR_BYTES {
        return message.into();
    }
    let mut boundary = MAX_PUBLIC_REPOSITORY_ERROR_BYTES;
    while !message.is_char_boundary(boundary) {
        boundary -= 1;
    }
    message[..boundary].into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsupported_platform_diagnostics_never_cross_the_public_git_boundary() {
        let native = AppError::Unsupported(
            r"unsupported native operation at C:\private\workspace\.git".into(),
        );

        for message in [commit(&native), projection(&native)] {
            assert!(message.len() <= MAX_PUBLIC_REPOSITORY_ERROR_BYTES);
            assert!(!message.contains("C:\\private"));
            assert!(!message.contains(".git"));
        }
    }
}
