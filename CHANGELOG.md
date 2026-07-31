## [0.2.2] - 2026-07-31

### Added

- `ConnectOptions::session_retry_limit` / `session_retry_interval`, controlling
  how the underlying `opcua` session reconnects and re-attaches its existing
  subscriptions after a dropped connection. Defaults raised from `opcua`'s own
  10 retries / 10s apart to 20 retries / 2s apart.
- `MonitorOptions::reconnect_backoff_min`, `restart_backoff_min`, and
  `restart_backoff_max`: exponential-backoff-with-jitter tuning for the poller
  and for `Collector`'s own restart loop
- `Collector::resilient` (on by default): `run()` now restarts automatically
  — fresh connect, fresh scan-or-cache-load, fresh subscriptions — whenever a
  run attempt ends for a recoverable reason, instead of returning. Bad
  credentials or a client that cannot be built at all still return
  immediately, since retrying those can never succeed.
- `Error::is_recoverable`

### Changed

- `MonitorOptions::health_check_interval` default lowered from 3s to 1s (the
  check is a local flag read, not a network call, so this is free)
- The poller's fixed `reconnect_delay` between attempts is now exponential
  backoff starting at `reconnect_backoff_min`, so a short blip recovers almost
  immediately instead of always waiting the full delay

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