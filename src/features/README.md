# Frontend feature organization

Group code by the domain it belongs to: cases, datasources, evidence, files,
artifacts, media, plugins, search, and workspace. Keep a change inside its owning
feature whenever possible.

```text
src/
  app/                         App shell, navigation, and startup UI
  components/                  Shared UI and theme components
  hooks/                       Hooks used across features
  lib/                         Shared infrastructure utilities
  features/
    search/
      pages/SearchPage.tsx     Page composition and workflow state
      components/              Feature-specific UI
      model/types.ts           Domain and command contracts
      services/                Tauri commands and persistence
      utils/                   Formatting, reports, and preferences
      index.ts                 Public domain API
```

Create folders only when needed. Features can also own `providers/` for React
context, `events/` for notifications, and `workers/` for background computation.
Keep domain types in `model/` so services and other features can use them without
depending on a UI component or provider implementation.

## Imports and ownership

- Within a feature, import the specific module using a relative path. Do not
  import your own `index.ts`; that can introduce circular dependencies.
- Across features, import domain APIs from `@/features/<feature>`. Use explicit
  exports in `index.ts`, and expose only APIs that consumers need.
- Import pages and integration components directly from their `pages/` or
  `components/` entry points. Keep them out of domain API barrels so importing a
  repository does not also load a page and its feature dependencies. The small
  workspace feature exposes its placeholder page directly.
- Keep application composition in `src/app` and reusable UI primitives in
  `src/components`. Move code into shared folders when multiple features use it.
- Keep Tauri command names and argument translation inside feature services.
  Pages should handle interaction and compose feature components; extract
  substantial independent rendering and domain calculations into focused
  modules.

For example, search owns its backend commands, search result contracts, preview
list, match list, settings, and report formatting. Files owns file tagging and
workspace-readiness events. Cases owns the case database lifecycle used by other
features through its public API.

Tests may import individual feature modules directly to exercise domain behavior
without loading unrelated React or Tauri modules. Run `bun test` and
`npm run build` after changing module boundaries or moving worker files.
