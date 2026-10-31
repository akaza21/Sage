# Third-party notices

Sage is licensed under the Apache License, Version 2.0. It ships with
third-party components that remain under their own licenses.

Release packaging generates the full list of bundled dependencies, with
their license texts, from the lockfiles:

- Rust crates: src-tauri/Cargo.lock
- npm packages bundled into the WebView: package-lock.json

## Platform components

| Component | License | How Sage uses it |
| --- | --- | --- |
| Microsoft Edge WebView2 Runtime (Windows) | Microsoft Software License Terms | System runtime; not redistributed |

Entries for any code adapted from another project go below, one per
source, with its license and the files that contain it.

## Adapted code

None yet.