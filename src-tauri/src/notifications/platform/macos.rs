//! macOS notices through `UNUserNotificationCenter`, with Sage's own delegate.
//! The delegate presents notices while Sage is frontmost (the chat may still
//! be out of view) and routes a click to its receipt by the notice's
//! identifier, so no thread waits for an answer. At most `MAX_IN_FLIGHT`
//! delivered notices are kept, like Windows toasts, and the rest are
//! withdrawn. The center is thread-safe, so the notice thread uses it
//! directly; a dispatch slot is held until the center accepts the request.
//!
//! The center works only inside Sage's app bundle: a development binary run
//! outside one shows no notices rather than raising an exception.
use super::super::{Event, MAX_IN_FLIGHT, Permit, clicked};
use block2::{DynBlock, RcBlock};
use objc2::{
    AllocAnyThread, DefinedClass, available, define_class, msg_send,
    rc::Retained,
    runtime::{Bool, NSObject, NSObjectProtocol, ProtocolObject},
};
use objc2_foundation::{NSArray, NSBundle, NSError, NSString};
use objc2_user_notifications::{
    UNAuthorizationOptions, UNMutableNotificationContent, UNNotification,
    UNNotificationDefaultActionIdentifier, UNNotificationPresentationOptions,
    UNNotificationRequest, UNNotificationResponse, UNUserNotificationCenter,
    UNUserNotificationCenterDelegate,
};
use std::{
    collections::VecDeque,
    sync::{Mutex, OnceLock, PoisonError},
};

const IDENTIFIER_PREFIX: &str = "sage-turn-";

/// `None` when the process cannot post notices; see the module docs.
static CENTER: OnceLock<Option<Center>> = OnceLock::new();

/// Installs the delegate and asks once for permission to show alerts; macOS
/// remembers the answer. Called at startup while notices are on and when
/// they are turned on, so the first notice is not lost to the prompt.
pub(in crate::notifications) fn prepare(app: &tauri::AppHandle) {
    center(app);
}

pub(in crate::notifications) fn dispatch(
    app: tauri::AppHandle,
    id: i64,
    event: Event,
    permit: Permit,
) {
    match center(&app) {
        Some(center) => center.deliver(id, event, permit),
        None => tracing::debug!("turn notification skipped: no notification center"),
    }
}

/// Withdraws Sage's delivered notices when the app exits.
pub(in crate::notifications) fn withdraw_all() {
    if let Some(Some(center)) = CENTER.get() {
        center.withdraw_all();
    }
}

fn center(app: &tauri::AppHandle) -> Option<&'static Center> {
    CENTER.get_or_init(|| Center::new(app)).as_ref()
}

struct Center {
    center: Retained<UNUserNotificationCenter>,
    /// The center holds its delegate weakly; this keeps it alive.
    _delegate: Retained<Delegate>,
    delivered: Mutex<VecDeque<Retained<NSString>>>,
}

// SAFETY: Apple documents `UNUserNotificationCenter` as safe to use from any
// thread. The delegate's only state is a thread-safe `AppHandle`, set before
// it is shared, and the identifiers are immutable strings behind a mutex.
unsafe impl Send for Center {}
// SAFETY: as above.
unsafe impl Sync for Center {}

impl Center {
    fn new(app: &tauri::AppHandle) -> Option<Self> {
        if !available!(macos = 10.14) {
            return None;
        }
        // `currentNotificationCenter` raises outside an app bundle.
        let bundle = NSBundle::mainBundle();
        bundle.bundleIdentifier()?;
        if !bundle.bundlePath().to_string().ends_with(".app") {
            return None;
        }
        let center = UNUserNotificationCenter::currentNotificationCenter();
        let delegate = Delegate::new(app.clone());
        center.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
        let answered = RcBlock::new(|granted: Bool, _error: *mut NSError| {
            if !granted.as_bool() {
                tracing::info!("turn notifications are turned off in macOS settings");
            }
        });
        center.requestAuthorizationWithOptions_completionHandler(
            UNAuthorizationOptions::Alert,
            &answered,
        );
        Some(Self {
            center,
            _delegate: delegate,
            delivered: Mutex::new(VecDeque::with_capacity(MAX_IN_FLIGHT)),
        })
    }

