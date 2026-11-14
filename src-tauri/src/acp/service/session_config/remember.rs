//! A write the restore makes on the stored value's behalf is a compare-and-set
//! against the value the restore read: a pick stored meanwhile always wins.
use crate::acp::{
    scheduler::RuntimeOwner,
    session_config::{
        ChoiceRecorder, ConfigMutation, ConfigValue, SessionConfig, choices,
        notices::{self, NoticeReason, SessionConfigNotice},
        reported::current_value,
    },
};

use super::AcpService;

/// The notice for a saved model the session does not run, with the model it
/// runs instead. `None` for any other option.
fn model_notice(
    config: &SessionConfig,
    saved: &ConfigMutation,
    current: &ConfigMutation,
    reason: NoticeReason,
) -> Option<SessionConfigNotice> {
    let (
        ConfigMutation::Option {
            config_id,
            value: ConfigValue::Select { value: saved },
        },
        ConfigMutation::Option {
            value: ConfigValue::Select { value: current },
            ..
        },
    ) = (saved, current)
    else {
        return None;
    };
    config
        .config_options
        .as_slice()
        .iter()
        .find(|option| &option.id == config_id)
        .filter(|option| option.category.as_ref().map(|c| c.as_str()) == Some("model"))?;
    Some(SessionConfigNotice {
        config_id: config_id.clone(),
        saved: saved.clone(),
        current: current.clone(),
        reason,
    })
}

pub(super) fn remember(
    database: &crate::persistence::Database,
    owner: &RuntimeOwner,
    choice: &ConfigMutation,
) {
    let stored = database.write_connection().and_then(|mut connection| {
        let transaction = connection.transaction()?;
        notices::clear(&transaction, &owner.session_id, choice)?;
        choices::record(
            &transaction,
            &owner.session_id,
            choice,
            sage_core::time::now_millis(),
        )?;
        transaction.commit().map_err(Into::into)
    });
    if let Err(error) = stored {
        tracing::warn!(?error, "session config choice was not stored");
    }
}

impl AcpService {
    pub(crate) fn remember_start_choice(
        &self,
        session: &crate::acp::contract::SageAgentSessionId,
        choice: &ConfigMutation,
    ) -> bool {
        self.database
            .write_connection()
            .and_then(|connection| {
                choices::record(&connection, session, choice, sage_core::time::now_millis())
                    .map_err(Into::into)
            })
            .inspect_err(|error| tracing::warn!(?error, "starting config choice was not stored"))
            .unwrap_or(false)
    }

    pub(in crate::acp::service) fn session_config_recorder(&self) -> ChoiceRecorder {
        let database = self.database.clone();
        std::sync::Arc::new(move |owner: &RuntimeOwner, reported: &[ConfigMutation]| {
            for choice in reported {
                remember(&database, owner, choice);
            }
        })
    }

    pub(super) fn remember_config_choice(&self, owner: &RuntimeOwner, choice: &ConfigMutation) {
        remember(&self.database, owner, choice);
    }

    /// While `expected` is still stored, replaces it with `new` (`None`
    /// removes it) and sets `notice`. Returns whether it did.
    fn replace_stored(
        &self,
        owner: &RuntimeOwner,
        expected: &ConfigMutation,
        new: Option<&ConfigMutation>,
        notice: Option<&SessionConfigNotice>,
    ) -> bool {
        let replaced = self.database.write_connection().and_then(|mut connection| {
            choices::replace_if_stored(
                &mut connection,
                &owner.session_id,
                expected,
                new,
                notice,
                sage_core::time::now_millis(),
            )
            .map_err(Into::into)
        });
        replaced
            .inspect_err(|error| tracing::warn!(?error, "session config choice was not stored"))
            .unwrap_or(false)
    }

    pub(super) fn confirm_restored(&self, owner: &RuntimeOwner, choice: &ConfigMutation) {
        self.replace_stored(owner, choice, Some(choice), None);
    }

    pub(super) fn fall_back(
        &self,
        owner: &RuntimeOwner,
        config: &SessionConfig,
        choice: &ConfigMutation,
        reason: NoticeReason,
    ) {
        let current = current_value(config, choice);
        let notice = current
            .as_ref()
            .and_then(|current| model_notice(config, choice, current, reason));
        if self.replace_stored(owner, choice, current.as_ref(), notice.as_ref()) && notice.is_some()
        {
            tracing::warn!(
                ?reason,
                "the saved model is not in use; the session keeps its own"
            );
        }
    }

    pub(super) fn still_stored(&self, owner: &RuntimeOwner, choice: &ConfigMutation) -> bool {
        choices::get(&self.database.connection(), &owner.session_id, choice)
            .inspect_err(|error| tracing::warn!(?error, "stored session config was not read"))
            .ok()
            .flatten()
            .as_ref()
            == Some(choice)
    }
}
