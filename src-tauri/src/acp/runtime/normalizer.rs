use crate::acp::{
    contract::{BoundedVec, ContractError, SafeText},
    intent::{NormalizedPayload, SafeResourceRef, ToolCategory},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum NormalizerInput {
    Text(String),
    ThoughtSummary(String),
    Plan(String),
    ToolCall {
        category: ToolCategory,
        label: String,
        input_summary: Option<String>,
    },
    ToolResult {
        category: ToolCategory,
        label: String,
        output_summary: Option<String>,
    },
    FileChange {
        resource: SafeResourceRef,
        summary: String,
        additions: Option<u64>,
        deletions: Option<u64>,
    },
    Usage {
        input_tokens: u64,
        output_tokens: u64,
    },
    Permission {
        category: ToolCategory,
        summary: String,
    },
    Error {
        code: String,
        message: String,
        retryable: bool,
    },
    Collaboration {
        label: String,
        message: Option<String>,
        receiver_labels: Vec<String>,
    },
    Unknown {
        external_kind: String,
        summary: String,
    },
}

pub(crate) fn normalize(input: NormalizerInput) -> Result<NormalizedPayload, ContractError> {
    Ok(match input {
        NormalizerInput::Text(text) => NormalizedPayload::Text {
            text: SafeText::new("text", text)?,
        },
        NormalizerInput::ThoughtSummary(summary) => NormalizedPayload::ThoughtSummary {
            summary: SafeText::new("thoughtSummary", summary)?,
        },
        NormalizerInput::Plan(summary) => NormalizedPayload::Plan {
            summary: SafeText::new("plan", summary)?,
        },
        NormalizerInput::ToolCall {
            category,
            label,
            input_summary,
        } => NormalizedPayload::ToolCall {
            category,
            label: SafeText::new("toolLabel", label)?,
            input_summary: input_summary
                .map(|value| SafeText::new("toolInput", value))
                .transpose()?,
            output_summary: None,
            locations: Default::default(),
        },
        NormalizerInput::ToolResult {
            category,
            label,
            output_summary,
        } => NormalizedPayload::ToolUpdateResult {
            category,
            label: SafeText::new("toolLabel", label)?,
            output_summary: output_summary
                .map(|value| SafeText::new("toolOutput", value))
                .transpose()?,
            locations: Default::default(),
        },
        NormalizerInput::FileChange {
            resource,
            summary,
            additions,
            deletions,
        } => NormalizedPayload::file_summary_only(
            crate::acp::intent::SafeResourceLocation::Resource { resource },
            SafeText::new("fileSummary", summary)?,
            additions,
            deletions,
            false,
        ),
        NormalizerInput::Usage {
            input_tokens,
            output_tokens,
        } => NormalizedPayload::Usage {
            input_tokens,
            output_tokens,
        },
        NormalizerInput::Permission { category, summary } => NormalizedPayload::PermissionRequest {
            category,
            summary: SafeText::new("permissionSummary", summary)?,
        },
        NormalizerInput::Error {
            code,
            message,
            retryable,
        } => NormalizedPayload::Error {
            code: SafeText::new("errorCode", code)?,
            message: SafeText::new("errorMessage", message)?,
            retryable,
        },
        NormalizerInput::Collaboration {
            label,
            message,
            receiver_labels,
        } => NormalizedPayload::Collaboration {
            label: SafeText::new("activityLabel", label)?,
            message: message
                .map(|value| SafeText::new("activityMessage", value))
                .transpose()?,
            receiver_labels: BoundedVec::new(
                "receivers",
                receiver_labels
                    .into_iter()
                    .map(|value| SafeText::new("receiverLabel", value))
                    .collect::<Result<Vec<_>, _>>()?,
            )?,
        },
        NormalizerInput::Unknown {
            external_kind,
            summary,
        } => NormalizedPayload::Unsupported {
            external_kind: SafeText::new("externalKind", external_kind)?,
            summary: SafeText::new("unsupportedSummary", summary)?,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::acp::contract::SageRootId;

    #[test]
    fn every_closed_variant_normalizes_without_absolute_path_authority() {
        let resource =
            SafeResourceRef::new(SageRootId::parse("root-one").unwrap(), "src/lib.rs").unwrap();
        let cases = vec![
            NormalizerInput::Text("hello".into()),
            NormalizerInput::ThoughtSummary("summary".into()),
            NormalizerInput::Plan("plan".into()),
            NormalizerInput::ToolCall {
                category: ToolCategory::Search,
                label: "search".into(),
                input_summary: Some("query".into()),
            },
            NormalizerInput::ToolResult {
                category: ToolCategory::Search,
                label: "search".into(),
                output_summary: Some("one match".into()),
            },
            NormalizerInput::FileChange {
                resource,
                summary: "changed".into(),
                additions: Some(1),
                deletions: Some(0),
            },
            NormalizerInput::Usage {
                input_tokens: 1,
                output_tokens: 2,
            },
            NormalizerInput::Permission {
                category: ToolCategory::Other,
                summary: "denied".into(),
            },
            NormalizerInput::Error {
                code: "fixture".into(),
                message: "failed".into(),
                retryable: false,
            },
            NormalizerInput::Collaboration {
                label: "child".into(),
                message: Some("done".into()),
                receiver_labels: vec!["reviewer".into()],
            },
            NormalizerInput::Unknown {
                external_kind: "future".into(),
                summary: "unsupported".into(),
            },
        ];
        assert_eq!(
            cases
                .into_iter()
                .filter_map(|case| normalize(case).ok())
                .count(),
            11
        );
    }

    #[test]
    fn summaries_reject_limit_plus_one() {
        assert!(
            normalize(NormalizerInput::Unknown {
                external_kind: "unknown".into(),
                summary: "x".repeat(16_385),
            })
            .is_err()
        );
        assert!(SafeResourceRef::new(SageRootId::parse("root-one").unwrap(), "/absolute").is_err());
    }

    #[test]
    fn collaboration_receivers_accept_exact_and_reject_plus_one() {
        let input = |count| NormalizerInput::Collaboration {
            label: "child".into(),
            message: None,
            receiver_labels: (0..count)
                .map(|index| format!("receiver-{index}"))
                .collect(),
        };
        assert!(normalize(input(32)).is_ok());
        assert!(normalize(input(33)).is_err());
    }
}
