//! A write the restore makes on the stored value's behalf is a compare-and-set
//! against the value the restore read: a pick stored meanwhile always wins.
use crate::acp::session_config::{
    ChoiceRecorder, choices,
    notices::{self, NoticeReason, SessionConfigNotice},
};
use sage_acp_protocol::{
    contract::BoundedVec,
    scheduler::RuntimeOwner,
    session_config::{
        ConfigMutation, ConfigOption, ConfigValue, SessionConfig, reported::current_value,
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
    /// Stores a choice made before `session` existed, so its first runtime
    /// starts with it (see `launch_choices`) or the restore gives it once that
    /// runtime reports its config, before the first prompt. `offered` is the
    /// option the binding listed it in, with every value the binding listed:
    /// until the session reports its own config, the chat shows that option,
    /// so another listed value can still be chosen if the session never gets
    /// that far. A value the session carried over from the chat's ended
    /// session for the same option was picked after any draft still held, so
    /// it stays. Returns whether the session keeps a value.
    pub(crate) fn remember_start_choice(
        &self,
        session: &sage_acp_protocol::contract::SageAgentSessionId,
        choice: &ConfigMutation,
        offered: &ConfigOption,
    ) -> bool {
        let shown = BoundedVec::new("configOptions", vec![offered.clone()])
            .ok()
            .map(|config_options| SessionConfig {
                config_options,
                ..SessionConfig::default()
            });
        self.database
            .write_connection()
            .and_then(|mut connection| {
                let transaction = connection.transaction()?;
                if choices::get(&transaction, session, choice)?.is_some() {
                    return Ok(true);
                }
                let now = sage_core::time::now_millis();
                let written = choices::record(&transaction, session, choice, now)?;
                if written && let Some(shown) = &shown {
                    choices::seed_snapshot(&transaction, session, shown, now)?;
                }
                transaction.commit()?;
                Ok(written)
            })
            .inspect_err(|error| tracing::warn!(?error, "starting config choice was not stored"))
            .unwrap_or(false)
    }

    /// The choices stored for `session` before its runtime opens it; a new
    /// session's runtime learns them before it starts its session.
    pub(in crate::acp::service) fn launch_choices(
        &self,
        session: &sage_acp_protocol::contract::SageAgentSessionId,
    ) -> Vec<ConfigMutation> {
        choices::load(&self.database.connection(), session)
            .inspect_err(|error| tracing::warn!(?error, "stored session config was not read"))
            .unwrap_or_default()
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
