# Vitrina

Рациональная витрина для iHerb: добавки сравниваются по цене того, что
действительно в капсуле (например, цена за 100 мг элементарного магния), а
не по цене банки. Фаза 1 — статический англоязычный сайт на одну категорию.

Требования: [`docs/spec-phase-1.md`](docs/spec-phase-1.md). Решения:
[`docs/adr/`](docs/adr/). Открытые вопросы:
[`docs/open-questions.md`](docs/open-questions.md). Задачи:
[`docs/backlog.md`](docs/backlog.md). Сравнение направлений дизайна:
[`design/napravleniya.html`](design/napravleniya.html).

## Как устроено

```
crates/
  catalog-core/   единицы, справочник, этикетки, доза, P_unit, R_b, пресеты; без IO, собирается под wasm
  catalog-store/  SQLite: схема, миграции, репозитории, снимки
  feed-ingest/    фид сети → день наблюдений; никогда не обращается к iherb.com
  site-gen/       статический сайт из снимка; проверки перед публикацией
  edge-api/       POST /api/click, POST /api/report
  ops-cli/        команда `vitrina`
data/reference/   вещества, формы (с формулами и источниками), категории
data/labels/      этикетки по товарам, вводятся вручную
data/fixtures/    контрольный набор A–F, синтетический фид и этикетки
site/             шаблоны askama, строки en.toml, CSS, JS
deploy/           systemd, окружение, уведомления, бэкап, мониторинг
```

## Быстрый старт на фикстурах

Нужен Rust stable (≥ 1.88). Данные фикстур синтетические.

```sh
cargo build --release -p ops-cli -p edge-api
V=target/release/vitrina
$V --config config/fixture.toml ingest --feed-file data/fixtures/feed/sample_feed.csv --date 2026-09-26
$V --config config/fixture.toml build
$V --config config/fixture.toml verify
$V --config config/fixture.toml publish --local
python3 tools/serve-site.py out/current/site 8080   # http://127.0.0.1:8080/c/magnesium
```

## Ежедневная задача

```sh
vitrina ingest && vitrina build && vitrina verify && vitrina publish
```

| Команда | Что делает | Код при отказе |
| --- | --- | --- |
| `ingest` | фид (`VITRINA_FEED_URL` или `--feed-file`) → цены дня; импорт справочника и этикеток; снимок `var/snapshots/vitrina-<date>.db` | 2 — фид недоступен или непригоден, снимок не пишется |
| `import-labels` | проверка и импорт `data/reference/` и `data/labels/` | 1 |
| `build` | сайт в `out/<date>/site/` и `manifest.json` из снимка | 1 |
| `verify` | падение ≤ 20%, контрольный набор, ссылки, раскрытие у кнопок, бюджеты | 3 — публиковать нельзя |
| `publish` | `VITRINA_UPLOAD_CMD`, затем переключение `out/current` | 3 без `verify`, 4 — загрузка не удалась |
| `status` | последний фид, очередь этикеток, счётчики событий | 1 |

Настройки — `config/vitrina.toml` (ветка рейтинга, локаль, пути), секреты —
переменные окружения (`deploy/vitrina.env.example`).

## Проверки

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build -p catalog-core --target wasm32-unknown-unknown
```

Снимки HTML (`insta`) — `crates/site-gen/tests/snapshots/`; после
намеренного изменения шаблона: `INSTA_UPDATE=always cargo test -p site-gen`
и просмотр диффа.
