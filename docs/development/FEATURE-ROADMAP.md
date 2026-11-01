# Feature roadmap

## GAP-B review follow-up

- GAP-B04: fixed user Copy and timestamp reveal on hover and keyboard focus.
  Browser checks assert computed opacity and stable bubble geometry.
- GAP-B09: fixed keyboard access between the production model popup's
  selected option and Search. Search reaches models beyond the 200-row cap.
- GAP-B11: browser checks compare the composer's focused and unfocused shadow
  and verify the ring disappears after focus leaves the text field.
- Bundle catalog: register ChatModelPicker, InheritedChatHistory,
  chatImagePreviewCache and retryChatTurn as required dynamic facades.
- GAP-B14: incomplete; the review identifies cross-lane dependencies. No
  dependency implementation or acceptance specification is present in this
  lane's tracked work items.
- GAP-B15: partial; resolved file references have file-kind treatment. Web-link
  treatment remains incomplete pending the cross-lane dependency identified
  in the review. This follow-up does not claim completion of either item.
