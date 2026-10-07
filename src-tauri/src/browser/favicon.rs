pub(super) const MAX_FAVICON_BYTES: usize = 32 * 1024;

use super::BrowserTabs;

impl<T> BrowserTabs<T> {
    pub(super) fn set_favicon_png(
        &mut self,
        label: &str,
        url: &str,
        navigation_id: u64,
        bytes: Option<&[u8]>,
    ) -> bool {
        let favicon = match bytes {
            Some(bytes) => match admitted_data_url(bytes) {
                Some(data_url) => Some(data_url),
                None => return false,
            },
            None => None,
        };
        let Some(tab) = self.entries.get_mut(label) else {
            return false;
        };
        if tab.loading
            || tab.last_error.is_some()
            || tab.committed_url.as_deref() != Some(url)
            || tab.navigation_id != Some(navigation_id)
            || tab.favicon == favicon
        {
            return false;
        }
        tab.favicon = favicon;
        tab.revision = tab.revision.wrapping_add(1);
        true
    }
}

pub(super) fn admitted_data_url(bytes: &[u8]) -> Option<String> {
    if bytes.len() < 45 || bytes.len() > MAX_FAVICON_BYTES {
        return None;
    }
    if &bytes[..8] != b"\x89PNG\r\n\x1a\n"
        || &bytes[8..16] != b"\0\0\0\rIHDR"
        || &bytes[bytes.len() - 12..] != b"\0\0\0\0IEND\xaeB`\x82"
    {
        return None;
    }
    let width = u32::from_be_bytes(bytes[16..20].try_into().ok()?);
    let height = u32::from_be_bytes(bytes[20..24].try_into().ok()?);
    if width == 0 || height == 0 || width > 1024 || height > 1024 {
        return None;
    }
    Some(format!("data:image/png;base64,{}", encode_base64(bytes)))
}

fn encode_base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut output = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let value = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        output.push(ALPHABET[((value >> 18) & 63) as usize] as char);
        output.push(ALPHABET[((value >> 12) & 63) as usize] as char);
        output.push(if chunk.len() > 1 {
            ALPHABET[((value >> 6) & 63) as usize] as char
        } else {
            '='
        });
        output.push(if chunk.len() > 2 {
            ALPHABET[(value & 63) as usize] as char
        } else {
            '='
        });
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_png() -> Vec<u8> {
        let mut bytes = vec![137, 80, 78, 71, 13, 10, 26, 10];
        bytes.extend_from_slice(&[0, 0, 0, 13, 73, 72, 68, 82]);
        bytes.extend_from_slice(&[0, 0, 0, 1, 0, 0, 0, 1, 8, 6, 0, 0, 0]);
        bytes.extend_from_slice(&[0, 0, 0, 0]);
        bytes.extend_from_slice(&[0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130]);
        bytes
    }

    #[test]
    fn png_favicon_is_bounded_and_encoded_as_an_image_data_url() {
        let png = sample_png();
        assert!(
            admitted_data_url(&png)
                .unwrap()
                .starts_with("data:image/png;base64,")
        );
        assert!(admitted_data_url(&vec![0; MAX_FAVICON_BYTES + 1]).is_none());
        assert!(admitted_data_url(b"<svg onload=alert(1)>").is_none());
    }

    #[test]
    fn base64_encodes_complete_and_partial_chunks() {
        assert_eq!(encode_base64(b"Man"), "TWFu");
        assert_eq!(encode_base64(b"Ma"), "TWE=");
        assert_eq!(encode_base64(b"M"), "TQ==");
    }

    #[test]
    fn favicon_projection_accepts_only_the_current_committed_navigation() {
        let mut tabs = BrowserTabs::<usize>::new();
        tabs.insert("browser-1".into(), 1, false);
        assert!(tabs.start_navigation("browser-1", 1, "https://example.com/"));
        assert!(tabs.finish_navigation("browser-1", 1, Some("https://example.com/")));
        let before = tabs.snapshot("browser-1").unwrap().revision;
        let png = sample_png();
        assert!(tabs.set_favicon_png("browser-1", "https://example.com/", 1, Some(&png)));
        let snapshot = tabs.snapshot("browser-1").unwrap();
        assert!(
            snapshot
                .favicon
                .as_deref()
                .unwrap()
                .starts_with("data:image/png;base64,")
        );
        assert!(snapshot.revision > before);
        assert!(!tabs.set_favicon_png("browser-1", "https://old.example/", 1, Some(&png)));
        assert!(tabs.start_navigation("browser-1", 2, "https://next.example/"));
        assert_eq!(tabs.snapshot("browser-1").unwrap().favicon, None);
        assert!(!tabs.set_favicon_png("browser-1", "https://example.com/", 1, Some(&png)));
    }
}
