# Правила для агентов

Прочитать `docs/spec-phase-1.md` целиком перед задачей. Кратко:

- Инварианты И1–И10 (§1) сильнее любой задачи. Особенно: программа никогда не
  запрашивает iherb.com (данные — только фид сети и этикетки); комиссия не
  входит ни в одну формулу; кнопка покупки — только трекинговая ссылка сети с
  раскрытием рядом; неизвестное показывается как неизвестное, не нулём.
- Формулы живут только в `crates/catalog-core`. SQL — только в
  `crates/catalog-store`. Формат фида — только в `crates/feed-ingest`.
- Новое допущение — ADR в `docs/adr/` со статусом `assumed`; отступление от
  спецификации — `proposed`. Неизвестная величина — пункт в
  `docs/open-questions.md`, не догадка.
- `data/reference/`: у каждой записи `source_url`, у PR — второй проверяющий.
  `data/labels/`: `verified_by` и `verified_at` обязательны.
- Строки интерфейса — только в `site/i18n/en.toml`; шаблоны обращаются по
  ключам. Названия товаров и состав не переводятся.
- Интерфейс: `docs/guidelines/interface.md` (принципы и чек-лист PR),
  `docs/guidelines/art-direction.md` (выразительность, закон направления
  «лучше — вверх и вправо», токены); прототипы в `design/prototypes/`.
- Перед PR: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets
  -- -D warnings`, `cargo test --workspace`, `cargo build -p catalog-core
  --target wasm32-unknown-unknown`.
- Коммиты на английском, в настоящем времени, с крейтом:
  `catalog-core: add IU conversion for vitamin D`.
