use super::on_committed_project_transition_epoch;
use crate::app_preview_commands::ServingPreview;

#[test]
fn committed_epoch_stops_only_preview_children_from_older_epochs() {
    let preview = ServingPreview::start(4);
    assert!(preview.is_serving());

    on_committed_project_transition_epoch(4);
    assert!(
        preview.is_serving(),
        "a child admitted for the committed epoch must keep serving"
    );

    on_committed_project_transition_epoch(5);
    assert!(
        !preview.is_serving(),
        "a child from an older epoch must be stopped by the commit"
    );
}
