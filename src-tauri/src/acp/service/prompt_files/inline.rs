use super::Resources;

impl Resources {
    /// A text file is quoted whole inside a `<context>` block that names it; any
    /// other file is named but not sent.
    pub(crate) fn inline(&self, text: &str) -> String {
        let mut inline = text.to_owned();
        for r in self.0.iter() {
            let name = r.name.replace(['"', '<', '>', '\n', '\r'], "_");
            match std::str::from_utf8(&r.bytes) {
                Ok(body) if !body.contains('\0') => {
                    inline.push_str(&format!(
                        "\n\n<context name=\"{name}\">\n{body}\n</context>"
                    ));
                }
                _ => inline.push_str(&format!(
                    "\n\n<context name=\"{name}\">\n(Binary file not shown: only text files can be sent to this harness.)\n</context>"
                )),
            }
        }
        inline
    }
}
