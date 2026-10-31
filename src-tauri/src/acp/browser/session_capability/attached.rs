use super::*;
use crate::acp::service::AcpService;

/// State access differs between Wry and the headless fixture; authorization
/// always runs against the same AcpService and document-revision rules.
pub(crate) trait BrowserSessionContext: Clone + Send + Sync + 'static {
    fn service(&self) -> &AcpService;
    fn settled_tab(&self, label: &str) -> Result<(String, u64), String>;
}

impl BrowserSessionContext for AppHandle {
    fn service(&self) -> &AcpService {
        &self.state::<AppState>().inner().acp
    }

    fn settled_tab(&self, label: &str) -> Result<(String, u64), String> {
        self.state::<BrowserState>().settled_tab(label)
    }
}

/// Only native side effects vary here. The returned port must issue under the
/// gate's live permit, just as NativeDevtools does in its UI callback.
pub(crate) trait AttachedTabFactory<C> {
    type Port<'a>: BrowserCapabilityDispatch
    where
        Self: 'a,
        C: 'a;

    fn port<'a>(&'a mut self, context: &'a C, lease: AttachmentLease) -> Self::Port<'a>;
}

pub(crate) struct AttachedSessionCapability<'a, C, F> {
    context: C,
    project: SageProjectId,
    chat: SageChatId,
    session: SageAgentSessionId,
    native: &'a mut F,
}

impl<'a, C, F> AttachedSessionCapability<'a, C, F> {
    pub(crate) fn new(
        context: C,
        project: SageProjectId,
        chat: SageChatId,
        session: SageAgentSessionId,
        native: &'a mut F,
    ) -> Self {
        Self {
            context,
            project,
            chat,
            session,
            native,
        }
    }
}

impl<C: BrowserSessionContext, F: AttachedTabFactory<C>> AttachedSessionCapability<'_, C, F> {
    fn perform_once(&mut self, request: &BrowserRequest) -> Result<BrowserResult, BrowserFailure> {
        if request.scope.session_id != self.session.as_str()
            || matches!(
                request.command,
                crate::acp::browser::BrowserCommand::OpenTab { .. }
            )
        {
            return Err(BrowserFailure::Denied);
        }
        let service = self.context.service();
        let lease = service
            .browser_attachment(&self.project, &self.chat)
            .filter(|lease| lease.tab_number == request.scope.tab_id)
            .or_else(|| {
                service.agent_browser_attachment(
                    &self.project,
                    &self.chat,
                    self.session.as_str(),
                    request.scope.tab_id,
                )
            })
            .ok_or(BrowserFailure::NoAttachment)?;
        if request.scope.tab_id != lease.tab_number {
            return Err(BrowserFailure::Denied);
        }
        let (url, revision) = self
            .context
            .settled_tab(&lease.tab_label)
            .map_err(|_| BrowserFailure::Denied)?;
        if url != lease.url || revision != lease.tab_revision {
            return Err(BrowserFailure::Denied);
        }
        let owner = service
            .current_browser_owner(&self.project, &self.chat, &self.session)
            .ok_or(BrowserFailure::Denied)?;
        let authority = lease
            .authority_for(&request.scope)
            .ok_or(if lease.user_controls() {
                BrowserFailure::UserControl
            } else {
                BrowserFailure::Denied
            })?;
        let native = self.native.port(&self.context, lease.clone());
        let label = lease.tab_label.clone();
        let decider = ServiceBrowserPermissionDecider {
            service,
            owner: owner.clone(),
            authority: authority.clone(),
            attachment: Some(lease),
        };
        let tab_context = self.context.clone();
        let _action = matches!(
            request.command,
            crate::acp::browser::BrowserCommand::Act { .. }
                | crate::acp::browser::BrowserCommand::Navigate { .. }
        )
        .then(|| service.begin_browser_agent_action(&label));
        with_session_owner_guards(
            BrowserPermissionGate::new(native, decider, authority).with_tab_revision_guard(
                move || {
                    tab_context
                        .settled_tab(&label)
                        .is_ok_and(|(_, current)| current == revision)
                },
            ),
            self.context.clone(),
            owner,
        )
        .perform(request)
    }
}

impl<C: BrowserSessionContext, F: AttachedTabFactory<C>> BrowserCapability
    for AttachedSessionCapability<'_, C, F>
{
    fn perform(&mut self, request: &BrowserRequest) -> Result<BrowserResult, BrowserFailure> {
        let context = self.context.clone();
        let (project, chat, session) = (
            self.project.clone(),
            self.chat.clone(),
            self.session.clone(),
        );
        with_pause(&context, &project, &chat, &session, request, || {
            self.perform_once(request)
        })
    }
}

pub(super) fn with_pause<C: BrowserSessionContext>(
    context: &C,
    project: &SageProjectId,
    chat: &SageChatId,
    session: &SageAgentSessionId,
    request: &BrowserRequest,
    attempt: impl FnMut() -> Result<BrowserResult, BrowserFailure>,
) -> Result<BrowserResult, BrowserFailure> {
    let service = context.service();
    let owner = service.current_browser_owner(project, chat, session);
    let open_tab = matches!(
        request.command,
        crate::acp::browser::BrowserCommand::OpenTab { .. }
    );
    let label = format!("browser-{}", request.scope.tab_id);
    crate::acp::browser::pause::run_with_pause(
        attempt,
        |outcome| {
            let declined_by_take_over = !open_tab
                && service
                    .browser_agent_control(&label)
                    .is_some_and(|control| {
                        control.user_control
                            && &control.project_id == project
                            && &control.chat_id == chat
                    });
            crate::acp::browser::pause::paused(outcome, declined_by_take_over)
        },
        crate::acp::browser::pause::PauseSignals {
            user_holds: || service.browser_chat_under_user_control(project, chat),
            hand_backs: || service.browser_hand_back_generation(project, chat),
            owner_current: || {
                owner
                    .as_ref()
                    .is_some_and(|owner| service.browser_owner_precheck_matches(owner))
            },
        },
        crate::acp::browser::pause::HAND_BACK_WAIT,
        crate::acp::browser::pause::POLL,
    )
}
