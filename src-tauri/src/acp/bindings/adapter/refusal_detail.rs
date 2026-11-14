use sage_acp_protocol::contract::SafeText;

const REFUSAL_DETAIL_CHARS: usize = 512;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RefusalDetail {
    message: Option<SafeText<REFUSAL_DETAIL_CHARS>>,
    remediation: Option<&'static str>,
}

impl RefusalDetail {
    pub(crate) fn new(message: Option<&str>, remediation: Option<&'static str>) -> Option<Self> {
        let message = message.and_then(|value| SafeText::new("refusalMessage", value).ok());
        let remediation = remediation.filter(|value| !value.trim().is_empty());
        (message.is_some() || remediation.is_some()).then_some(Self {
            message,
            remediation,
        })
    }

    pub(crate) fn message(&self) -> Option<&str> {
        self.message.as_ref().map(SafeText::as_str)
    }

    pub(crate) fn remediation(&self) -> Option<&'static str> {
        self.remediation
    }
}
