# Changelog

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [unreleased]

### Changed

- Dropped `Clone` bound for `group_sequential` context

### [0.0.96] - 2025-07-19

### Added

- `group_sequential`

### Changed

- `group_by_key` renamed to `group_concurrent`

### Fixed

- Replaced broken doc links

## [0.0.95] - 2024-10-03

### Added

- `Slab`-based combinators
  - `multicast::bufferless_slab`
  - `deal::slab`
  - `route::slab`
- `CloseAll<R,_>: From<R>`

### Changed

- require `Ord` for `Key`
- `#[must_use]` on many things
- moved `route::Router*` to `route::keyed`
- moved `deal::Dealer*` to `deal::keyed`

### Deprecated

- `deal`
  - `::Dealer`
  - `::DealerExtending`
  - `::DealerExt`
- `route`
  - `::Router`
  - `::RouterExtending`
  - `::RouterExt`

### Fixed

- Restricted `ConcurrentExt` to apply to `FusedStream` only

## [0.0.94] - 2024-07-16

### Changed

- `Unroute` has been moved to `ruchei-route`

## [0.0.93] - 2024-07-09

(baseline)

[unreleased]: https://github.com/parrrate/ruchei/compare/0.0.96..HEAD
[0.0.96]: https://github.com/parrrate/ruchei/compare/0.0.95..0.0.96
[0.0.95]: https://github.com/parrrate/ruchei/compare/0.0.94..0.0.95
[0.0.94]: https://github.com/parrrate/ruchei/compare/0.0.94..0.0.95
[0.0.93]: https://github.com/parrrate/releases/tag/0.0.93
