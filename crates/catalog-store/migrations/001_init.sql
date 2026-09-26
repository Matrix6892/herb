-- 001: initial schema (spec §4.2, §4.3).
--
-- Masses are integer micrograms, money integer US cents, dates TEXT
-- 'YYYY-MM-DD', timestamps INTEGER Unix seconds UTC. Unknown values are
-- NULL, never zero (И7). Migrations only go forward.

-- Reference (data/reference/*.toml), replaced as a whole on import.
CREATE TABLE substance (
    id            TEXT PRIMARY KEY,
    name          TEXT NOT NULL,
    synonyms_json TEXT NOT NULL,
    default_unit  TEXT NOT NULL,
    element       TEXT,
    ug_per_iu     TEXT,
    source_url    TEXT NOT NULL
) STRICT;

CREATE TABLE form (
    id                   TEXT PRIMARY KEY,
    substance_id         TEXT NOT NULL REFERENCES substance(id) DEFERRABLE INITIALLY DEFERRED,
    name                 TEXT NOT NULL,
    synonyms_json        TEXT NOT NULL,
    elemental_ratio      TEXT NOT NULL,
    formula              TEXT,
    bioavailability_k    TEXT,
    bioavailability_note TEXT,
    ug_per_iu            TEXT,
    source_url           TEXT NOT NULL
) STRICT;

CREATE TABLE category (
    id              TEXT PRIMARY KEY,
    slug            TEXT NOT NULL UNIQUE,
    name            TEXT NOT NULL,
    substance_id    TEXT NOT NULL REFERENCES substance(id) DEFERRABLE INITIALLY DEFERRED,
    unit            TEXT NOT NULL,
    unit_label      TEXT NOT NULL,
    feed_match_json TEXT NOT NULL
) STRICT;

-- Products seen in the feed.
CREATE TABLE product (
    iherb_id     TEXT PRIMARY KEY,
    slug         TEXT NOT NULL,
    brand        TEXT NOT NULL,
    title        TEXT NOT NULL,
    category_id  TEXT NOT NULL,
    status       TEXT NOT NULL CHECK (status IN ('active', 'pending_label', 'delisted')),
    tracking_url TEXT,
    label_ref    TEXT,
    first_seen   TEXT NOT NULL,
    last_seen    TEXT NOT NULL
) STRICT;
CREATE INDEX product_category ON product(category_id);

-- Hand-entered labels (data/labels/*.toml). One row per version; a formula
-- change adds a row with a later effective_from.
CREATE TABLE label (
    id                     INTEGER PRIMARY KEY,
    product_id             TEXT NOT NULL,
    effective_from         TEXT NOT NULL,
    serving_size           INTEGER NOT NULL CHECK (serving_size > 0),
    servings_per_container INTEGER NOT NULL CHECK (servings_per_container > 0),
    verified_by            TEXT NOT NULL CHECK (length(trim(verified_by)) > 0),
    verified_at            TEXT NOT NULL,
    label_photo_ref        TEXT,
    third_party_test       TEXT,
    UNIQUE (product_id, effective_from)
) STRICT;

CREATE TABLE label_line (
    label_id    INTEGER NOT NULL REFERENCES label(id) ON DELETE CASCADE,
    position    INTEGER NOT NULL,
    form_id     TEXT NOT NULL REFERENCES form(id) DEFERRABLE INITIALLY DEFERRED,
    declared_as TEXT NOT NULL CHECK (declared_as IN ('salt', 'elemental', 'iu')),
    -- micrograms for 'salt' and 'elemental', whole IU for 'iu'
    amount      INTEGER NOT NULL CHECK (amount >= 0),
    confidence  TEXT NOT NULL CHECK (confidence IN ('verified', 'recognized', 'unknown')),
    PRIMARY KEY (label_id, position)
) STRICT;

-- Daily observations from the feed. Kept from day one for price history in
-- phase 2 even though phase 1 pages show only the latest day.
CREATE TABLE price_snapshot (
    product_id        TEXT NOT NULL,
    date              TEXT NOT NULL,
    price_cents       INTEGER CHECK (price_cents IS NULL OR price_cents > 0),
    in_stock          INTEGER NOT NULL CHECK (in_stock IN (0, 1)),
    hidden_until_cart INTEGER NOT NULL CHECK (hidden_until_cart IN (0, 1)),
    feed_row_hash     TEXT NOT NULL,
    PRIMARY KEY (product_id, date)
) STRICT;

-- Filled only in rating branch A (spec §5.4).
CREATE TABLE rating_snapshot (
    product_id TEXT NOT NULL,
    date       TEXT NOT NULL,
    avg        REAL,
    count      INTEGER,
    PRIMARY KEY (product_id, date)
) STRICT;

CREATE TABLE pending_labels (
    product_id TEXT PRIMARY KEY,
    queued_on  TEXT NOT NULL
) STRICT;

CREATE TABLE ingest_run (
    date          TEXT PRIMARY KEY,
    fetched_at    INTEGER NOT NULL,
    source        TEXT NOT NULL,
    feed_sha256   TEXT NOT NULL,
    rows_total    INTEGER NOT NULL,
    rows_rejected INTEGER NOT NULL,
    rows_in_scope INTEGER NOT NULL,
    new_products  INTEGER NOT NULL
) STRICT;

-- edge-api (spec §8). No personal data: no IP, no cookies.
CREATE TABLE click_event (
    id           INTEGER PRIMARY KEY,
    product_id   TEXT NOT NULL,
    preset       TEXT NOT NULL,
    page         TEXT NOT NULL,
    ts           INTEGER NOT NULL,
    session_hash TEXT NOT NULL
) STRICT;

CREATE TABLE error_report (
    id         INTEGER PRIMARY KEY,
    product_id TEXT,
    field      TEXT NOT NULL,
    text       TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    page_url   TEXT NOT NULL
) STRICT;
