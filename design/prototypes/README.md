# Прототипы

- `magnesium-ranking.html` — прототип страницы рейтинга. Собран, не
  редактировать руками: источник — `magnesium-ranking.src.html`, данные —
  `cases/fixture-a.json` и `cases/fixture-b.json` (ADR 0020).
- `inside-the-capsule.html` — главная-история.

Все числа прототипа рейтинга — выход генератора сайта. Скрипт прототипа
раскладывает и формулирует их, но не ранжирует и не считает баллы.

```sh
# после изменения catalog-core или site-gen: обновить случаи и проверить их
UPDATE_PROTOTYPE_CASES=1 cargo test -p site-gen --test evaluation
cargo test -p site-gen --test evaluation

# пересобрать прототип
python3 tools/build-prototype.py

# браузерная проверка всех случаев (0 и 1 товар, ничьи, пустые пресеты)
cd design/prototypes/tests && npm ci && node states.mjs --shots /tmp/shots
```

Требования к виду и поведению — `docs/guidelines/art-direction.md` и
`docs/guidelines/interface.md`.
