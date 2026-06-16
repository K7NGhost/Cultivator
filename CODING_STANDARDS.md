# Coding Standards

You are working in a production codebase. Optimize for maintainable, scalable, readable, and well-organized code. Do not optimize for fitting everything into one file. Optimize for long-term code quality.

## Core Principles

- Write production-quality code, not prototype code.
- Prefer small, focused files with clear responsibilities.
- Keep business logic separate from framework glue when possible.
- Avoid unnecessary abstraction, but introduce structure when it improves clarity or reuse.
- Prefer explicit, readable code over clever code.
- Maintain consistency with the existing project style, naming, and architecture.
- Avoid duplicating logic that already exists elsewhere.
- Do not make unrelated changes while completing a task.

## Before Writing Code

Before implementing changes:

1. Inspect the existing project structure.
2. Identify the framework, language, tooling, and package manager being used.
3. Follow existing conventions unless they are clearly poor or inconsistent.
4. Reuse existing utilities, services, hooks, types, and modules when available.
5. Check whether linting, formatting, tests, or type rules exist and respect them.
6. Keep changes scoped to the requested behavior.

## File Organization

Use a structured codebase layout. Prefer grouping code by responsibility and feature area.

Good default structure:

```txt
src/
  app/
  features/
    feature-name/
      hooks/
      services/
      types/
      utils/
      index.ts
  hooks/
  lib/
  services/
  stores/
  types/
  utils/
```

Prefer feature-based organization for larger applications:

```txt
src/features/books/
  hooks/useBooks.ts
  services/booksApi.ts
  types/book.types.ts
  utils/bookFilters.ts
  index.ts
```

Use shared folders only for code reused across multiple features:

```txt
src/hooks/useDebounce.ts
src/lib/apiClient.ts
src/utils/formatDate.ts
```

Do not create a shared abstraction just because something might be reused later. Extract only when it is actually reused or clearly improves readability.

## Module Rules

Each module should have one clear purpose.

Good:

```txt
booksApi.ts
book.types.ts
bookFilters.ts
useBooks.ts
```

Bad:

```txt
helpers.ts
stuff.ts
data.ts
everything.ts
```

Module guidelines:

- Keep modules small and readable.
- Use clear inputs and outputs.
- Move complex logic into utilities or services.
- Avoid deeply nested control flow when it can be simplified.
- Keep side effects contained and predictable.
- Avoid unnecessary local state.
- Avoid hidden dependencies between unrelated modules.

## TypeScript Rules

Use TypeScript properly.

- Avoid `any` unless absolutely necessary.
- Prefer explicit interfaces or types for API responses, domain models, function inputs, and service results.
- Put feature-specific types inside the feature folder.
- Put globally shared types in `src/types`.
- Use discriminated unions when modeling multiple states.
- Do not silence TypeScript errors without a clear reason.
- Avoid type assertions unless the boundary is validated or otherwise guaranteed.

Example:

```ts
type LoadState<T> =
  | { status: "idle" }
  | { status: "loading" }
  | { status: "success"; data: T }
  | { status: "error"; message: string };
```

## Rust Rules

Use Rust properly when working in Rust crates, Tauri commands, backend services, CLI tools, or shared native modules.

- Prefer clear ownership and borrowing over unnecessary cloning.
- Use `.clone()` only when the ownership cost is intentional and acceptable.
- Avoid `unwrap()` and `expect()` in production code unless the value is guaranteed by a clear invariant.
- Use `Result<T, E>` for recoverable failures.
- Use `Option<T>` when a value may be absent.
- Model domain states with structs and enums instead of loose strings, booleans, or integer flags.
- Keep functions small and focused.
- Prefer readable code over dense iterator chains when a loop is clearer.
- Keep public APIs narrow and intentional.
- Document public behavior, errors, and important invariants.
- Avoid global mutable state unless there is a clear architectural reason.
- Keep `unsafe` code rare, isolated, and documented.

Good:

```rust
pub enum ImportStatus {
    Pending,
    Running,
    Complete,
    Failed(String),
}

pub fn parse_limit(value: Option<&str>) -> Result<Option<u32>, ParseIntError> {
    value.map(str::parse).transpose()
}
```

Bad:

```rust
let status = "failed";
let limit = input.unwrap().parse::<u32>().unwrap();
```

Rust error handling guidelines:

- Return errors instead of panicking for invalid input, failed I/O, failed parsing, and external service failures.
- Use `?` to propagate errors when the caller can handle them.
- Convert error types deliberately using `From`, `thiserror`, `anyhow`, or project-local error types, depending on the existing project style.
- Include enough context for errors that cross process or API boundaries.
- Do not discard errors with `_`, `.ok()`, or empty `match` arms unless ignoring the error is intentional and documented.

Rust organization guidelines:

- Keep crate modules focused around responsibilities.
- Put shared domain types in a clear module instead of duplicating structs across features.
- Keep Tauri command handlers thin; move business logic into testable Rust modules.
- Keep serialization types explicit when crossing the frontend/backend boundary.
- Avoid mixing parsing, validation, file I/O, and command wiring in one function.

Rust testing guidelines:

