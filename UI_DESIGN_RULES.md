# UI Design Rules for Production Desktop-Style React Apps

Use this file as a design instruction document for coding agents building the UI.

## Goal

Build a professional, dense, native-tool-style interface. This is not a marketing website, landing page, SaaS dashboard, or mobile-first consumer app.

The UI should feel closer to:

- TradingView
- Microsoft Word
- Docker Desktop
- Autopsy
- Visual Studio Code
- Linear/Notion only where useful, but more compact
- Professional forensic, developer, analysis, or productivity software

The design should prioritize information density, speed, hierarchy, and workflow efficiency.

## Core Design Principles

### 1. Dense native software layout

Use compact spacing by default.

Avoid:

- Large cards
- Huge rounded panels
- Excessive padding
- Large empty hero sections
- Marketing-style layouts
- Oversized icons
- Big gradient backgrounds
- Centered single-column website layouts

Prefer:

- Toolbars
- Panels
- Split views
- Sidebars
- Tables
- Tabs
- Status bars
- Compact menus
- Tree views
- Inspector/details panes
- Command bars
- Docked layouts

Good desktop apps fit many controls and data views on screen without feeling cluttered.

## Spacing Rules

Use tight spacing.

Recommended defaults:

```ts
const spacing = {
  page: "p-0",
  panel: "p-2",
  section: "p-2",
  toolbar: "px-2 py-1",
  row: "px-2 py-1",
  input: "h-8",
  button: "h-8 px-2",
  iconButton: "h-7 w-7",
};
```

Avoid using `p-6`, `p-8`, `gap-8`, `space-y-8`, or large card spacing unless there is a very specific reason.

Default gaps should usually be:

```text
gap-1
gap-2
gap-3
```

Rarely use larger than `gap-4`.

## Layout Structure

Prefer app-shell layouts:

```text
AppShell
├─ TopMenuBar
├─ MainToolbar
├─ Workspace
│  ├─ LeftSidebar
│  ├─ MainContent
│  └─ RightInspector
└─ StatusBar
```

Use resizable panels where useful.

Recommended structure:

```text
src/
├─ app/
│  ├─ App.tsx
│  ├─ router.tsx
│  └─ providers.tsx
├─ components/
│  ├─ ui/                 # shadcn components
│  ├─ app-shell/
│  ├─ toolbar/
│  ├─ sidebar/
│  ├─ panels/
│  ├─ tables/
│  └─ icons/
├─ features/
│  └─ feature-name/
│     ├─ components/
│     ├─ hooks/
│     ├─ lib/
│     ├─ types.ts
│     └─ index.ts
├─ lib/
├─ hooks/
├─ styles/
└─ types/
```

Do not put the entire UI in one file.

## Component Library Rules

Hard rule:

- Always use a component from shadcn/ui or DaisyUI for UI primitives.
- Never create custom UI primitives unless the user specifically asks for a custom component.
- This applies to buttons, sidebars, menus, dialogs, tables, inputs, dropdowns, tabs, cards, tooltips, scroll areas, sheets, drawers, command palettes, forms, alerts, badges, and similar interface building blocks.
- If a needed shadcn/ui component is not installed, add it with the shadcn CLI before using it.
- Use commands like:

```powershell
bunx --bun shadcn@latest add menubar
bunx --bun shadcn@latest add sidebar
```

- Custom project components are allowed only as composition wrappers for app-specific behavior, pages, layouts, or feature views, and they must be built from shadcn/ui or DaisyUI components.
- Keep all UI compact and small by default.

Use **shadcn/ui** as the primary component foundation.

Use shadcn for:

- Button
- Input
- Select
- Dialog
- Dropdown Menu
- Context Menu
- Tabs
- Tooltip
- Scroll Area
- Separator
- Sheet only when needed
- Resizable panels
- Table primitives

Use **DaisyUI** selectively for utility-style components when they improve speed and visual consistency.

DaisyUI may be used for:

- Badge
- Tooltip-like labels
- Loading indicators
- Progress bars
- Compact alerts
- Theme utilities
- Small stat indicators

Do not let DaisyUI make the app look like a website. Avoid large DaisyUI cards, hero sections, and oversized components.

## Visual Style

### General

Use a professional, compact, neutral interface.

Prefer:

- Subtle borders
- Flat surfaces
- Small shadows only when necessary
- Clear selected states
- Compact controls
- Muted backgrounds
- High contrast for active elements

Avoid:

- Heavy shadows
- Large border radius everywhere
- Glassmorphism
- Neon gradients
- Overly playful styling
- Mobile app spacing

### Border radius

Use small radius.

```text
rounded-sm
rounded
```

Avoid excessive use of:

```text
rounded-xl
rounded-2xl
rounded-3xl
```

### Borders

Use borders to separate functional regions.

```tsx
<div className="border-b" />
<div className="border-r" />
<div className="border" />
```

Prefer borders over large spacing.

## Typography

Use small, readable text sizes.

Recommended:

```text
text-xs     metadata, labels, status bar
text-sm     normal UI text
text-base   important headings only
```

Avoid large page headings unless the screen truly needs one.

For dense tools, prefer section headers like:

```tsx
<div className="h-8 border-b px-2 flex items-center text-xs font-medium uppercase tracking-wide text-muted-foreground">
  Evidence Items
</div>
```

## Buttons

Buttons should be compact.

Use:

```tsx
<Button size="sm" className="h-8 px-2 text-xs" />
```

Icon buttons:

```tsx
<Button variant="ghost" size="icon" className="h-7 w-7" />
```

Avoid large primary CTA buttons unless it is a rare workflow entry point.

