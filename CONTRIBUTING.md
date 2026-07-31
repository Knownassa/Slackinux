# Contributing to Slackinux

Thanks for helping improve Slackinux.

## Development setup

Install Rust, Node.js, npm, GTK 3, and the WebKitGTK 4.1 development package.
Then run:

```bash
cd apps/desktop
npm install
npm run build
cd ../..
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

## Pull requests

Keep changes focused, explain the user-facing impact, and include tests for
behavioral changes where practical. Run formatting, tests, and Clippy before
opening a pull request:

```bash
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

Slackinux is an unofficial shell around Slack Web. Do not add Slack
credentials, tokens, private APIs, or code that bypasses Slack security.
