/// Carries positive pre-invocation proof without inferring it from an error kind.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct PromptSendError<E> {
    pub error: E,
    pub prompt_dispatch: super::PromptDispatch,
}

impl<E> PromptSendError<E> {
    pub(crate) fn not_started(error: E) -> Self {
        Self {
            error,
            prompt_dispatch: super::PromptDispatch::NotStarted,
        }
    }

    pub(crate) fn unknown(error: E) -> Self {
        Self {
            error,
            prompt_dispatch: super::PromptDispatch::Unknown,
        }
    }

    pub(crate) fn map<F>(self, map: impl FnOnce(E) -> F) -> PromptSendError<F> {
        PromptSendError {
            error: map(self.error),
            prompt_dispatch: self.prompt_dispatch,
        }
    }
}
