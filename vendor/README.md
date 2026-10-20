# Vendored crates

## gpui-pre-linux

`gpui-pre-linux` 0.3.6 from crates.io, Zed's `gpui_linux` crate at
zed-industries/zed@bcf6582ce3500df93a8a39366640173e6786cea6. It replaces the
registry crate through `[patch.crates-io]` in the workspace `Cargo.toml` and
is excluded from the workspace, so it keeps its upstream layout.

The first commit adding it is a byte-for-byte copy of the published crate
(without the registry's `Cargo.toml.orig` and `.cargo-ok`). Later commits carry
only fixes that are also proposed upstream to Zed; drop the vendored copy once
a `gpui-pre` release includes them.
