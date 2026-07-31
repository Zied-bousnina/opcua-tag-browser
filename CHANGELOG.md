## [0.2.1] - 2026-07-31

Version-only release: `0.2.0` was already claimed on crates.io by an earlier
publish attempt, so this ships the same content under `0.2.1`. The entries
below were not previously changelogged.

### Added

- `PlcTag` now carries the OPC 10000-3 Table 13 `Variable` attributes:
  `browse_name`, `data_type`, `value_rank`, `access_level`,
  `min_sampling_interval`, `historizing`, `type_definition`, `reference_type`,
  read in a batched pass after each scan (`Collector::read_attributes`,
  default on)
- `PlcTag::is_readable`, `is_writable`, `has_history`, `is_array`, `is_property`
- `ScanOptions::include_properties`, excluding `HasProperty` metadata nodes
  (`EngineeringUnits`, `EURange`, ...) from scans by default
- `Error::NotWritable`

### Changed

- **Breaking:** `PlcTag::path` is now built from `BrowseName` rather than
  `DisplayName`, which is stable across server locales. Existing caches load,
  but paths may differ; rescan to refresh.
- `TagClient::set` refuses a write to a tag whose `AccessLevel` says read-only,
  returning `Error::NotWritable` without a round trip
- Subscriptions use each tag's own `MinimumSamplingInterval` when the server
  reported one, instead of one sampling interval applied uniformly
- Fixed a broken intra-doc link (`PlcTag`'s docs pointed at
  `ScanOptions::read_attributes`, which doesn't exist; the toggle is
  `Collector::read_attributes`)

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