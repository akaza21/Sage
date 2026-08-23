use std::{
    future::poll_fn,
    sync::{Arc, Mutex, OnceLock},
    task::Poll,
};

use tauri::{AppHandle, Emitter};
use tokio::sync::{Semaphore, oneshot};

use crate::{
    domain::ResourceRef,
    workspace::search::language_fixture::{LanguageFixtureReports, LanguageFixtureRequest},
};

const LANGUAGE_DIAGNOSTIC_EVENT: &str = "language-diagnostic-v1";
const LANGUAGE_OUTLINE_EVENT: &str = "language-outline-v1";
static LANGUAGE_LIFECYCLE: OnceLock<Arc<Semaphore>> = OnceLock::new();
static LANGUAGE_SESSIONS: OnceLock<Mutex<LanguageSessions>> = OnceLock::new();
#[cfg(test)]
static TEST_ACTIVE_PROCESSES: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

struct LanguageSessions {
    active: Option<LanguageSessionEntry>,
    next_generation: u32,
}

struct LanguageSessionEntry {
    resource: ResourceRef,
    generation: u32,
    cancellation: oneshot::Sender<()>,
    completion: oneshot::Receiver<()>,
}

impl Default for LanguageSessions {
    fn default() -> Self {
        Self {
            active: None,
            next_generation: 0,
        }
    }
}

#[tauri::command]
pub async fn start_language_session(
    app: AppHandle,
    resource: ResourceRef,
    document: String,
) -> Result<u32, String> {
    let (generation, cancellation, completion, previous) = register_session(resource.clone())?;
    if let Some(previous) = previous {
        let _ = previous.send(());
    }
    tauri::async_runtime::spawn(async move {
        let result = run_language_fixture(
            LanguageFixtureRequest {
                resource: resource.clone(),
                generation,
                document,
            },
            cancellation,
        )
        .await;
        if let Ok(reports) = result
            && active_session_matches(&resource, generation)
        {
            let _ = app.emit(LANGUAGE_DIAGNOSTIC_EVENT, reports.diagnostics);
            let _ = app.emit(LANGUAGE_OUTLINE_EVENT, reports.outline);
        }
        clear_session(&resource, generation);
        let _ = completion.send(());
    });
    Ok(generation)
}

#[tauri::command]
pub async fn cancel_language_session(resource: ResourceRef) -> Result<(), String> {
    let active = {
        let mut sessions = language_sessions()
            .lock()
            .map_err(|_| "language sessions unavailable")?;
        match sessions.active.as_ref() {
            Some(active) if active.resource == resource => sessions.active.take(),
            _ => None,
        }
    };
    if let Some(active) = active {
        let _ = active.cancellation.send(());
        let _ = active.completion.await;
    }
    Ok(())
}

fn register_session(
    resource: ResourceRef,
) -> Result<
    (
        u32,
        oneshot::Receiver<()>,
        oneshot::Sender<()>,
        Option<oneshot::Sender<()>>,
    ),
    String,
> {
    let mut sessions = language_sessions()
        .lock()
        .map_err(|_| "language sessions unavailable")?;
    let generation = sessions.next_generation.saturating_add(1);
    sessions.next_generation = generation;
    let (sender, receiver) = oneshot::channel();
    let (completion, completed) = oneshot::channel();
    let previous = sessions.active.replace(LanguageSessionEntry {
        resource,
        generation,
        cancellation: sender,
        completion: completed,
    });
    Ok((
        generation,
        receiver,
        completion,
        previous.map(|entry| entry.cancellation),
    ))
}

fn active_session_matches(resource: &ResourceRef, generation: u32) -> bool {
    language_sessions()
        .lock()
        .ok()
        .and_then(|sessions| {
            sessions
                .active
                .as_ref()
                .map(|active| active.resource == *resource && active.generation == generation)
        })
        .unwrap_or(false)
}

fn clear_session(resource: &ResourceRef, generation: u32) {
    if let Ok(mut sessions) = language_sessions().lock()
        && sessions
            .active
            .as_ref()
            .is_some_and(|active| active.resource == *resource && active.generation == generation)
    {
        sessions.active = None;
    }
}

async fn run_language_fixture(
    request: LanguageFixtureRequest,
    cancellation: oneshot::Receiver<()>,
) -> Result<LanguageFixtureReports, String> {
    let lifecycle = language_lifecycle();
    let mut acquire = Box::pin(lifecycle.acquire_owned());
    let mut cancellation = Box::pin(cancellation);
    let permit = poll_fn(|context| {
        if let Poll::Ready(permit) = acquire.as_mut().poll(context) {
            return Poll::Ready(
                permit.map_err(|_| String::from("language lifecycle is unavailable")),
            );
        }
        if cancellation.as_mut().poll(context).is_ready() {
            return Poll::Ready(Err("language session was cancelled".into()));
        }
        Poll::Pending
    })
    .await?;
    let _permit = permit;
    let cancellation = *std::pin::Pin::into_inner(cancellation);
    #[cfg(test)]
    TEST_ACTIVE_PROCESSES.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    #[cfg(test)]
    let result =
        crate::workspace::search::language_fixture::run_test_fixture(request, cancellation).await;
    #[cfg(not(test))]
    let result = crate::workspace::search::language_fixture::run(request, cancellation).await;
    #[cfg(test)]
    TEST_ACTIVE_PROCESSES.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
    result
}

#[cfg(test)]
fn active_language_processes() -> usize {
    TEST_ACTIVE_PROCESSES.load(std::sync::atomic::Ordering::SeqCst)
}

fn language_lifecycle() -> Arc<Semaphore> {
    LANGUAGE_LIFECYCLE
        .get_or_init(|| Arc::new(Semaphore::new(1)))
        .clone()
}

fn language_sessions() -> &'static Mutex<LanguageSessions> {
    LANGUAGE_SESSIONS.get_or_init(|| Mutex::new(LanguageSessions::default()))
}

#[cfg(test)]
mod tests;
