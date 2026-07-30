## [0.2.0] - 2026-07-30

### Added
- `Collector`: connect, discover, subscribe, poll, reconnect, and stream changes
  to a sink, with `CollectorHandle` for shutdown from another thread
- `TagClient` for reading and writing individual values by browse path
- `TagSet`, indexing tags for lookup by path, name, node id, or glob
- `TagSink` with `TagChange` / `ConnectionChange`, implemented by any closure;
  `sinks::LogSink` and `sinks::JsonlSink` ship ready to use
- Tag selection via `Collector::only`, `matching`, and `select`
- `MonitorOptions`, including a deadband filter for noisy analogue values
- `Security` and `Credentials` on `ConnectOptions`, plus `ConnectOptions::insecure`
- `discover` free function for a one-off scan of an endpoint
- `PlcSession::write`; `variant_as_f64`, `variant_as_i64`, `variant_as_bool`
- `ScanOptions::max_depth` and `descend_into_variables` builder methods
- Features: `monitoring` (default), `jsonl-sink`, `ctrl-c`, `full`

### Changed
- **Breaking:** `ConnectOptions::default` now signs and encrypts with
  `Basic256Sha256` and rejects untrusted server certificates. Plaintext servers
  need `ConnectOptions::insecure`, which records the decision in your source.
- **Breaking:** `NodeFilter`'s blanket impl now requires `Fn(&str) -> bool + Send + Sync`
- Sending credentials over an unencrypted channel fails with
  `Error::InsecureCredentials` instead of transmitting them; `Credentials`
  redacts passwords in `Debug`
- New `Error` variants: `MissingSink`, `SinkSetup`, `InsecureCredentials`,
  `UnknownTag`, `BadNodeId`, `Write`, `Read`

## [0.1.3] - 2026-07-29

### Added
- README with usage guide, design notes, and version compatibility table
- Expanded crate-level documentation

### Changed
- `#![deny(rustdoc::broken_intra_doc_links)]` now enforced at build time