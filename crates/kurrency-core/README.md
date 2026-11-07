# kurrency-core

Kurrency's model without the UI: the arithmetic typed into an amount, currency
metadata and number formats, how amounts and rates read on screen, the
palette's commands and search, prices pasted from the clipboard, the rate
tables with their providers' formats and caches, the history maths, settings
and their migrations, undo, and the UI strings (`locales/` at the repository
root, looked up with `kurrency_core::tr!` from both crates).

It must not depend on gpui-kit or any GPUI crate; CI checks the dependency
graph. That keeps its tests fast and headless:

```sh
cargo nextest run -p kurrency-core   # or: cargo test -p kurrency-core
```

The GPUI application, including fetching rates over HTTP and every view, is
[`crates/kurrency`](../kurrency). Not published; the API is `pub` only where
the app uses it.
