# Delivery roadmap

Keep Agent and Editor aligned around shared workbench resources, bounded
session state and native ownership. Track task delivery in
[the feature roadmap](FEATURE-ROADMAP.md).

Changes must preserve lazy loading for inactive heavy views, existing bundle
caps and the production Git CLI boundary. Workspace walks admit at most 200,000
files and skip `.git` and `node_modules`. WebView lists remain capped at 500
Explorer entries, 500 Search matches and 300 Git changes.

Every delivery requires format, lint, behavior tests, budget checks, a desktop
build, web bundle verification and policy checks. Changed Rust modules also
require formatting, Clippy and tests. UI changes require the relevant browser
journeys. Run these checks independently so an earlier failure cannot conceal a
later result; do not weaken assertions or caps to complete a delivery.
