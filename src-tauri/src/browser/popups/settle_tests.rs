use super::*;

#[derive(Default)]
struct Recorder {
    calls: Vec<&'static str>,
    fail: Vec<&'static str>,
}

impl Recorder {
    fn failing(fail: &[&'static str]) -> Self {
        Self {
            calls: Vec::new(),
            fail: fail.to_vec(),
        }
    }
    fn call(&mut self, name: &'static str) -> Result<(), String> {
        self.calls.push(name);
        if self.fail.contains(&name) {
            Err(format!("{name} E_FAIL"))
        } else {
            Ok(())
        }
    }
}

impl NewWindowRequest for Recorder {
    fn set_new_window(&mut self) -> Result<(), String> {
        self.call("SetNewWindow")
    }
    fn set_handled(&mut self) -> Result<(), String> {
        self.call("SetHandled")
    }
    fn complete(&mut self) -> Result<(), String> {
        self.call("Complete")
    }
}

#[test]
fn attaches_with_set_new_window_last_and_completes_once() {
    let mut request = Recorder::default();
    assert_eq!(settle(&mut request, Ok(())), Settlement::Attached);
    assert_eq!(request.calls, ["SetNewWindow", "SetHandled", "Complete"]);
}

#[test]
fn preparation_failure_denies_without_set_new_window_and_falls_back() {
    let mut request = Recorder::default();
    assert_eq!(
        settle(&mut request, Err("hooks failed".into())),
        Settlement::FallBack("hooks failed".into())
    );
    assert_eq!(request.calls, ["SetHandled", "Complete"]);
}

#[test]
fn set_new_window_failure_denies_and_falls_back() {
    let mut request = Recorder::failing(&["SetNewWindow", "SetHandled"]);
    assert_eq!(
        settle(&mut request, Ok(())),
        Settlement::FallBack("SetNewWindow E_FAIL; SetHandled failed: SetHandled E_FAIL".into())
    );
    assert_eq!(request.calls, ["SetNewWindow", "SetHandled", "Complete"]);
}

#[test]
fn failures_after_set_new_window_abandon_without_a_duplicate_intent() {
    for (fail, reason) in [
        ("SetHandled", "SetHandled failed: SetHandled E_FAIL"),
        ("Complete", "Complete failed: Complete E_FAIL"),
    ] {
        let mut request = Recorder::failing(&[fail]);
        assert_eq!(
            settle(&mut request, Ok(())),
            Settlement::Abandoned(reason.into())
        );
        assert_eq!(request.calls, ["SetNewWindow", "SetHandled", "Complete"]);
    }
}
