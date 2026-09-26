# Sample feed (synthetic)

`sample_feed.csv` is **invented test data** in the general shape of an Impact
product catalog export. Brands, products, prices, ratings and ids are made up;
the 9000xx ids do not refer to real products, and `tracking.example` is a
reserved domain. It exists so that `vitrina ingest --feed-file` and the tests
can run before the network provides a real sample (question О3).

Rows 13–17 exercise the edge cases: a product outside the category, a
combination product that must not match `Magnesium`, a non-USD price, a
duplicate id and an unknown availability value.

When the real feed arrives, add a sample of it here next to this file and
update `crates/feed-ingest/src/columns.rs`; see
`docs/adr/0013-feed-format-assumed.md`.
