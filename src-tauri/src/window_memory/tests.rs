use std::{cell::Cell, rc::Rc};

use super::{MemoryWindow, WindowMemory};
use crate::browser::background::{MemoryTarget, MemoryTargetPort};

struct TestWebview {
    placement: Option<bool>,
    minimized: Rc<Cell<bool>>,
    target: Cell<MemoryTarget>,
    refreshes: Cell<usize>,
}

impl MemoryTargetPort for Rc<TestWebview> {
    fn update_memory_target(
        &self,
        policy: impl FnOnce(Option<bool>, bool) -> MemoryTarget + Send + 'static,
    ) {
        self.target
            .set(policy(self.placement, self.minimized.get()));
        self.refreshes.set(self.refreshes.get() + 1);
    }
}

#[derive(Clone, Default)]
struct TestWindow {
    minimized: Rc<Cell<bool>>,
    webviews: Vec<Rc<TestWebview>>,
}

impl MemoryWindow for TestWindow {
    type Webview = Rc<TestWebview>;

    fn is_minimized(&self) -> Result<bool, String> {
        Ok(self.minimized.get())
    }

    fn webviews(&self) -> Vec<Self::Webview> {
        self.webviews.clone()
    }
}

impl TestWindow {
    fn create(&mut self, memory: &WindowMemory, placement: Option<bool>) -> Rc<TestWebview> {
        let webview = Rc::new(TestWebview {
            placement,
            minimized: self.minimized.clone(),
            target: Cell::new(MemoryTarget::Normal),
            refreshes: Cell::new(0),
        });
        self.webviews.push(webview.clone());
        memory.handle_webview_ready("main", self, &webview);
        webview
    }

    fn resize(&self, memory: &WindowMemory) {
        memory.handle_window_event(
            "main",
            &tauri::WindowEvent::Resized(tauri::PhysicalSize::new(800, 600)),
            || Some(self.clone()),
        );
    }
}

#[test]
fn only_minimize_and_restore_refresh_window_memory_targets() {
    let memory = WindowMemory::default();
    let mut window = TestWindow::default();
    let main = window.create(&memory, None);
    let placed = window.create(&memory, Some(true));
    let hidden = window.create(&memory, Some(false));
    let targets = || [main.target.get(), placed.target.get(), hidden.target.get()];
    let refreshes = || {
        [
            main.refreshes.get(),
            placed.refreshes.get(),
            hidden.refreshes.get(),
        ]
    };

    assert_eq!(
        targets(),
        [
            MemoryTarget::Normal,
            MemoryTarget::Normal,
            MemoryTarget::Low
        ]
    );
    for _ in 0..3 {
        window.resize(&memory);
    }
    assert_eq!(refreshes(), [1, 1, 1], "only creation has refreshed");

    window.minimized.set(true);
    window.resize(&memory);
    assert_eq!(targets(), [MemoryTarget::Low; 3]);
    assert_eq!(refreshes(), [2, 2, 2]);
    for _ in 0..3 {
        window.resize(&memory);
    }
    assert_eq!(refreshes(), [2, 2, 2]);

    window.minimized.set(false);
    window.resize(&memory);
    assert_eq!(
        targets(),
        [
            MemoryTarget::Normal,
            MemoryTarget::Normal,
            MemoryTarget::Low
        ]
    );
    assert_eq!(refreshes(), [3, 3, 3]);
    for _ in 0..3 {
        window.resize(&memory);
    }
    assert_eq!(refreshes(), [3, 3, 3]);
}

#[test]
fn webviews_created_while_minimized_get_low_and_restore_by_placement() {
    let memory = WindowMemory::default();
    let mut window = TestWindow::default();
    let main = window.create(&memory, None);
    window.minimized.set(true);

    // A ready callback can precede the resize that reports minimization.
    let placed = window.create(&memory, Some(true));
    assert_eq!(placed.target.get(), MemoryTarget::Low);
    assert_eq!(main.target.get(), MemoryTarget::Normal);
    assert_eq!(
        main.refreshes.get(),
        1,
        "creation only refreshes the new view"
    );
    window.resize(&memory);
    assert_eq!(main.target.get(), MemoryTarget::Low);
    assert_eq!(main.refreshes.get(), 2);

    // Creation must also refresh after the window transition is cached.
    let hidden = window.create(&memory, Some(false));
    assert_eq!(hidden.target.get(), MemoryTarget::Low);
    window.resize(&memory);
    assert_eq!(placed.refreshes.get(), 2);
    assert_eq!(hidden.refreshes.get(), 1);

    window.minimized.set(false);
    window.resize(&memory);
    assert_eq!(main.target.get(), MemoryTarget::Normal);
    assert_eq!(placed.target.get(), MemoryTarget::Normal);
    assert_eq!(hidden.target.get(), MemoryTarget::Low);
}
