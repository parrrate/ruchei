# State of `Iterator` and `Stream` methods

> [!NOTE]
> current document hasn't been thoroughly checked yet

versions

| crate              | version  |
| ------------------ | -------: |
| `core`             | `1.90.0` |
| `itertools`        | `0.14.0` |
| `futures-lite`     | `2.6.1`  |
| `futures-util`     | `0.3.31` |
| `tokio-stream`     | `0.1.17` |
| `ruchei-itertools` | unstable |

methods

\`&mldr;' here means the base `trait`s (or extension `trait`s in base crate) implement those methods

|                          | `core`  | `itertools` | `futures-util` | `tokio-stream` | `futures-lite` | `ruchei-itertools` |
| ------------------------ | :-----: | :---------: | :------------: | :------------: | :------------: | :----------------: |
| `advance_by`             | &check; | &mldr;      |                |                |                |                    |
| `all`                    | &check; | &mldr;      | &check;        | &check;        | &check;        | &mldr;             |
| `all_equal`              |         | &check;     |                |                |                |                    |
| `all_equal_value`        |         | &check;     |                |                |                |                    |
| `all_unique`             |         | &check;     |                |                |                |                    |
| `and_then`               |         |             | &check;        |                |                | &mldr;             |
| `any`                    | &check; | &mldr;      | &check;        | &check;        | &check;        | &mldr;             |
| `array_chunks`           | &check; | &mldr;      |                |                |                |                    |
| `array_combinations`     |         | &check;     |                |                |                |                    |
| `batching`               |         | &check;     |                |                |                |                    |
| `buffer_local`           |         |             | &check;        |                |                | &mldr;             |
| `buffer_unordered`       |         |             | &check;        |                |                | &mldr;             |
| `by_ref`                 | &check; | &mldr;      | &check;        |                |                | &mldr;             |
| `cartesian_product`      |         | &check;     |                |                |                |                    |
| `catch_unwind`           |         |             | &check;        |                |                | &mldr;             |
| `chain`                  | &check; | &mldr;      | &check;        | &check;        | &check;        | &mldr;             |
| `chunk_by`               |         | &check;     |                |                |                |                    |
| `chunks`                 |         | &check;     | &check;        |                |                | &mldr;             |
| `chunks_timeout`         |         |             |                | &check;        |                |                    |
| `circulat_tuple_windows` |         | &check;     |                |                |                |                    |
| `cloned`                 | &check; | &mldr;      |                |                | &check;        |                    |
| `cmp`                    | &check; | &mldr;      |                |                |                |                    |
| `cmp_by`                 | &check; | &mldr;      |                |                |                |                    |
| `coalesce`               |         | &check;     |                |                |                |                    |
| `collect`                | &check; | &mldr;      | &check;        | &check;        | &check;        | &mldr;             |
| `collect_array`          |         | &check;     |                |                |                |                    |
| `collect_into`           | &check; | &mldr;      |                |                |                |                    |
| `collect_tuple`          |         | &check;     |                |                |                |                    |
| `collect_vec`            |         | &check;     |                |                |                |                    |
| `combinations`           |         | &check;     |                |                |                |                    |
| `combinations_with_...`  |         | &check;     |                |                |                |                    |
| `concat`                 |         | &check;     | &check;        |                |                | &mldr;             |
| `copied`                 | &check; | &mldr;      |                |                | &check;        |                    |
| `count`                  | &check; | &mldr;      | &check;        |                | &check;        | &mldr;             |
| `counts`                 |         | &check;     |                |                |                |                    |
| `counts_by`              |         | &check;     |                |                |                |                    |
| `cycle`                  | &check; | &mldr;      | &check;        |                | &check;        | &mldr;             |
| `dedup`                  |         | &check;     |                |                |                |                    |
| `dedup_by`               |         | &check;     |                |                |                |                    |
| `dedup_by_with_count`    |         | &check;     |                |                |                |                    |
| `dedup_eager`            |         | &check;     |                |                |                | &check;            |
| `dedup_with_count`       |         | &check;     |                |                |                |                    |
| `drain`                  |         |             |                |                | &check;        |                    |
| `dropping`               |         | &check;     |                |                |                |                    |
| `dropping_back`          |         | &check;     |                |                |                |                    |
| `duplicates`             |         | &check;     |                |                |                |                    |
| `duplicates_by`          |         | &check;     |                |                |                |                    |
| `enumerate`              | &check; | &mldr;      | &check;        |                | &check;        | &mldr;             |
| `eq`                     | &check; | &mldr;      |                |                |                |                    |
| `eq_by`                  | &check; | &mldr;      |                |                |                |                    |
| `err_into`               |         |             | &check;        |                |                | &mldr;             |
| `exactly_one`            |         | &check;     |                |                |                |                    |
| `filter`                 | &check; | &mldr;      | &check;        | &check;        | &check;        | &mldr;             |
| `filter_map`             | &check; | &mldr;      | &check;        | &check;        | &check;        | &mldr;             |
| `filter_map_ok`          |         | &check;     |                |                |                |                    |
| `filter_ok`              |         | &check;     |                |                |                |                    |
| `find`                   | &check; | &mldr;      |                |                | &check;        |                    |
| `find_map`               | &check; | &mldr;      |                |                | &check;        |                    |
| `find_or_first`          |         | &check;     |                |                |                |                    |
| `find_or_last`           |         | &check;     |                |                |                |                    |
| `find_position`          |         | &check;     |                |                |                |                    |
| `flat_map`               | &check; | &mldr;      | &check;        |                | &check;        | &mldr;             |
| `flap_map_unordered`     |         |             | &check;        |                |                | &mldr;             |
| `flatten`                | &check; | &mldr;      | &check;        |                | &check;        | &mldr;             |
| `flatten_ok`             |         | &check;     |                |                |                |                    |
| `flatten_unordered`      |         |             | &check;        |                |                | &mldr;             |
| `fold`                   | &check; | &mldr;      | &check;        | &check;        | &check;        | &mldr;             |
| `fold_ok`                |         | &check;     |                |                |                |                    |
| `fold_options`           |         | &check;     |                |                |                |                    |
| `fold_while`             |         | &check;     |                |                |                |                    |
| `for_each`               | &check; | &mldr;      | &check;        |                | &check;        | &mldr;             |
| `for_each_concurrent`    |         |             | &check;        |                |                | &mldr;             |
| `format`                 |         | &check;     |                |                |                |                    |
| `format_with`            |         | &check;     |                |                |                |                    |
| `forward`                |         |             | &check;        |                |                | &mldr;             |
| `fuse`                   | &check; | &mldr;      | &check;        | &check;        | &check;        | &mldr;             |
| `ge`                     | &check; | &mldr;      |                |                |                |                    |
| `get`                    |         | &check;     |                |                |                |                    |
| `gt`                     | &check; | &mldr;      |                |                |                |                    |
| `inspect`                | &check; | &mldr;      | &check;        |                | &check;        | &mldr;             |
| `inspect_err`            |         |             | &check;        |                |                | &mldr;             |
| `inspect_ok`             |         |             | &check;        |                |                | &mldr;             |
| `interleave`             |         | &check;     |                | [^tm]          |                | &check;            |
| `interleave_shortest`    |         | &check;     |                |                |                |                    |
| `intersperse`            | &check; | &check;     |                |                |                |                    |
| `intersperse_with`       | &check; | &check;     |                |                |                |                    |
| `into_async_read`        |         |             | &check;        |                |                | &mldr;             |
| `into_future`            |         |             | &check;        |                |                | &mldr;             |
| `into_group_map`         |         | &check;     |                |                |                |                    |
| `into_group_map_by`      |         | &check;     |                |                |                |                    |
| `into_grouping_map`      |         | &check;     |                |                |                |                    |
| `into_grouping_map_by`   |         | &check;     |                |                |                |                    |
| `is_partitioned`         | &check; | &mldr;      |                |                |                |                    |
| `is_sorted`              | &check; | &mldr;      |                |                |                |                    |
| `is_sorted_by`           | &check; | &mldr;      |                |                |                |                    |
| `is_sorted_by_key`       | &check; | &mldr;      |                |                |                |                    |
| `join`                   |         | &check;     |                |                |                |                    |
| `k_largest`              |         | &check;     |                |                |                |                    |
| `k_largest_by`           |         | &check;     |                |                |                |                    |
| `k_largest_by_key`       |         | &check;     |                |                |                |                    |
| `k_largest_relaxed`      |         | &check;     |                |                |                |                    |
| `k_largest_relaxed_by`   |         | &check;     |                |                |                |                    |
| `k_largest_relaxed_...`  |         | &check;     |                |                |                |                    |
| `k_smallest`             |         | &check;     |                |                |                |                    |
| `k_smallest_by`          |         | &check;     |                |                |                |                    |
| `k_smallest_by_key`      |         | &check;     |                |                |                |                    |
| `k_smallest_relaxed`     |         | &check;     |                |                |                |                    |
| `k_smallest_relaxed_by`  |         | &check;     |                |                |                |                    |
| `k_smallest_relaxed_...` |         | &check;     |                |                |                |                    |
| `k_merge`                |         | &check;     |                |                |                |                    |
| `k_merge_by`             |         | &check;     |                |                |                |                    |
| `last`                   | &check; | &mldr;      |                |                | &check;        |                    |
| `le`                     | &check; | &mldr;      |                |                |                |                    |
| `left_stream`            |         |             | &check;        |                |                | &mldr;             |
| `lt`                     | &check; | &mldr;      |                |                |                |                    |
| `map`                    | &check; | &mldr;      | &check;        | &check;        | &check;        | &mldr;             |
| `map_err`                |         |             | &check;        |                |                | &mldr;             |
| `map_into`               |         | &check;     |                |                |                |                    |
| `map_ok`                 |         | &check;     |                |                |                |                    |
| `map_while`              | &check; | &mldr;      |                | &check;        | &check;        |                    |
| `max`                    | &check; | &mldr;      |                |                |                |                    |
| `max_by`                 | &check; | &mldr;      |                |                |                |                    |
| `max_by_key`             | &check; | &mldr;      |                |                |                |                    |
| `max_set`                |         | &check;     |                |                |                |                    |
| `max_set_by`             |         | &check;     |                |                |                |                    |
| `max_set_by_key`         |         | &check;     |                |                |                |                    |
| `merge`                  |         | &check;     |                | [^tm]          |                |                    |
| `merge_by`               |         | &check;     |                |                |                |                    |
| `merge_join_by`          |         | &check;     |                |                |                |                    |
| `min`                    | &check; | &mldr;      |                |                |                |                    |
| `min_by`                 | &check; | &mldr;      |                |                |                |                    |
| `min_by_key`             | &check; | &mldr;      |                |                |                |                    |
| `min_set`                |         | &check;     |                |                |                |                    |
| `min_set_by`             |         | &check;     |                |                |                |                    |
| `min_set_by_key`         |         | &check;     |                |                |                |                    |
| `minmax`                 |         | &check;     |                |                |                |                    |
| `minmax_by`              |         | &check;     |                |                |                |                    |
| `minmax_by_key`          |         | &check;     |                |                |                |                    |
| `multi_cartesian_...`    |         | &check;     |                |                |                |                    |
| `multipeek`              |         | &check;     |                |                |                |                    |
| `multiunzip`             |         | &check;     |                |                |                |                    |
| `ne`                     | &check; | &mldr;      |                |                |                |                    |
| `next`                   | &check; | &mldr;      | &check;        | &check;        | &check;        | &mldr;             |
| `next_array`             |         | &check;     |                |                |                |                    |
| `next_chunk`             | &check; | &mldr;      |                |                |                |                    |
| `next_tuple`             |         | &check;     |                |                |                |                    |
| `nth`                    | &check; | &mldr;      |                |                | &check;        |                    |
| `or`                     |         |             |                |                | &check;        |                    |
| `or_else`                |         |             | &check;        |                |                | &mldr;             |
| `pad_using`              |         | &check;     |                |                |                |                    |
| `partial_cmp`            | &check; | &mldr;      |                |                |                |                    |
| `partial_cmp_by`         | &check; | &mldr;      |                |                |                |                    |
| `partition`              | &check; | &mldr;      |                |                | &check;        |                    |
| `partition_in_place`     | &check; | &mldr;      |                |                |                |                    |
| `partition_map`          |         | &check;     |                |                |                |                    |
| `partition_result`       |         | &check;     |                |                |                |                    |
| `peekable`               | &check; | &mldr;      | &check;        | &check;        |                | &mldr;             |
| `peeking_take_while`     |         | &check;     |                |                |                |                    |
| `permutations`           |         | &check;     |                |                |                |                    |
| `position`               | &check; | &check;     |                |                | &check;        |                    |
| `position_max`           |         | &check;     |                |                |                |                    |
| `position_max_by`        |         | &check;     |                |                |                |                    |
| `position_max_by_key`    |         | &check;     |                |                |                |                    |
| `position_min`           |         | &check;     |                |                |                |                    |
| `position_min_by`        |         | &check;     |                |                |                |                    |
| `position_min_by_key`    |         | &check;     |                |                |                |                    |
| `position_minmax`        |         | &check;     |                |                |                |                    |
| `position_minmax_by`     |         | &check;     |                |                |                |                    |
| `position_minmax_by_key` |         | &check;     |                |                |                |                    |
| `positions`              |         | &check;     |                |                |                |                    |
| `powerset`               |         | &check;     |                |                |                |                    |
| `process_results`        |         | &check;     |                |                |                |                    |
| `product`                | &check; | &mldr;      |                |                |                |                    |
| `product1`               |         | &check;     |                |                |                |                    |
| `race`                   |         |             |                |                | &check;        |                    |
| `ready_chunks`           |         |             | &check;        |                |                | &mldr;             |
| `reduce`                 | &check; | &mldr;      |                |                |                |                    |
| `rev`                    | &check; | &mldr;      |                |                |                |                    |
| `right_stream`           |         |             | &check;        |                |                | &mldr;             |
| `rposition`              | &check; | &mldr;      |                |                |                |                    |
| `scan`                   | &check; | &mldr;      | &check;        |                | &check;        | &mldr;             |
| `select_next_some`       |         |             | &check;        |                |                | &mldr;             |
| `set_from`               |         | &check;     |                |                |                |                    |
| `size_hint`              | &check; | &mldr;      | &mldr;         | &mldr;         |                | &mldr;             |
| `skip`                   | &check; | &mldr;      | &check;        | &check;        | &check;        | &mldr;             |
| `skip_while`             | &check; | &mldr;      | &check;        | &check;        | &check;        | &mldr;             |
| `sorted`                 |         | &check;     |                |                |                |                    |
| `sorted_by`              |         | &check;     |                |                |                |                    |
| `sorted_by_cached_key`   |         | &check;     |                |                |                |                    |
| `sorted_by_key`          |         | &check;     |                |                |                |                    |
| `sorted_unstable`        |         | &check;     |                |                |                |                    |
| `sorted_unstable_by`     |         | &check;     |                |                |                |                    |
| `sorted_unstable_by_key` |         | &check;     |                |                |                |                    |
| `split`                  |         |             | &check;        |                |                | &mldr;             |
| `step_by`                | &check; | &mldr;      |                |                | &check;        |                    |
| `sum`                    | &check; | &mldr;      |                |                |                |                    |
| `sum1`                   |         | &check;     |                |                |                |                    |
| `tail`                   |         | &check;     |                |                |                |                    |
| `take`                   | &check; | &mldr;      | &check;        | &check;        | &check;        | &mldr;             |
| `take_until`             |         |             | &check;        |                |                | &mldr;             |
| `take_while`             | &check; | &mldr;      | &check;        | &check;        | &check;        | &mldr;             |
| `take_while_inclusive`   |         | &check;     |                |                |                |                    |
| `take_while_ref`         |         | &check;     |                |                |                |                    |
| `tee`                    |         | &check;     |                |                |                |                    |
| `then`                   |         |             | &check;        | &check;        | &check;        | &mldr;             |
| `throttle`               |         |             |                | &check;        |                |                    |
| `timeout`                |         |             |                | &check;        |                |                    |
| `timeout_repeating`      |         |             |                | &check;        |                |                    |
| `tree_reduce`            |         | &check;     |                |                |                |                    |
| `try_all`                |         |             | &check;        |                |                | &mldr;             |
| `try_any`                |         |             | &check;        |                |                | &mldr;             |
| `try_buffer_unordered`   |         |             | &check;        |                |                | &mldr;             |
| `try_buffered`           |         |             | &check;        |                |                | &mldr;             |
| `try_chunks`             |         |             | &check;        |                |                | &mldr;             |
| `try_collect`            | &check; | &check;     | &check;        |                | &check;        | &mldr;             |
| `try_concat`             |         |             | &check;        |                |                | &mldr;             |
| `try_filter`             |         |             | &check;        |                |                | &mldr;             |
| `try_filter_map`         |         |             | &check;        |                |                | &mldr;             |
| `try_find`               | &check; | &mldr;      |                |                |                |                    |
| `try_flatten`            |         |             | &check;        |                |                | &mldr;             |
| `try_flatten_unordered`  |         |             | &check;        |                |                | &mldr;             |
| `try_fold`               | &check; | &mldr;      | &check;        |                | &check;        | &mldr;             |
| `try_for_each`           | &check; | &mldr;      | &check;        |                | &check;        | &mldr;             |
| `try_for_each_...`       |         |             | &check;        |                |                | &mldr;             |
| `try_len`                |         | &check;     |                |                |                |                    |
| `try_next`               |         |             |                | &check;        | &check;        |                    |
| `try_ready_chunks`       |         |             | &check;        |                |                | &mldr;             |
| `try_skip_while`         |         |             | &check;        |                |                | &mldr;             |
| `try_take_while`         |         |             | &check;        |                |                | &mldr;             |
| `tuple_combinations`     |         | &check;     |                |                |                |                    |
| `tuple_windows`          |         | &check;     |                |                |                |                    |
| `tuples`                 |         | &check;     |                |                |                |                    |
| `unique`                 |         | &check;     |                |                |                |                    |
| `unique_by`              |         | &check;     |                |                |                |                    |
| `unzip`                  | &check; | &mldr;      | &check;        |                | &check;        | &mldr;             |
| `update`                 |         | &check;     |                |                |                |                    |
| `while_some`             |         | &check;     |                |                |                |                    |
| `with_position`          |         | &check;     |                |                |                |                    |
| `zip`                    | &check; | &mldr;      | &check;        |                | &check;        | &mldr;             |
| `zip_eq`                 |         | &check;     |                |                |                |                    |
| `zip_longest`            |         | &check;     |                |                |                |                    |

[^tm]: `tokio` calls `interleave` \``merge`'
