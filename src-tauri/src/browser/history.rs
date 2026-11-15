pub(super) const MAX_BROWSER_HISTORY: usize = 64;

#[derive(Default)]
pub(super) struct BrowserHistory {
    entries: Vec<String>,
    cursor: Option<usize>,
    pending: Option<(usize, String)>,
    active: Option<(u64, usize, bool)>,
    native_diverged: bool,
    retry_available: bool,
    failed: bool,
}

impl BrowserHistory {
    pub(super) fn recent(&self) -> impl Iterator<Item = &str> {
        self.entries.iter().rev().map(String::as_str)
    }

    pub(super) fn bounds(&self) -> (bool, bool) {
        (
            self.cursor.is_some_and(|cursor| cursor > 0),
            self.cursor
                .is_some_and(|cursor| cursor + 1 < self.entries.len()),
        )
    }

    pub(super) fn start(&mut self, navigation_id: u64, url: &str) {
        self.active = self
            .pending
            .take()
            .map(|(index, target)| (navigation_id, index, target == url));
    }

    pub(super) fn finish(&mut self, navigation_id: u64, url: Option<&str>) -> Option<String> {
        if let Some((id, index, matched_start)) = self.active.take()
            && id == navigation_id
        {
            let target = self.entries[index].clone();
            if matched_start && url == Some(target.as_str()) {
                self.cursor = Some(index);
                self.retry_available = false;
                return None;
            }
            return self.retry_or_fail(index, target);
        }
        if let Some(url) = url {
            self.append(url);
        }
        None
    }

    pub(super) fn same_document(&mut self, url: &str) -> Option<String> {
        if let Some((index, target)) = self.pending.take() {
            if target == url {
                self.cursor = Some(index);
                self.retry_available = false;
                return None;
            }
            return self.retry_or_fail(index, target);
        }
        self.append(url);
        None
    }

    pub(super) fn pending_target_is(&self, url: &str) -> bool {
        self.pending
            .as_ref()
            .is_some_and(|(_, target)| target == url)
    }

    pub(super) fn native_aligned(&self) -> bool {
        !self.native_diverged
    }

    pub(super) fn mark_native_diverged(&mut self) {
        self.native_diverged = true;
        self.retry_available = false;
    }

    fn retry_or_fail(&mut self, index: usize, target: String) -> Option<String> {
        self.native_diverged = true;
        if self.retry_available {
            self.retry_available = false;
            self.pending = Some((index, target.clone()));
            Some(target)
        } else {
            self.pending = None;
            self.failed = true;
            None
        }
    }

    pub(super) fn abandon(&mut self, navigation_id: u64) {
        if self.active.is_some_and(|(id, _, _)| id == navigation_id) {
            self.active = None;
            self.pending = None;
            self.retry_available = false;
        }
    }

    pub(super) fn take_failure(&mut self) -> bool {
        std::mem::take(&mut self.failed)
    }

    pub(super) fn append(&mut self, url: &str) {
        if self
            .cursor
            .and_then(|cursor| self.entries.get(cursor))
            .is_some_and(|current| current == url)
        {
            return;
        }
        if let Some(cursor) = self.cursor {
            self.entries.truncate(cursor + 1);
        }
        self.entries.push(url.to_owned());
        if self.entries.len() > MAX_BROWSER_HISTORY {
            self.entries.remove(0);
        }
        self.cursor = Some(self.entries.len() - 1);
    }

    pub(super) fn traverse(&mut self, direction: i8) -> Result<String, String> {
        if self.pending.is_some() {
            return Err("browser navigation is busy".into());
        }
        let cursor = self.cursor.ok_or("browser history is empty")?;
        let next = cursor
            .checked_add_signed(isize::from(direction))
            .filter(|next| *next < self.entries.len())
            .ok_or("browser history boundary reached")?;
        let target = self.entries[next].clone();
        self.pending = Some((next, target.clone()));
        self.retry_available = true;
        self.failed = false;
        Ok(target)
    }

    pub(super) fn cancel_traversal(&mut self) {
        self.pending = None;
        self.active = None;
        self.retry_available = false;
    }

    #[cfg(test)]
    pub(super) fn len(&self) -> usize {
        self.entries.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recent_history_is_bounded_and_newest_first() {
        let mut history = BrowserHistory::default();
        history.append("https://first.example/");
        history.append("https://second.example/");
        assert_eq!(
            history.recent().collect::<Vec<_>>(),
            vec!["https://second.example/", "https://first.example/"]
        );
    }

    #[test]
    fn divergent_native_back_retries_the_bounded_target_without_committing_wrong_page() {
        let mut history = BrowserHistory::default();
        history.append("https://a.example/");
        history.append("https://b.example/");
        history.append("https://c.example/");
        assert_eq!(history.traverse(-1).unwrap(), "https://b.example/");
        history.start(7, "https://unexpected.example/");
        assert_eq!(
            history.finish(7, Some("https://unexpected.example/")),
            Some("https://b.example/".into())
        );
        assert_eq!(
            history.entries.last().map(String::as_str),
            Some("https://c.example/")
        );
        history.start(8, "https://b.example/");
        assert_eq!(history.finish(8, Some("https://redirect.example/")), None);
        assert_eq!(
            history.entries.last().map(String::as_str),
            Some("https://c.example/")
        );
    }
}
