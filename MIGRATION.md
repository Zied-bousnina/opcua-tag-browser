# 0.1.3 -> 0.2.0

## Replace outright

    Cargo.toml
    src/lib.rs
    src/error.rs
    src/session.rs      (adds `write` to PlcSession)
    src/variant.rs      (adds variant_as_f64 / _i64 / _bool)
    src/connection.rs   (from the 0.2.0 zip; security rework)

## Add

    src/tagset.rs
    src/sink.rs
    src/writer.rs
    src/collector.rs
    src/sinks/mod.rs
    src/sinks/log_sink.rs
    src/sinks/jsonl.rs
    src/monitor/mod.rs
    src/monitor/subscriber.rs
    src/monitor/poller.rs
    src/monitor/watchdog.rs
    examples/collect.rs
    examples/write.rs
    tests/tagset.rs

## Edit by hand

### src/filter.rs

    F: Fn(&str) -> bool + Send + Sync,

### src/scanner.rs

Ensure `#[derive(Debug, Clone)]` on `ScanOptions`, then add:

```rust
impl ScanOptions {
    /// Sets the maximum recursion depth below the root node.
    pub fn max_depth(mut self, depth: usize) -> Self {
        self.max_depth = depth;
        self
    }

    /// Sets whether to descend into the children of `Variable` nodes.
    pub fn descend_into_variables(mut self, yes: bool) -> Self {
        self.descend_into_variables = yes;
        self
    }
}
```

### src/repository.rs

`TagRepository` must be object safe with `Send + Sync`. No code change needed;
`JsonFileTagRepository` holds only a `PathBuf`.

### examples/scan_and_dump.rs

`ConnectOptions::default()` -> `ConnectOptions::insecure()`, or the example
fails against a plaintext server.

## Unchanged

    src/tag.rs
    src/browser.rs
    src/repository.rs
    tests/scanner.rs
    LICENSE-MIT, LICENSE-APACHE, .gitignore
