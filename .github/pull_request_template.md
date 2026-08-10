<!-- What changed, and why. The "why" is what review spends its time on. -->

## What this changes

## Why

## Checks

- [ ] `cargo fmt --all --check`
- [ ] `cargo clippy --all-targets --all-features`
- [ ] `cargo test`
- [ ] Rendering changes have a test pinning the output, not just a screenshot
- [ ] Anything drawing document-derived text goes through `Sink::push`, or says why not
