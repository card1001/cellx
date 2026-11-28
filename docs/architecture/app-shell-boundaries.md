# Application shell boundaries

CellX's desktop UI is being decomposed incrementally. The current `App.tsx` remains the composition root while feature modules are extracted behind stable interfaces.

## Current boundaries

- `api.ts`: typed Tauri command boundary. UI components should not call Rust commands directly.
- `components/*`: feature-specific presentation and dialogs.
- `grid/*`: grid model, rendering, and data sources.
- `appTypes.ts`: navigation tab identity and in-progress cell-edit session contracts.
- `i18n/*`: locale selection and message formatting.

## Refactoring sequence

1. Extract stable shared types and pure state helpers without changing behavior.
2. Move dialog state and dialog orchestration behind a dedicated hook.
3. Move workbook commands and persistence flows behind feature hooks.
4. Split data-sheet, formula-editing, and worksheet interactions into feature controllers.
5. Keep `App.tsx` as a thin composition root and verify each extraction with existing UI tests.

## Constraints

- Do not duplicate business state across hooks.
- Keep Tauri invocation in the typed API layer.
- Preserve keyboard navigation, selection identity, and edit-session semantics.
- Prefer small refactors with behavior-preserving tests over broad file moves.
- Do not claim a feature is modularized until its state, effects, and command ownership have moved together.
