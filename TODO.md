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

## Architecture / Code Quality

- [x] **Domain Purity**: Удалена зависимость `ra_ap_syntax` из `rem-domain` — deref-rewriting вынесен в `SyntaxRewritePort`/`SyntaxRewriteAdapter`.
- [x] **Ownership Oracle — Move Analysis**: AST-based `refine_ownership` в адаптере определяет Owned/MutRef/SharedRef по реальным использованию. Оракул корректно обрабатывает move + used_after → SharedRef.

## Infrastructure / DevEx

- [ ] **Proc-macro support**: Включение и настройка proc-macro server для корректного анализа кода с макросами.

## Integration Tests

- [ ] Тесты на извлечение из асинхронных блоков.
- [ ] Тесты на извлечение с глубокой вложенностью лайфтаймов.
- [ ] Тесты на некорректный синтаксис (Graceful failure).
- [x] **Generic Extraction Test**: Валидация корректности переноса Trait Bounds (например, `T: MyTrait`).
