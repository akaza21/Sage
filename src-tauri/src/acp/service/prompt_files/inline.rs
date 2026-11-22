use super::Resources;
use sha2::{Digest, Sha256};

impl Resources {
    /// A text file is quoted whole inside a `<context>` block that names it; any
    /// other file is named but not sent.
    pub(crate) fn inline(&self, text: &str) -> String {
        let mut inline = String::with_capacity(text.len());
        self.write_inline(text, |part| inline.push_str(part));
        inline
    }

    /// Hash exactly the native inline text without retaining another file copy.
    pub(crate) fn inline_digest(&self, text: &str) -> [u8; 32] {
        let mut digest = Sha256::new();
        self.write_inline(text, |part| digest.update(part.as_bytes()));
        digest.finalize().into()
    }

    fn write_inline(&self, text: &str, mut write: impl FnMut(&str)) {
        write(text);
        for r in self.0.iter() {
            let name = r.name.replace(['"', '<', '>', '\n', '\r'], "_");
            write("\n\n<context name=\"");
            write(&name);
            write("\">\n");
            match std::str::from_utf8(&r.bytes) {
                Ok(body) if !body.contains('\0') => write(body),
                _ => write("(Binary file not shown: only text files can be sent to this harness.)"),
            }
            write("\n</context>");
        }
    }
}
