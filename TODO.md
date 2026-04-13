# REM 3.0 TODO

## High Priority

- [x] **Deref Repair**: Добавить автоматическую вставку операторов разыменования (`*`) для параметров-ссылок в теле извлеченной функции.
- [x] **Smarter Lifetime Repair**: Реализовать парсинг конкретных имен аргументов из ошибок E0106 для таргетного добавления лайфтаймов.
- [x] **Control Flow Refinement**: Реализовать проверку того, является ли `break`/`continue` выходом за пределы выделенного фрагмента (Loop Target Resolution).
- [x] **Trait Bound Handling**: Автоматический проброс Trait Bounds при использовании Generic-параметров из внешнего контекста.

## Features

- [ ] **Method Extraction**: Поддержка извлечения методов в блоки `impl`.
- [ ] **Visibility Control**: Автоматическое определение необходимости `pub` или `pub(crate)` для извлеченной функции.
- [ ] **Doc Comments**: Перенос или генерация базовых doc-комментариев.

## Infrastructure / DevEx

- [ ] **Proc-macro support**: Включение и настройка proc-macro server для корректного анализа кода с макросами.

## Очередь решения (Current Stage)

1. **Infrastructure Fix**: Решена проблема "Attached DB" паники через `ra_ap_hir::attach_db`. ✅
2. **Generic Extraction**: Реализована логика `as_type_param` для идентификации параметров и `trait_bounds` для сбора ограничений. ✅
3. **Ownership Correctness**: Исправлено определение владения — `Local::is_ref(db)` для различения by-value и by-ref параметров. ✅
4. **Call-site Generics**: Разделение generic-параметров для сигнатуры (с bounds) и call-site (только имена). ✅
5. **Final Check**: Валидация через `test_generic_extract`. ✅

## Integration Tests

- [ ] Тесты на извлечение из асинхронных блоков.
- [ ] Тесты на извлечение с глубокой вложенностью лайфтаймов.
- [ ] Тесты на некорректный синтаксис (Graceful failure).
- [x] **Generic Extraction Test**: Валидация корректности переноса Trait Bounds (например, `T: MyTrait`).
