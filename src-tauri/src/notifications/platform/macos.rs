//! macOS notices through `NSUserNotificationCenter`, with Sage's own delegate.
//! The delegate presents notices while Sage is frontmost (the chat may still
//! be out of view) and routes a click to its receipt by the notice's
//! identifier, so no thread waits for an answer. At most `MAX_IN_FLIGHT`
//! delivered notices are kept, like Windows toasts, and the rest are
//! withdrawn. Every call runs on the main thread, which owns the center.
//!
//! The center is deprecated in favour of `UNUserNotificationCenter`, which
//! needs a signed bundle, an authorization prompt and a delegate installed
//! before launch completes; it remains the follow-up once Sage ships signed.
#![allow(
    deprecated,
    reason = "NSUserNotificationCenter is the macOS API usable without a signed bundle"
)]

use super::super::{Event, MAX_IN_FLIGHT, Permit, clicked};
use objc2::{
    ClassType, DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send,
    rc::Retained,
    runtime::{NSObject, NSObjectProtocol, ProtocolObject},
};
use objc2_foundation::{
    NSString, NSUserNotification, NSUserNotificationCenter, NSUserNotificationCenterDelegate,
};
use std::{
    cell::{OnceCell, RefCell},
    collections::VecDeque,
};

const IDENTIFIER_PREFIX: &str = "sage-turn-";

pub(in crate::notifications) fn dispatch(
    app: tauri::AppHandle,
    id: i64,
    event: Event,
    permit: Permit,
) {
    let target = app.clone();
    let result = app.run_on_main_thread(move || {
        let _permit = permit;
        with_center(&target, |center| center.deliver(id, event));
    });
    if result.is_err() {
        tracing::warn!("turn notification dispatcher unavailable");
    }
}

/// Withdraws Sage's delivered notices when the app exits; called on the main
/// thread from the plugin's exit event.
pub(in crate::notifications) fn withdraw_all() {
    CENTER.with(|cell| {
        if let Some(Some(center)) = cell.get() {
            center.withdraw_all();
        }
    });
}

thread_local! {
    /// Main thread only. `None` when the process has no notification center,
    /// such as an unbundled development binary.
    static CENTER: OnceCell<Option<Center>> = const { OnceCell::new() };
}

fn with_center(app: &tauri::AppHandle, use_center: impl FnOnce(&Center)) {
    CENTER.with(|cell| {
        if let Some(center) = cell.get_or_init(|| Center::new(app)) {
            use_center(center);
        }
    });
}

struct Center {
    center: Retained<NSUserNotificationCenter>,
    /// The center holds its delegate unretained; this keeps it alive.
    _delegate: Retained<Delegate>,
    delivered: RefCell<VecDeque<Retained<NSUserNotification>>>,
}

impl Center {
    fn new(app: &tauri::AppHandle) -> Option<Self> {
        let main_thread = MainThreadMarker::new()?;
        // SAFETY: the class method takes no arguments and returns the shared
        // center, or nil when the process has no bundle to post for.
        let center: Option<Retained<NSUserNotificationCenter>> = unsafe {
            msg_send![
                NSUserNotificationCenter::class(),
                defaultUserNotificationCenter
            ]
        };
        let center = center?;
        let delegate = Delegate::new(main_thread, app.clone());
        // SAFETY: `Center` keeps the delegate alive for as long as the center
        // can call it: the thread-local lives until the process exits.
        unsafe { center.setDelegate(Some(ProtocolObject::from_ref(&*delegate))) };
        Some(Self {
            center,
            _delegate: delegate,
            delivered: RefCell::new(VecDeque::with_capacity(MAX_IN_FLIGHT)),
        })
    }

    fn deliver(&self, id: i64, event: Event) {
        let notice = NSUserNotification::new();
        // All text is fixed product copy; no harness or chat content enters it.
        notice.setTitle(Some(&NSString::from_str("Sage")));
        notice.setInformativeText(Some(&NSString::from_str(event.body())));
        notice.setIdentifier(Some(&NSString::from_str(&format!(
            "{IDENTIFIER_PREFIX}{id}"
        ))));
        let Ok(mut delivered) = self.delivered.try_borrow_mut() else {
            return;
        };
        if delivered.len() == MAX_IN_FLIGHT
            && let Some(oldest) = delivered.pop_front()
        {
            self.center.removeDeliveredNotification(&oldest);
        }
        self.center.deliverNotification(&notice);
        delivered.push_back(notice);
    }

    fn forget(&self, notice: &NSUserNotification) {
        self.center.removeDeliveredNotification(notice);
        if let Ok(mut delivered) = self.delivered.try_borrow_mut() {
            delivered.retain(|kept| kept.identifier() != notice.identifier());
        }
    }

    fn withdraw_all(&self) {
        if let Ok(mut delivered) = self.delivered.try_borrow_mut() {
            for notice in delivered.drain(..) {
                self.center.removeDeliveredNotification(&notice);
            }
        }
    }
}

/// The receipt id a notice was delivered for, from its identifier.
fn receipt(notice: &NSUserNotification) -> Option<i64> {
    notice
        .identifier()?
        .to_string()
        .strip_prefix(IDENTIFIER_PREFIX)?
        .parse()
        .ok()
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements and `Delegate` does
    // not implement `Drop`.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "SageTurnNoticeDelegate"]
    #[ivars = tauri::AppHandle]
    struct Delegate;

    unsafe impl NSObjectProtocol for Delegate {}

    unsafe impl NSUserNotificationCenterDelegate for Delegate {
        /// Present even while Sage is frontmost: the notice's chat was not in
        /// view, or it would not have been sent.
        #[unsafe(method(userNotificationCenter:shouldPresentNotification:))]
        fn should_present(
            &self,
            _center: &NSUserNotificationCenter,
            _notice: &NSUserNotification,
        ) -> bool {
            true
        }

        #[unsafe(method(userNotificationCenter:didActivateNotification:))]
        fn did_activate(&self, _center: &NSUserNotificationCenter, notice: &NSUserNotification) {
            if let Some(id) = receipt(notice) {
                clicked(self.ivars(), id);
            }
            CENTER.with(|cell| {
                if let Some(Some(center)) = cell.get() {
                    center.forget(notice);
                }
            });
        }
    }
);

impl Delegate {
    fn new(main_thread: MainThreadMarker, app: tauri::AppHandle) -> Retained<Self> {
        let this = Self::alloc(main_thread).set_ivars(app);
        // SAFETY: NSObject's designated initializer.
        unsafe { msg_send![super(this), init] }
    }
}
