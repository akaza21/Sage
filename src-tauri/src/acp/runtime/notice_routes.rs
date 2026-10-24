use std::collections::VecDeque;

use crate::acp::{
    contract::{ProjectionRevision, SageAgentSessionId, SageChatId, SageProjectId},
    intent::{ChildProjectionChangedV1, ProjectionChangedV1},
};

/// Chat sessions whose last published revision is remembered for child-only
/// routing. An evicted session's next notice is a parent notice, which only
/// costs one page reload.
pub(crate) const MAX_PROJECTION_NOTICE_ROUTES: usize = 16;

type RouteKey = (SageProjectId, SageChatId, SageAgentSessionId);

/// The last parent-published revision per (project, chat, session), as a
/// bounded LRU, oldest first.
#[derive(Debug, Default)]
pub(crate) struct ProjectionNoticeRoutes {
    routes: VecDeque<(RouteKey, ProjectionRevision)>,
}

impl ProjectionNoticeRoutes {
    /// The remembered revision for `key`, now the most recently used route.
    fn touch(&mut self, key: &RouteKey) -> Option<ProjectionRevision> {
        let index = self.routes.iter().position(|(known, _)| known == key)?;
        let route = self.routes.remove(index)?;
        let revision = route.1;
        self.routes.push_back(route);
        Some(revision)
    }

    fn publish(&mut self, key: RouteKey, revision: ProjectionRevision) {
        if let Some(route) = self.routes.back_mut().filter(|(known, _)| *known == key) {
            route.1 = revision;
            return;
        }
        if self.routes.len() == MAX_PROJECTION_NOTICE_ROUTES {
            self.routes.pop_front();
        }
        self.routes.push_back((key, revision));
    }
}

/// Routes a committed projection notice. A notice that moves only the child
/// epoch at the revision last published for the same project, chat and
/// session is a child notice; anything else is a parent notice and becomes
/// that session's last published revision.
pub(crate) fn route_projection_notice(
    routes: &mut ProjectionNoticeRoutes,
    event: ProjectionChangedV1,
) -> Result<ProjectionChangedV1, ChildProjectionChangedV1> {
    let key = (
        event.project_id.clone(),
        event.chat_id.clone(),
        event.session_id.clone(),
    );
    let last = routes.touch(&key);
    if event.child_projection_epoch == 0 || last != Some(event.projection_revision) {
        routes.publish(key, event.projection_revision);
        return Ok(event);
    }
    Err(ChildProjectionChangedV1 {
        project_id: event.project_id,
        chat_id: event.chat_id,
        session_id: event.session_id,
        child_projection_epoch: event.child_projection_epoch,
    })
}
