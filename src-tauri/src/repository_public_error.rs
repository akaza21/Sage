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
        AppError::MixedLineEndings
        | AppError::UnknownHarness(_)
        | AppError::HarnessUnavailable(_) => "Sage could not complete the Git commit.",
    };
    bounded(message)
}

pub(crate) fn create_branch(error: &AppError) -> String {
    let message = match error {
        AppError::InvalidRequest(message) | AppError::ResourceLimit(message) => message.as_str(),
        AppError::Git(_) => {
            "Git could not create that local branch. Refresh Source Control, resolve the repository error, and retry."
        }
        AppError::Io(_) => "Git could not access the selected repository.",
        AppError::Database(_) => {
            "Sage could not load the active Project for this Git create branch."
        }
        AppError::Unsupported(_) => "Git create branch is unavailable on this platform.",
        AppError::GitEffectUnknown(_) => {
            "The Git create branch outcome is uncertain. Refresh Source Control before continuing."
        }
        AppError::GitContainment { .. } => containment(),
        AppError::MixedLineEndings
        | AppError::UnknownHarness(_)
        | AppError::HarnessUnavailable(_) => "Sage could not complete the Git create branch.",
    };
    bounded(message)
}

pub(crate) fn sequence_abort(error: &AppError) -> String {
    let message = match error {
        AppError::InvalidRequest(message) | AppError::ResourceLimit(message) => message.as_str(),
        AppError::Git(_) => {
            "Git could not abort that in-progress merge, rebase, cherry-pick, or revert. Refresh Source Control, resolve the repository error, and retry."
        }
        AppError::Io(_) => "Git could not access the selected repository.",
        AppError::Database(_) => "Sage could not load the active Project for this Git abort.",
        AppError::Unsupported(_) => "Git abort is unavailable on this platform.",
        AppError::GitEffectUnknown(_) => {
            "The Git abort outcome is uncertain. Refresh Source Control before continuing."
        }
        AppError::GitContainment { .. } => containment(),
        AppError::MixedLineEndings
        | AppError::UnknownHarness(_)
        | AppError::HarnessUnavailable(_) => "Sage could not complete the Git abort.",
    };
    bounded(message)
}

pub(crate) fn rename_branch(error: &AppError) -> String {
    let message = match error {
        AppError::InvalidRequest(message) | AppError::ResourceLimit(message) => message.as_str(),
        AppError::Git(_) => {
            "Git could not rename that local branch. Refresh Source Control, resolve the repository error, and retry."
        }
        AppError::Io(_) => "Git could not access the selected repository.",
        AppError::Database(_) => {
            "Sage could not load the active Project for this Git rename branch."
        }
        AppError::Unsupported(_) => "Git rename branch is unavailable on this platform.",
        AppError::GitEffectUnknown(_) => {
            "The Git rename branch outcome is uncertain. Refresh Source Control before continuing."
        }
        AppError::GitContainment { .. } => containment(),
        AppError::MixedLineEndings
        | AppError::UnknownHarness(_)
        | AppError::HarnessUnavailable(_) => "Sage could not complete the Git rename branch.",
    };
    bounded(message)
}

pub(crate) fn delete_branch(error: &AppError) -> String {
    let message = match error {
        AppError::InvalidRequest(message) | AppError::ResourceLimit(message) => message.as_str(),
        AppError::Git(_) => {
            "Git could not delete that local branch. Refresh Source Control, resolve the repository error, and retry."
        }
        AppError::Io(_) => "Git could not access the selected repository.",
        AppError::Database(_) => {
            "Sage could not load the active Project for this Git delete branch."
        }
        AppError::Unsupported(_) => "Git delete branch is unavailable on this platform.",
        AppError::GitEffectUnknown(_) => {
            "The Git delete branch outcome is uncertain. Refresh Source Control before continuing."
        }
        AppError::GitContainment { .. } => containment(),
        AppError::MixedLineEndings
        | AppError::UnknownHarness(_)
        | AppError::HarnessUnavailable(_) => "Sage could not complete the Git delete branch.",
    };
    bounded(message)
}

pub(crate) fn checkout(error: &AppError) -> String {
    let message = match error {
        AppError::InvalidRequest(message) | AppError::ResourceLimit(message) => message.as_str(),
        AppError::Git(_) => {
            "Git could not switch to that local branch. Refresh Source Control, resolve the repository error, and retry."
        }
        AppError::Io(_) => "Git could not access the selected repository.",
        AppError::Database(_) => "Sage could not load the active Project for this Git checkout.",
        AppError::Unsupported(_) => "Git checkout is unavailable on this platform.",
        AppError::GitEffectUnknown(_) => {
            "The Git checkout outcome is uncertain. Refresh Source Control before continuing."
        }
        AppError::GitContainment { .. } => containment(),
        AppError::MixedLineEndings
        | AppError::UnknownHarness(_)
        | AppError::HarnessUnavailable(_) => "Sage could not complete the Git checkout.",
    };
    bounded(message)
}

pub(crate) fn rebase(error: &AppError) -> String {
    let message = match error {
        AppError::InvalidRequest(message) | AppError::ResourceLimit(message) => message.as_str(),
        AppError::Git(_) => {
            "Git could not continue the rebase. Refresh Source Control, resolve remaining conflicts, and retry."
        }
        AppError::Io(_) => "Git could not access the selected repository.",
        AppError::Database(_) => "Sage could not load the active Project for this rebase continue.",
        AppError::Unsupported(_) => "Continue rebase is unavailable on this platform.",
        AppError::GitEffectUnknown(_) => {
            "The Git rebase outcome is uncertain. Refresh Source Control before continuing."
        }
        AppError::GitContainment { .. } => containment(),
        AppError::MixedLineEndings
        | AppError::UnknownHarness(_)
        | AppError::HarnessUnavailable(_) => "Sage could not complete the rebase continue.",
    };
    bounded(message)
}

