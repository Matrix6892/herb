# Labels

One file per product, `<iherb_id>.toml`, entered by hand from the label photo
(spec §4.2). The schema is `catalog_core::schema::LabelFile`; a fixture
example is `data/fixtures/labels/900006.toml` (two versions after a formula
change).

Rules (spec §10.3.4):

- every `[[label]]` has `verified_by` and `verified_at`;
- amounts are copied exactly as printed (`"200 mg"`, `"1000 IU"`), with
  `declared_as = "salt" | "elemental" | "iu"` saying what the number is;
- a line that cannot be read with certainty is entered with
  `confidence = "unknown"`; the product then stays out of comparison and its
  page says why (И7);
- a formula change appends a new `[[label]]` with a later `effective_from`.
  Old versions stay.

`vitrina import-labels` validates every file against `data/reference/` and
refuses the whole import on the first error.

This directory is empty until labels of real products are entered (milestone
M2). Label data must come from product photos, never from store pages (И1).
