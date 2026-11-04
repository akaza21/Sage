use super::*;

impl Patch {
    pub(in crate::acp::bindings::codex::incoming::items::file_change) fn saved_sides(
        &self,
        context: Option<&str>,
    ) -> Result<(String, String), PatchError> {
        if self.hunks.len() <= 1 {
            // One replacement excerpt is complete evidence without a file
            // read. Disjoint hunks must never be joined: moves could cancel.
            return Ok((self.old_text.clone(), self.new_text.clone()));
        }
        let context = context.ok_or(Invalid)?;
        if context.len() > DIFF_SIDE_BYTES_MAX {
            return Err(Oversized);
        }
        // Real events follow a write. Fixtures may report the patch while
        // retaining the old file. Neither direction accepts fuzzy matches.
        let reverse = self.apply(context, true);
        let reverse_oversized = matches!(reverse, Err(Oversized));
        if let Ok(old_text) = reverse {
            return Ok((old_text, context.to_owned()));
        }
        match self.apply(context, false) {
            Ok(new_text) => Ok((context.to_owned(), new_text)),
            Err(Invalid) if reverse_oversized => Err(Oversized),
            Err(error) => Err(error),
        }
    }

    pub(in crate::acp::bindings::codex::incoming::items::file_change) fn needs_context(
        &self,
    ) -> bool {
        self.hunks.len() > 1
    }

    fn apply(&self, context: &str, reverse: bool) -> Result<String, PatchError> {
        let lines: Vec<_> = context.split_inclusive('\n').collect();
        let mut cursor = 0;
        let mut result = String::new();
        for hunk in &self.hunks {
            let (start, source, replacement) = if reverse {
                (
                    hunk.new_start,
                    &self.new_text[hunk.new_bytes.clone()],
                    &self.old_text[hunk.old_bytes.clone()],
                )
            } else {
                (
                    hunk.old_start,
                    &self.old_text[hunk.old_bytes.clone()],
                    &self.new_text[hunk.new_bytes.clone()],
                )
            };
            for line in lines.get(cursor..start).ok_or(Invalid)? {
                append(&mut result, line)?;
            }
            let consumed = source.split_inclusive('\n').count();
            let end = start.checked_add(consumed).ok_or(Invalid)?;
            if !lines
                .get(start..end)
                .ok_or(Invalid)?
                .iter()
                .copied()
                .eq(source.split_inclusive('\n'))
            {
                return Err(Invalid);
            }
            if (!replacement.is_empty() && !result.is_empty() && !result.ends_with('\n'))
                || (!replacement.is_empty() && !replacement.ends_with('\n') && end < lines.len())
            {
                return Err(Invalid);
            }
            append(&mut result, replacement)?;
            cursor = end;
        }
        for line in lines.get(cursor..).ok_or(Invalid)? {
            append(&mut result, line)?;
        }
        Ok(result)
    }
}