## Tables and Data Views

Professional tool software should use tables often.

Tables should be compact:

```tsx
<TableRow className="h-8">
<TableCell className="px-2 py-1 text-xs">
```

Support:

- Sorting
- Filtering
- Column resizing where needed
- Row selection
- Context menus
- Keyboard navigation when practical
- Empty states that are small and useful

Avoid large empty-state illustrations.

## Sidebars

Sidebars should be functional, not decorative.

Use sidebars for:

- Navigation
- File trees
- Case/project trees
- Filters
- Collections
- Sources
- Layers
- Tool categories

Sidebar items should be compact:

```tsx
<button className="h-7 w-full px-2 flex items-center gap-2 text-xs hover:bg-accent">
```

## Toolbars

Use toolbars instead of large page headers.

Toolbar rules:

- Height: `h-9` or `h-10`
- Use small buttons
- Group related actions with separators
- Put global search or command actions near the top
- Use icons with short labels only when helpful

Example:

```tsx
<header className="h-10 border-b flex items-center gap-1 px-2 bg-background">
  <Button size="sm" variant="ghost" className="h-8 px-2 text-xs">
    Open
  </Button>
  <Button size="sm" variant="ghost" className="h-8 px-2 text-xs">
    Export
  </Button>
  <Separator orientation="vertical" className="mx-1 h-5" />
  <Input className="h-8 w-64 text-xs" placeholder="Search..." />
</header>
```

## Panels

Use panels for workspace regions.

Panel header:

```tsx
<div className="h-8 border-b px-2 flex items-center justify-between text-xs font-medium">
  <span>Properties</span>
</div>
```

Panel body:

```tsx
<div className="p-2 text-sm">
```

Do not wrap everything in large cards.

## Forms

Forms should be compact and aligned.

Prefer label + control rows:

```tsx
<div className="grid grid-cols-[120px_1fr] items-center gap-2 py-1">
  <Label className="text-xs text-muted-foreground">Case Name</Label>
  <Input className="h-8 text-xs" />
</div>
```

Avoid huge vertical forms with excessive whitespace.

## Menus and Context Actions

Use context menus for advanced or secondary actions.

Good examples:

- Right-click table row
- Right-click tree item
- Dropdown on toolbar
- Kebab menu for item actions

Use shadcn ContextMenu and DropdownMenu.

## Theme

Support dark mode and light mode.

Default dark theme should feel like professional software, not a gaming UI.

Use CSS variables through shadcn theme tokens:

```css
--background
--foreground
--muted
--muted-foreground
--border
--accent
--accent-foreground
```

Do not hardcode many colors directly into components.

## Icons

Use `lucide-react` unless the project already uses another icon set.

Rules:

- Default size: `h-4 w-4`
- Small icon buttons: `h-3.5 w-3.5`
- Avoid oversized decorative icons
- Icons must support actions or recognition

## Responsive Behavior

This is desktop-first software.

Prioritize desktop layouts:

- 1280px and wider
- Split panes
- Dense tables
- Sidebars
- Multi-panel workflows

For smaller screens, collapse secondary panels or use tabs, but do not design the primary experience like a mobile website.

## React Best Practices

Use modern React patterns.

Required:

- Functional components
- TypeScript
- Composition over giant components
- Feature-based organization
- Hooks for reusable behavior
- Shared UI primitives
- Strong prop types
- Clear state ownership

Avoid:

- One huge `App.tsx`
- One-file prototypes
- Inline business logic everywhere
- Duplicated components
- Deep prop drilling when context or state libraries are better
- Styling everything with random one-off classes

## Component Size Rules

As a guideline:

- UI primitive: 50-150 lines
- Feature component: 100-250 lines
- Complex screen: split into child components before it gets too large

If a file grows beyond 300 lines, consider splitting it.

## State Management

Keep state close to where it is used.

Use:

- `useState` for local UI state
- `useReducer` for complex local state
- React Context for app-level settings or layout state
- Zustand/Jotai/TanStack Query only when justified by the project

Do not add heavy global state management unnecessarily.

## Example Screen Pattern

A good screen should look like this:

```text
Top toolbar: compact actions and search
Left sidebar: tree/filter/navigation
Center: table, editor, canvas, reader, or workspace
Right panel: selected item details/properties
Bottom status bar: progress, counts, background task state
```

Not like this:

```text
Huge page title
Large card
Large card
Large card
Lots of padding
Marketing-style empty space
```

## Example Component Style

```tsx
export function WorkspaceHeader() {
  return (
    <header className="h-10 border-b bg-background px-2 flex items-center justify-between">
      <div className="flex items-center gap-1">
        <Button variant="ghost" size="sm" className="h-8 px-2 text-xs">
          New
        </Button>
        <Button variant="ghost" size="sm" className="h-8 px-2 text-xs">
          Import
        </Button>
        <Separator orientation="vertical" className="mx-1 h-5" />
        <Input className="h-8 w-72 text-xs" placeholder="Search workspace" />
      </div>

      <div className="flex items-center gap-1">
        <span className="badge badge-sm badge-neutral">Ready</span>
      </div>
    </header>
  );
}
```

## Final Agent Instruction

When generating UI code, always ask:

1. Does this look like professional desktop software?
2. Is the layout dense and useful?
3. Did I avoid large website-style cards?
4. Did I split the UI into organized components?
5. Am I using shadcn as the main component system?
6. Am I using DaisyUI only where it improves compact utility UI?
7. Would this feel natural in apps like Docker Desktop, TradingView, Microsoft Word, VS Code, or Autopsy?

If the answer is no, refactor before finishing.