    fn deliver(&self, id: i64, event: Event, permit: Permit) {
        let identifier = NSString::from_str(&format!("{IDENTIFIER_PREFIX}{id}"));
        let content = UNMutableNotificationContent::new();
        // All text is fixed product copy; no harness or chat content enters it.
        content.setTitle(&NSString::from_str("Sage"));
        content.setBody(&NSString::from_str(event.body()));
        let request = UNNotificationRequest::requestWithIdentifier_content_trigger(
            &identifier,
            &content,
            None,
        );
        {
            let mut delivered = self
                .delivered
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            if delivered.len() >= MAX_IN_FLIGHT
                && let Some(oldest) = delivered.pop_front()
            {
                self.withdraw(&[&*oldest]);
            }
            delivered.push_back(identifier);
        }
        // The center answers on its own queue; the slot ends with the answer.
        let slot = Mutex::new(Some(permit));
        let added = RcBlock::new(move |error: *mut NSError| {
            drop(slot.lock().unwrap_or_else(PoisonError::into_inner).take());
            if !error.is_null() {
                tracing::warn!("platform turn notification dispatch failed");
            }
        });
        self.center
            .addNotificationRequest_withCompletionHandler(&request, Some(&added));
    }

    /// Stops tracking a notice the user answered and removes it.
    fn forget(&self, identifier: &NSString) {
        self.delivered
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .retain(|kept| !kept.isEqualToString(identifier));
        self.withdraw(&[identifier]);
    }

    fn withdraw_all(&self) {
        let delivered: Vec<_> = self
            .delivered
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .drain(..)
            .collect();
        let identifiers: Vec<&NSString> = delivered.iter().map(|kept| &**kept).collect();
        if !identifiers.is_empty() {
            self.withdraw(&identifiers);
        }
    }

    fn withdraw(&self, identifiers: &[&NSString]) {
        self.center
            .removeDeliveredNotificationsWithIdentifiers(&NSArray::from_slice(identifiers));
    }
}

/// The receipt id a notice was delivered for, from its identifier.
fn receipt(identifier: &NSString) -> Option<i64> {
    identifier
        .to_string()
        .strip_prefix(IDENTIFIER_PREFIX)?
        .parse()
        .ok()
}

/// Banners while Sage is frontmost: the notice's chat was not in view, or it
/// would not have been sent.
fn presentation() -> UNNotificationPresentationOptions {
    if available!(macos = 11.0) {
        UNNotificationPresentationOptions::Banner | UNNotificationPresentationOptions::List
    } else {
        #[allow(deprecated, reason = "Banner and List need macOS 11")]
        UNNotificationPresentationOptions::Alert
    }
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements and `Delegate` does
    // not implement `Drop`.
    #[unsafe(super(NSObject))]
    #[thread_kind = AllocAnyThread]
    #[name = "SageTurnNoticeDelegate"]
    #[ivars = tauri::AppHandle]
    struct Delegate;

    unsafe impl NSObjectProtocol for Delegate {}

    unsafe impl UNUserNotificationCenterDelegate for Delegate {
        #[unsafe(method(userNotificationCenter:willPresentNotification:withCompletionHandler:))]
        fn will_present(
            &self,
            _center: &UNUserNotificationCenter,
            _notice: &UNNotification,
            completion: &DynBlock<dyn Fn(UNNotificationPresentationOptions)>,
        ) {
            completion.call((presentation(),));
        }

        #[unsafe(method(userNotificationCenter:didReceiveNotificationResponse:withCompletionHandler:))]
        fn did_receive(
            &self,
            _center: &UNUserNotificationCenter,
            response: &UNNotificationResponse,
            completion: &DynBlock<dyn Fn()>,
        ) {
            let identifier = response.notification().request().identifier();
            // SAFETY: a constant string exported by UserNotifications.
            let default_action = unsafe { UNNotificationDefaultActionIdentifier };
            if response.actionIdentifier().isEqualToString(default_action)
                && let Some(id) = receipt(&identifier)
            {
                clicked(self.ivars(), id, None);
            }
            if let Some(Some(center)) = CENTER.get() {
                center.forget(&identifier);
            }
            completion.call(());
        }
    }
);

impl Delegate {
    fn new(app: tauri::AppHandle) -> Retained<Self> {
        let this = Self::alloc().set_ivars(app);
        // SAFETY: NSObject's designated initializer.
        unsafe { msg_send![super(this), init] }
    }
}
