use super::super::io;
use super::choice::{Planned, Request};

pub(super) fn submit(
    request: Request,
    completed: impl FnOnce(Option<Planned>) + Send + 'static,
) -> bool {
    let Some(folder) = request.destination() else {
        completed(Some(request.fallback()));
        return true;
    };
    let Some(destination) = io::acquire(folder) else {
        return false;
    };
    destination.run(
        move |destination, deadline| {
            let planned = request.plan_on(destination, deadline);
            if let Err(error) = deadline.check() {
                planned.discard();
                return Err(error);
            }
            Ok(planned)
        },
        move |planned| completed(planned.ok()),
        Planned::discard,
    )
}

#[cfg(test)]
mod tests;