- Add unit tests for parsing, transformations, calculations, and error handling.
- Add integration tests for file I/O, command behavior, persistence, and public workflows.
- Keep tests deterministic.
- Avoid tests that depend on the developer machine's local paths or environment unless explicitly configured.
- Run `cargo fmt` before finishing Rust changes.
- Run `cargo clippy` and treat warnings as issues unless the project has a documented exception.
- Run `cargo test` when Rust behavior changes.

## State Management

Choose the simplest state solution that fits the problem.

Use local state for:

- Temporary values
- Form inputs
- Local workflow state
- Values that do not need to be shared

Use shared state for:

- Cross-feature data
- App-wide settings
- State with many actions or derived values
- Values needed by many unrelated modules

Use server-state libraries when handling:

- Remote data fetching
- Caching
- Refetching
- Request status
- Mutations

Do not put everything into global state.

## Data Fetching and Services

Separate data access from consumers.

Good:

```txt
src/features/books/services/booksApi.ts
src/features/books/hooks/useBooks.ts
src/features/books/types/book.types.ts
```

Bad:

```ts
async function loadEverything() {
  // fetching, parsing, validation, transformation, and orchestration all together
}
```

Service guidelines:

- Keep API calls in service files.
- Keep transformation logic separate when it grows.
- Handle errors intentionally.
- Avoid hardcoding repeated endpoint strings across the codebase.
- Validate or type API responses when possible.

Example:

```ts
export async function getBooks(): Promise<Book[]> {
  const response = await fetch("/api/books");

  if (!response.ok) {
    throw new Error("Failed to load books");
  }

  return response.json() as Promise<Book[]>;
}
```

## Error Handling

Production code must handle errors.

- Avoid silent failures.
- Avoid `console.log` as the only error handling.
- Return, throw, or surface errors intentionally.
- Include useful context at system boundaries.
- Do not assume external data is always valid.
- Avoid swallowing exceptions unless there is a clear fallback path.

## Forms and Validation

For simple forms, controlled state is acceptable.

For larger forms, prefer a form library already used in the project.

Form and input rules:

- Validate user input.
- Avoid duplicating validation logic across files.
- Keep schema and submission logic organized.
- Keep parsing, validation, and persistence boundaries clear.

## Performance

Do not prematurely optimize, but avoid obvious performance problems.

- Avoid unnecessary re-renders or recomputation when they matter.
- Use memoization only when it provides real value.
- Virtualize very large lists.
- Debounce expensive search or filtering.
- Avoid doing expensive work directly inside render or hot paths.
- Split large modules into smaller focused modules.
- Lazy-load heavy routes or modules when appropriate.

## Testing

When tests are part of the project, add or update tests for meaningful changes.

Test:

- Business logic
- Utility functions
- Complex hooks
- Data transformations
- Error handling
- Persistence and I/O boundaries
- Critical workflows

Do not write fragile tests that depend heavily on implementation details.

## Naming Conventions

Use clear names.

- Functions: `camelCase`
- Variables: `camelCase`
- Types/interfaces: `PascalCase`
- Constants: follow the existing project convention
- Files: match the exported function, type, module, or purpose

Examples:

```txt
useBooks.ts
booksApi.ts
book.types.ts
formatReadingTime.ts
calculateGoalProgress.ts
```

Avoid vague names:

```txt
helpers.ts
stuff.ts
data.ts
newFile.ts
module2.ts
```

## Imports and Exports

- Keep imports clean and organized.
- Avoid circular dependencies.
- Use barrel exports only when they improve ergonomics and do not create confusing dependency graphs.
- Prefer named exports for functions, hooks, utilities, and types.
- Avoid default exports unless the project already uses them consistently.

Example feature barrel:

```ts
export { useBooks } from "./hooks/useBooks";
export { getBooks } from "./services/booksApi";
export type { Book } from "./types/book.types";
```

## Security

Production code should avoid unsafe behavior.

- Never expose secrets in client code.
- Do not trust client-side validation alone.
- Sanitize or safely handle user-generated content.
- Handle auth and permissions intentionally.
- Do not log sensitive data.
- Treat filesystem, plugin, script, and process execution boundaries as high-risk.

## Code Review Checklist

Before finishing, verify:

- The code is split into appropriate files.
- Modules are not overly large.
- Business logic is not tangled with framework glue.
- Types are clear and useful.
- Errors are handled intentionally.
- Existing project conventions are followed.
- No unnecessary dependencies were added.
- No secrets or sensitive values are hardcoded.
- No unrelated files were changed.
- The code would be understandable to another developer.

## Required Agent Behavior

When implementing a task:

1. First understand the existing architecture.
2. Follow an organized file structure.
3. Create separate files for services, types, hooks, and utilities when appropriate.
4. Keep entrypoint files focused on composition, not all implementation details.
5. Avoid dumping everything into one file.
6. Prefer reusable and testable modules.
7. Explain the files changed and why.
8. Mention any assumptions made.
9. Mention any follow-up improvements that were intentionally left out.

## Final Output Expectations

When you complete a coding task, summarize your work like this:

```txt
Implemented:
- Added feature-level structure for [feature name]
- Moved data access into [service file]
- Added types in [types file]
- Added validation/error handling for [workflow]

Files changed:
- src/features/example/hooks/useExample.ts
- src/features/example/services/exampleApi.ts
- src/features/example/types/example.types.ts

Notes:
- Followed existing project conventions.
- No secrets or environment-specific values were hardcoded.
```

Production-quality code means organized code, not just working code.