pub(crate) fn cherry_pick(error: &AppError) -> String {
    let message = match error {
        AppError::InvalidRequest(message) | AppError::ResourceLimit(message) => message.as_str(),
        AppError::Git(_) => {
            "Git could not continue the cherry-pick. Refresh Source Control, resolve remaining conflicts, and retry."
        }
        AppError::Io(_) => "Git could not access the selected repository.",
        AppError::Database(_) => {
            "Sage could not load the active Project for this cherry-pick continue."
        }
        AppError::Unsupported(_) => "Continue cherry-pick is unavailable on this platform.",
        AppError::GitEffectUnknown(_) => {
            "The Git cherry-pick outcome is uncertain. Refresh Source Control before continuing."
        }
        AppError::GitContainment { .. } => containment(),
        AppError::MixedLineEndings
        | AppError::UnknownHarness(_)
        | AppError::HarnessUnavailable(_) => "Sage could not complete the cherry-pick continue.",
    };
    bounded(message)
}

pub(crate) fn remote_transport(error: &AppError) -> String {
    let message = match error {
        AppError::InvalidRequest(message) | AppError::ResourceLimit(message) => message.as_str(),
        AppError::Git(_) => {
            "Git could not complete fetch or push for that remote. Refresh Source Control, resolve the repository error, and retry."
        }
        AppError::Io(_) => "Git could not access the selected repository.",
        AppError::Database(_) => {
            "Sage could not load the active Project for this Git fetch or push."
        }
        AppError::Unsupported(_) => "Git fetch and push are unavailable on this platform.",
        AppError::GitEffectUnknown(_) => {
            "The Git fetch or push outcome is uncertain. Refresh Source Control before continuing."
        }
        AppError::GitContainment { .. } => containment(),
        AppError::MixedLineEndings
        | AppError::UnknownHarness(_)
        | AppError::HarnessUnavailable(_) => "Sage could not complete the Git fetch or push.",
    };
    bounded(message)
}

pub(crate) fn pull(error: &AppError) -> String {
    let message = match error {
        AppError::InvalidRequest(message) | AppError::ResourceLimit(message) => message.as_str(),
        AppError::Git(_) => {
            "Git could not fast-forward the incoming history. Refresh Source Control, resolve authentication, network, or conflict errors, and retry."
        }
        AppError::Io(_) => "Git could not access the selected repository.",
        AppError::Database(_) => "Sage could not load the active Project for this Git pull.",
        AppError::Unsupported(_) => "Git pull is unavailable on this platform.",
        AppError::GitEffectUnknown(_) => {
            "The Git pull outcome is uncertain. Refresh Source Control before continuing."
        }
        AppError::GitContainment { .. } => containment(),
        AppError::MixedLineEndings
        | AppError::UnknownHarness(_)
        | AppError::HarnessUnavailable(_) => "Sage could not complete the Git pull.",
    };
    bounded(message)
}

pub(crate) fn revert(error: &AppError) -> String {
    let message = match error {
        AppError::InvalidRequest(message) | AppError::ResourceLimit(message) => message.as_str(),
        AppError::Git(_) => {
            "Git could not continue the revert. Refresh Source Control, resolve remaining conflicts, and retry."
        }
        AppError::Io(_) => "Git could not access the selected repository.",
        AppError::Database(_) => "Sage could not load the active Project for this revert continue.",
        AppError::Unsupported(_) => "Continue revert is unavailable on this platform.",
        AppError::GitEffectUnknown(_) => {
            "The Git revert outcome is uncertain. Refresh Source Control before continuing."
        }
        AppError::GitContainment { .. } => containment(),
        AppError::MixedLineEndings
        | AppError::UnknownHarness(_)
        | AppError::HarnessUnavailable(_) => "Sage could not complete the revert continue.",
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
        AppError::MixedLineEndings
        | AppError::UnknownHarness(_)
        | AppError::HarnessUnavailable(_) => "Repository status is unavailable.",
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

        for message in [
            commit(&native),
            checkout(&native),
            create_branch(&native),
            delete_branch(&native),
            rename_branch(&native),
            sequence_abort(&native),
            rebase(&native),
            cherry_pick(&native),
            revert(&native),
            remote_transport(&native),
            pull(&native),
            projection(&native),
        ] {
            assert!(message.len() <= MAX_PUBLIC_REPOSITORY_ERROR_BYTES);
            assert!(!message.contains("C:\\private"));
            assert!(!message.contains(".git"));
        }
    }

    #[test]
    fn git_remote_transport_errors_never_include_urls() {
        let leaked = AppError::Git(
            "fatal: could not read from remote https://secret.example/repo.git".into(),
        );
        let message = remote_transport(&leaked);
        assert!(!message.contains("secret.example"));
        assert!(!message.contains("http"));
        assert!(!message.contains(".git"));
    }

    #[test]
    fn git_pull_errors_never_include_urls() {
        let leaked = AppError::Git(
            "fatal: could not read from remote https://secret.example/repo.git".into(),
        );
        let message = pull(&leaked);
        assert!(!message.contains("secret.example"));
        assert!(!message.contains("http"));
        assert!(!message.contains(".git"));
    }
}
