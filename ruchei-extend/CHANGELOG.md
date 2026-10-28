# Changelog

## [Unreleased]

## [0.1.5] — 2026-10-28

### Changed

- now shares incoming stream wakers between `next` and `ready`, only works with `std` feature

- `impl` is now restricted requiring the incoming stream to be a stream

## [0.1.4] — 2026-04-28

### Changed

- dropped `FusedStream` requirement for `ExtendingKeyedExt`

## [0.1.3] — 2025-10-23

### Changed

- dropped `FusedStream` requirement

### Removed

- erroneously derived traits for `Extending`. ***this is a breaking change***, but you shouldn't've
  been using `Extending` this way in any case

## [0.1.2] — 2025-09-22

### Added

- `extending_keyed` (gated behind `std` + `unstable`)

## [0.1.1] — 2025-09-20

(baseline)

[unreleased]: https://github.com/parrrate/ruchei/compare/ruchei-extend/0.1.5...HEAD
[0.1.5]: https://github.com/parrrate/ruchei/compare/ruchei-extend/0.1.4...ruchei-extend/0.1.5
[0.1.4]: https://github.com/parrrate/ruchei/compare/ruchei-extend/0.1.3...ruchei-extend/0.1.4
[0.1.3]: https://github.com/parrrate/ruchei/compare/ruchei-extend/0.1.2...ruchei-extend/0.1.3
[0.1.2]: https://github.com/parrrate/ruchei/compare/ruchei-extend/0.1.1...ruchei-extend/0.1.2
[0.1.1]: https://github.com/parrrate/ruchei/releases/tag/ruchei-extend/0.1.1
