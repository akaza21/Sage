//! Admit WebView2's borrowed UTF-16 before allocating an owned response. CDP
//! may already have allocated an oversized inline string; Sage never copies it.
use std::{cell::RefCell, char::decode_utf16};
use webview2_com::Microsoft::Web::WebView2::Win32::{
    ICoreWebView2CallDevToolsProtocolMethodCompletedHandler,
    ICoreWebView2CallDevToolsProtocolMethodCompletedHandler_Impl,
};
use windows_core::{HRESULT, PCWSTR, implement};

use crate::acp::browser::BrowserFailure;

type Completed = Box<dyn FnOnce(Result<String, BrowserFailure>) -> windows_core::Result<()>>;

#[implement(ICoreWebView2CallDevToolsProtocolMethodCompletedHandler)]
struct ResponseHandler {
    limit: usize,
    completed: RefCell<Option<Completed>>,
}

pub(super) fn handler(
    limit: usize,
    completed: Completed,
) -> ICoreWebView2CallDevToolsProtocolMethodCompletedHandler {
    ResponseHandler {
        limit,
        completed: RefCell::new(Some(completed)),
    }
    .into()
}

#[allow(non_snake_case)]
impl ICoreWebView2CallDevToolsProtocolMethodCompletedHandler_Impl for ResponseHandler_Impl {
    fn Invoke(&self, outcome: HRESULT, json: &PCWSTR) -> windows_core::Result<()> {
        let completed = self.completed.borrow_mut().take();
        if let Some(completed) = completed {
            // WebView2 owns this NUL-terminated buffer for the duration of Invoke.
            // Neither the pointer nor a slice escapes this synchronous decode.
            completed(unsafe { decode(*json, self.limit, outcome.is_ok()) })?;
        }
        Ok(())
    }
}

unsafe fn decode(json: PCWSTR, limit: usize, succeeded: bool) -> Result<String, BrowserFailure> {
    if json.is_null() {
        return Err(BrowserFailure::Unavailable);
    }
    // Every UTF-16 code unit contributes at least one UTF-8 byte. Stop after
    // limit + 1 units, without even walking the rest of an oversized response.
    let mut units = 0;
    while units <= limit && unsafe { *json.0.add(units) } != 0 {
        units += 1;
    }
    if units > limit {
        return Err(BrowserFailure::Capacity);
    }
    // SAFETY: Invoke supplies a live NUL-terminated buffer; the bounded scan
    // above established this exact initialized slice before its terminator.
    let borrowed = unsafe { std::slice::from_raw_parts(json.0, units) };
    let chars =
        || decode_utf16(borrowed.iter().copied()).map(|c| c.unwrap_or(char::REPLACEMENT_CHARACTER));
    let mut bytes = 0usize;
    for c in chars() {
        bytes = bytes.saturating_add(c.len_utf8());
        if bytes > limit {
            return Err(BrowserFailure::Capacity);
        }
    }
    if !succeeded {
        return Err(BrowserFailure::Unavailable);
    }
    let mut result = String::with_capacity(bytes);
    result.extend(chars());
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{rc::Rc, sync::Arc};

    fn invoke(data: &[u16], limit: usize, outcome: HRESULT) -> Result<String, BrowserFailure> {
        assert_eq!(
            data.last(),
            Some(&0),
            "the COM input must be NUL-terminated"
        );
        let received = Rc::new(RefCell::new(None));
        let output = received.clone();
        let callback = handler(
            limit,
            Box::new(move |response| {
                *output.borrow_mut() = Some(response);
                Ok(())
            }),
        );
        unsafe { callback.Invoke(outcome, PCWSTR(data.as_ptr())) }.unwrap();
        let response = received.borrow_mut().take().unwrap();
        // Duplicate invocation consumes nothing and does not dereference data.
        unsafe { callback.Invoke(outcome, PCWSTR::null()) }.unwrap();
        assert!(received.borrow().is_none());
        response
    }

    #[test]
    fn oversized_inline_native_result_is_rejected_without_retaining_its_buffer() {
        let limit = sage_core::limits::BROWSER_LIMITS.cdp_result_bytes;
        let mut data = vec![b'x' as u16; limit + 1];
        data.push(0);
        let data = Arc::new(data);
        let weak = Arc::downgrade(&data);
        for outcome in [HRESULT(0), HRESULT(-1)] {
            assert_eq!(invoke(&data, limit, outcome), Err(BrowserFailure::Capacity));
        }
        drop(data);
        assert!(weak.upgrade().is_none());
        assert_eq!(
            invoke(
                &super::super::wide(r#"{"result":{"value":42}}"#),
                limit,
                HRESULT(0)
            ),
            Ok(r#"{"result":{"value":42}}"#.into())
        );
    }

    #[test]
    fn native_admission_counts_utf8_before_decoding_and_preserves_accepted_text() {
        let data = super::super::wide("a\u{1f600}\u{00e9}");
        assert_eq!(
            invoke(&data, 7, HRESULT(0)),
            Ok("a\u{1f600}\u{00e9}".into())
        );
        assert_eq!(invoke(&data, 6, HRESULT(0)), Err(BrowserFailure::Capacity));
        assert_eq!(
            invoke(&[0xd800, 0], 2, HRESULT(0)),
            Err(BrowserFailure::Capacity)
        );
        assert_eq!(invoke(&[0xd800, 0], 3, HRESULT(0)), Ok("\u{fffd}".into()));
        assert_eq!(
            invoke(&[0], 8, HRESULT(-1)),
            Err(BrowserFailure::Unavailable)
        );
    }
}
