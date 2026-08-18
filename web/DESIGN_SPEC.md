# NeoMind Frontend Design & Component Specification

This document defines the visual design system, component usage rules, and coding conventions for the NeoMind web frontend. All frontend changes must follow these standards.

---

## 1. Color System

### Design Token Architecture

All colors are defined as OKLCH CSS variables in `src/index.css` (`:root` for light, `.dark` for dark mode) and mapped in `tailwind.config.js`.

**NEVER use hardcoded Tailwind palette colors** (`bg-blue-500`, `text-green-600`, `bg-orange-100`, etc.).

### Semantic Color Tokens

| Purpose | Text Class | Background Class | CSS Variable |
|---------|-----------|-----------------|--------------|
| Success | `text-success` | `bg-success` / `bg-success-light` | `--color-success` |
| Warning | `text-warning` | `bg-warning` / `bg-warning-light` | `--color-warning` |
| Error | `text-error` | `bg-error` / `bg-error-light` | `--color-error` |
| Info | `text-info` | `bg-info` / `bg-info-light` | `--color-info` |

### Accent Category Tokens

| Category | Default | Light Background |
|----------|---------|-----------------|
| Purple | `text-accent-purple` | `bg-accent-purple-light` |
| Orange | `text-accent-orange` | `bg-accent-orange-light` |
| Cyan | `text-accent-cyan` | `bg-accent-cyan-light` |
| Emerald | `text-accent-emerald` | `bg-accent-emerald-light` |
| Indigo | `text-accent-indigo` | `bg-accent-indigo-light` |

### Chart Colors

Six chart-compatible tokens defined in `index.css`: `--chart-1` through `--chart-6`.

### Text on Colored Backgrounds

When placing text/icons on colored backgrounds (buttons, badges, status pills), use `text-primary-foreground` — it resolves to white in both light and dark themes.

```tsx
// Good
<span className="bg-success text-primary-foreground">Active</span>
// Bad
<span className="bg-success text-white">Active</span>
```

### Exceptions: `text-white` / `bg-black` Are Allowed ONLY For

- **Media overlays**: Video player controls, image caption overlays (these sit on top of photos/videos where only pure white/black provides contrast)
- **Modal backdrops**: `bg-black/50`, `bg-black/80` for dialog overlays
- **Micro-contrast backgrounds**: `bg-black/[0.02]` / `bg-white/[0.02]` in builder layouts

### Opacity Limitation (Tailwind v3)

CSS variables defined as plain `oklch()` values do NOT support Tailwind's `/` opacity modifier. These silently fail:

```tsx
// BROKEN - silently produces no opacity
<div className="bg-primary/10" />
<div className="bg-muted-foreground/20" />
```

**Workarounds:**
- Use pre-defined alpha variables: `bg-muted-20`, `bg-muted-30`, `bg-muted-50`, `bg-bg-50`, `bg-bg-70`, etc.
- Use inline styles: `style={{ backgroundColor: 'oklch(0.18 0.02 270 / 10%)' }}`
- Use `bg-success-light` / `bg-error-light` etc. (pre-defined at 8-10% opacity)

### Chat-Specific Colors

| Token | Purpose |
|-------|---------|
| `--msg-user-bg` / `--msg-user-text` | User message bubbles |
| `--msg-ai-bg` / `--msg-ai-text` | AI message bubbles |
| `--msg-system-bg` / `--msg-system-text` | System messages |
| `--tool-bg` / `--tool-border` / `--tool-header-bg` | Tool call display |
| `--thinking-bg` / `--thinking-border` / `--thinking-text` | Thinking state display |

### Hover State Tokens

| Token | Usage |
|-------|-------|
| `--primary-hover` | Primary button hover |
| `--secondary-hover` | Secondary button hover |
| `--destructive-hover` | Destructive button hover |
| `--card-hover-bg` | Card hover background |
| `--glass-border-hover` | Glass border hover state |

### Semantic Color Tints (bg/border overlays)

For tinted backgrounds (subtle color washes) and matching borders, use the `*-light` token alias. These tokens bake the correct alpha in at the CSS-variable level, so Tailwind's `/opacity` modifier is never needed (and silently fails on CSS-var colors — see "Opacity Limitation" above).

| Token | Tailwind class | Use for |
|-------|----------------|---------|
| `--primary-bg` | `bg-primary-light` / `border-primary` | Primary accent wash; primary-tinted borders use full color |
| `--destructive-bg` | `bg-destructive-light` / `border-destructive-light` | Destructive error wash + border |
| `--color-success-bg` | `bg-success-light` / `border-success-light` | Success wash + border |
| `--color-warning-bg` | `bg-warning-light` / `border-warning-light` | Warning wash + border |
| `--color-error-bg` | `bg-error-light` / `border-error-light` | Error wash + border |
| `--color-info-bg` | `bg-info-light` / `border-info-light` | Info wash + border |

**Hard rule**: Never write `bg-{token}/N` for any CSS-variable color. Use the `*-light` alias instead.

### Overlay Tokens (semi-transparent masks)

For modal backdrops, loading overlays, image-label backgrounds — anywhere a darkening mask is needed:

| Token | Tailwind class | Light mode | Dark mode |
|-------|----------------|------------|-----------|
| `--overlay-light` | `bg-overlay-light` | `oklch(0 0 0 / 30%)` | `oklch(0 0 0 / 40%)` |
| `--overlay-medium` | `bg-overlay-medium` | `oklch(0 0 0 / 50%)` | `oklch(0 0 0 / 60%)` |
| `--overlay-heavy` | `bg-overlay-heavy` | `oklch(0 0 0 / 80%)` | `oklch(0 0 0 / 85%)` |

Mapping from legacy `bg-black/N`:

- `/5` `/10` `/20` `/30` → `bg-overlay-light`
- `/40` `/50` `/60` → `bg-overlay-medium`
- `/70` `/80` → `bg-overlay-heavy`

**Exception**: `bg-white/X` and `bg-black/X dark:bg-white/Y` glass-effect pairs are allowed to remain as-is (decorative glass highlights, not semantic masks).

---

### Spacing Scale

Use Tailwind's `gap-*` / `space-*` utilities. Defaults:

| Class | Px | Use case |
|-------|----|----------|
| `gap-1` | 4 | Tight grouping: icon ↔ label inside a button |
| `gap-2` | 8 | **Default** for inline rows, button groups |
| `gap-3` | 12 | Inside cards, form-field clusters |
| `gap-4` | 16 | Between sections in a panel |
| `gap-6` | 24 | Page-level section separation |

### Radius Scale

| Class | Use case |
|-------|----------|
| `rounded-sm` | Tags, badges |
| `rounded-md` | Inputs, small buttons (`size="sm"` / `"xs"`) |
| `rounded-lg` | Cards, dialogs (**preferred default for surfaces**) |
| `rounded-xl` | EmptyState containers, large icon containers |
| `rounded-2xl` | Only PageHeader gradient containers |
| `rounded-full` | Only avatars, status dots, purely circular controls |

**Hard rule**: EmptyState and its variants all use `rounded-xl`. Do not mix `rounded-2xl` / `rounded-full` across variants of the same component.

---

### Button Size Scale

`Button` (`@/components/ui/button`) ships six size variants. **Never override height with `className="h-7/h-8/h-9"`** — pick the right size variant instead.

| `size=` | Dimensions | Use case |
|---------|------------|----------|
| `xs` | h-7 (28px) | List-row inline mini buttons, text-adjacent actions |
| `sm` | h-9 (36px) | Toolbars, dialog footers, secondary actions |
| `default` | h-10 (40px) | Form submit, primary CTA (**default**) |
| `lg` | h-11 (44px) | Hero CTA, login |
| `icon-sm` | 32×32 | Icon-only button inside lists/cards (use `<IconButton size="sm">`) |
| `icon` | 40×40 | Standalone primary icon action (use `<IconButton size="md">`) |

### Icon Buttons

**Always use `<IconButton>`** (re-exported from `@/components/ui/button`). It wraps `Button` with sensible defaults: `variant="ghost"`, `text-muted-foreground`, hover `text-foreground bg-muted`.

```tsx
import { IconButton } from "@/components/ui/button"

<IconButton size="sm" aria-label="More">     {/* 32×32, default */}
  <MoreVertical className="h-4 w-4" />
</IconButton>

<IconButton size="md" aria-label="Settings"> {/* 40×40, standalone */}
  <Settings className="h-5 w-5" />
</IconButton>
```

**Forbidden**:

```tsx
// ❌ Raw <button> with ad-hoc styling
<button className="p-1 rounded-md hover:bg-muted">
  <MoreVertical className="h-4 w-4 text-muted-foreground" />
</button>

// ❌ Button + custom height override
<Button variant="ghost" size="icon" className="h-8 w-8">
  <X className="h-4 w-4" />
</Button>
```

**Exception**: text links rendered as `<button>` for a11y (e.g. "Show all", inline text-button) stay as bare `<button className={inlineLinkBtn}>`. Use the shared `inlineLinkBtn` constant (`'text-xs text-primary hover:underline'`) so the style is reusable.

---

## 2. Typography

### Font Family

| Token | Class | Usage |
|-------|-------|-------|
| Sans | `font-sans` (default) | All UI text — Plus Jakarta Sans + Noto Sans SC + system-ui |
| Mono | `font-mono` | Code, device IDs, monospaced data |

For inline styles (CodeMirror, Recharts), use `fontMonoStack` from `@/design-system/tokens/typography`.

### Font Size Tokens

The below-`text-xs` scale is registered as **first-class Tailwind fontSize utilities** in `tailwind.config.js` — use the `text-*` class directly in static `className` strings (no import needed); JS contexts use the matching constants from `@/design-system/tokens/typography`. **NEVER hardcode `text-[Npx]`** — tune sizes in the tailwind config (single source), never per-component. Each utility pairs a tuned line-height (~1.3–1.45); explicit `leading-*` still overrides.

| Utility | Constant | Size / Line | Use Case |
|---------|----------|-------------|----------|
| `text-micro` | `textMicro` | 9px / 12px | Extreme micro labels — data type badges in execution details |
| `text-nano` | `textNano` | 10px / 14px | Timestamps, tiny metadata, compact badges |
| `text-mini` | `textMini` | 11px / 16px | Badge text, secondary labels, tab labels |
| `text-code` | `textCode` | 12px / 17px | Inline code in markdown, code snippets |
| `text-body` | `textBody` | 13px / 19px | Chat messages, tool call text, markdown body |
| `text-heading` | `textHeading` | 15px / 22px | Markdown headings within content |

Standard Tailwind sizes fill the remaining tiers:

| Class | Size | Use Case |
|-------|------|----------|
| `text-xs` | 12px | Small labels, secondary text, helper text |
| `text-sm` | 14px | Body text, form labels, descriptions |
| `text-base` | 16px | Standard body, primary content |
| `text-lg` | 18px | Section headings |
| `text-xl`+ | 20px+ | Page titles, hero text |

### Badge Size Presets

Use `badgeSize` from typography tokens for consistent badge text:

```tsx
import { badgeSize } from '@/design-system/tokens/typography'

<Badge className={cn(badgeSize.micro, "h-4 px-1")}>int</Badge>    // 9px
<Badge className={cn(badgeSize.small, "h-5 px-2")}>online</Badge> // 10px
<Badge className={cn(badgeSize.default, "h-5 px-2")}>Active</Badge> // 11px
```

### Visual Hierarchy

```
9px(micro) → 10px(nano) → 11px(mini) → 12px(xs/code) → 13px(body) → 14px(sm) → 15px(heading) → 16px(base)
```

The smallest sizes (9-11px) are reserved for **non-essential metadata** that users scan rather than read: timestamps, device IDs, status badges, data type labels. Primary content and interactive elements always use 12px or larger.

### Exceptions: When `text-[Xpx]` Must Stay

These patterns require literal static strings and cannot use tokens:

1. **Tailwind prose modifiers** — `prose-h1:text-[15px]` (JIT needs full string)
2. **CVA variant configs** — `sm: 'text-[10px]'` (must be static for type inference)
3. **Third-party library APIs** — Recharts `tick={{ fontSize: 10 }}`, CodeMirror `theme({ fontSize: '13px' })`

### Import Pattern

```tsx
import { textNano, textMini, textBody, cn } from '@/design-system/tokens/typography'
import { cn } from '@/lib/utils'

<span className={cn(textNano, "text-muted-foreground")}>2 min ago</span>
<p className={cn(textBody, "leading-relaxed")}>Chat message content</p>
```

---

## 3. Page Layout

### Mandatory Pattern: `PageLayout`

Every page must use `PageLayout` from `@/components/layout/PageLayout`.

```tsx
<PageLayout
  title="Page Title"
  actions={<Button>Add</Button>}
  footer={<Pagination ... />}
>
  {/* Content grows naturally; PageLayout handles scrolling */}
</PageLayout>
```

**Rules:**
- Content area uses `overflow-auto` via PageLayout's scroll container — do NOT add your own scroll
- Fixed headers (tabs) go in `headerContent` prop
- Fixed footers (pagination) go in `footer` prop
- Page-level loading MUST use skeleton screens, never spinners

### Tabs Pattern: `PageTabsBar` + `PageTabsContent`

**Page-level primary actions (Add / Configure / Create) live in `PageTabsBar` `actions` — level with the tabs, top-right — NOT inside the tab content.** Build `actions` per `activeTab` (e.g. "Add Channel" on the channels tab, "Configure IM bridge" on the IM tab). Do not render a separate toolbar or button row inside `PageTabsContent` for a primary add/configure action.

```tsx
<PageLayout title="Title" headerContent={<PageTabsBar tabs={tabs} />}>
  <PageTabsContent />
</PageLayout>
```

---

## 4. Dialogs

### Standard Dialogs: `UnifiedFormDialog`

Use `UnifiedFormDialog` from `@/components/dialog/UnifiedFormDialog` for all form dialogs. Do NOT use raw `Dialog` from `@/components/ui/dialog`.

```tsx
<UnifiedFormDialog
  open={open}
  onOpenChange={setOpen}
  title="Create Device"
  width="md"
  onSubmit={handleSubmit}
  isSubmitting={saving}
  submitLabel={t('common:create')}
>
  {/* Form fields */}
</UnifiedFormDialog>
```

**Width options:** `sm` (max-w-md) | `md` (max-w-lg) | `lg` (max-w-xl) | `xl` (max-w-2xl) | `2xl` (max-w-3xl) | `3xl` (max-w-5xl)

**Features:** Mobile full-screen, loading overlay, safe area insets, escape/backdrop close, body scroll lock on mobile.

### Full-Screen Builder Dialogs: `FullScreenDialog`

For multi-step builders (rule creation, data transform), use `FullScreenDialog` from `@/components/automation/dialog/FullScreenDialog`.

### Confirmation Dialog: `useConfirm`

Promise-based confirmation dialog for destructive actions:

```tsx
import { useConfirm } from '@/components/ui/use-confirm'

const confirm = useConfirm()
const yes = await confirm({ title: 'Delete?', description: 'This cannot be undone.' })
if (yes) await api.deleteItem(id)
```

### Dialog State Hook: `useDialog`

```tsx
import { useDialog } from '@/hooks/useDialog'

const { open, data, openDialog, closeDialog } = useDialog<User>()
openDialog(user)  // passes user as data
```

---

## 5. Loading States

### LoadingState Component

`LoadingState` from `@/components/shared/LoadingState` provides two variants:

| Variant | Behavior | Use Case |
|---------|----------|----------|
| `variant="page"` | Full skeleton screen (header + 6 cards grid) | Page-level loading |
| `variant="default"` | Centered spinner with optional text | Inline/section loading |

**Size options** (default variant only): `sm` (w-4 h-4), `md` (w-6 h-6), `lg` (w-8 h-8)

```tsx
import { LoadingState, LoadingSpinner } from '@/components/shared/LoadingState'

// Page-level skeleton (MUST use for page loading)
<LoadingState variant="page" />

// Inline spinner with text
<LoadingState size="sm" text="Loading..." />

// Tiny inline spinner (buttons, badges)
<LoadingSpinner className="text-muted-foreground" />
```

### Page-Level: Skeleton Screens

```tsx
// MUST use for page-level loading
<LoadingState variant="page" />
```

`ResponsiveTable` has built-in skeleton rows when `loading={true}` — no extra loading component needed.

### Inline / Button / Dialog: Spinner

```tsx
// OK for button-level or inline loading
<Button disabled><Loader2 className="h-4 w-4 animate-spin mr-2" />Saving...</Button>
```

### Dialog Loading: Use Props

```tsx
<UnifiedFormDialog loading={initialLoad} isSubmitting={saving} />
```

### FullScreenDialog Loading

For builder dialogs, show skeleton content or a loading overlay inside the dialog body:

```tsx
<FullScreenDialog open>
  <FullScreenDialogHeader ... />
  {loading ? <LoadingState variant="page" /> : <BuilderContent />}
</FullScreenDialog>
```

### Loading Button Hook

```tsx
import { useLoadingButton } from '@/hooks/useLoadingButton'

const { isLoading, handleClick } = useLoadingButton(async () => { await api.save(data) })
<Button onClick={handleClick} disabled={isLoading}>
  {isLoading && <Loader2 className="h-4 w-4 animate-spin mr-2" />}Save
</Button>
```

**NEVER use `Loader2` spinners for page-level or table-level content loading.**

---

## 6. Data Display

### Tables: `ResponsiveTable`

```tsx
<ResponsiveTable
  columns={columns}
  data={items}
  renderCell={(key, row) => <CellComponent />}
  rowKey={(row) => row.id}
  actions={[{ label: 'Delete', onClick: handleDelete, variant: 'destructive' }]}
  loading={loading}
  emptyState={<EmptyState />}
/>
```

**Mobile behavior:** First column becomes card title, remaining columns become key-value pairs. Skeleton rows shown during loading.

**Empty state (hard rule):** `ResponsiveTable` always renders the header and shows its built-in `EmptyStateCompact` when `data` is empty. Do NOT swap the whole table for a standalone `EmptyState` on empty data, and do NOT hand-roll a custom `<p>` empty hint. If you need a custom message, pass it via `emptyState={<EmptyState .../>}` (same component family) — never a bare paragraph. The table + header must stay visible at all times so every tab's empty state looks the same.

### Virtual List: `VirtualList`

For rendering 1000+ items (sessions, logs, etc.), use `VirtualList` from `@/components/ui/virtual-list` for high-performance rendering.

### Pagination: `Pagination`

Default page size is **10** across all pages.

```tsx
<Pagination
  total={total}
  pageSize={10}
  currentPage={page}
  onPageChange={setPage}
  hideOnMobile  // enables infinite scroll on mobile
  onLoadMore={loadMore}
/>
```

### Bulk Actions: `BulkActionBar`

For multi-select operations:

```tsx
<BulkActionBar
  selectedCount={selected.length}
  onCancel={() => setSelected([])}
  actions={[{ label: 'Delete', variant: 'destructive', onClick: handleBulkDelete }]}
/>
```

### Stats Display: `StatsCard` / `MonitorStatsGrid`

```tsx
<StatsCard title="Devices" value={42} icon={Server} trend="+5%" />
<MonitorStatsGrid stats={stats} />
```

### Status Badges: `StatusBadge`

```tsx
<StatusBadge status="online" />
<StatusBadge variant="warning">Pending</StatusBadge>
```

### Empty States: `EmptyState`

```tsx
<EmptyState icon={Server} title="No devices" description="Add your first device" />
```

---

## 7. UI Components

### Button Variants

| Variant | Use Case |
|---------|----------|
| `default` | Primary action (one per section) |
| `destructive` | Delete, dangerous operations |
| `outline` | Secondary actions |
| `secondary` | Tertiary actions |
| `ghost` | Toolbar/icon buttons |
| `link` | Text-only navigation |

### Form Controls

Always import from `@/components/ui/`:

| Component | Import Path | Notes |
|-----------|-------------|-------|
| Input | `@/components/ui/input` | Standard text input |
| Select | `@/components/ui/select` | Radix-based dropdown |
| Checkbox | `@/components/ui/checkbox` | Radix-based, use `onCheckedChange` |
| Switch | `@/components/ui/switch` | Toggle switch |
| Label | `@/components/ui/label` | Form label |
| Textarea | `@/components/ui/textarea` | Multi-line input |
| Slider | `@/components/ui/slider` | Range input |
| ColorPicker | `@/components/ui/color-picker` | Color selection |
| IconPicker | `@/components/ui/icon-picker` | Icon selection |
| EntityIconPicker | `@/components/ui/entity-icon-picker` | Entity icon selection |
| CodeEditor | `@/components/ui/code-editor` | Code/text editing |

**NEVER use raw HTML `<input>`, `<select>`, `<checkbox>` elements in pages.**

### Form Layout

| Component | Import Path | Purpose |
|-----------|-------------|---------|
| FormSection | `@/components/ui/form-section` | Grouped form sections with title |
| Field | `@/components/ui/field` | Label + input + error wrapper |

### Checkbox

All checkboxes must use the shared `Checkbox` from `@/components/ui/checkbox`. Do NOT import `@radix-ui/react-checkbox` directly.

```tsx
import { Checkbox } from '@/components/ui/checkbox'
<Checkbox checked={enabled} onCheckedChange={(checked) => setEnabled(!!checked)} />
```

### Other UI Primitives

| Component | Import Path | Purpose |
|-----------|-------------|---------|
| Card | `@/components/ui/card` | Content card containers |
| Badge | `@/components/ui/badge` | Labels and tags |
| Avatar | `@/components/ui/avatar` | User avatars |
| Tabs | `@/components/ui/tabs` | Content tabs (in-page, not page-level) |
| Accordion | `@/components/ui/accordion` | Collapsible sections |
| Sheet | `@/components/ui/sheet` | Side panel |
| Progress | `@/components/ui/progress` | Progress bar |
| Skeleton | `@/components/ui/skeleton` | Custom skeleton shapes |
| ScrollArea | `@/components/ui/scroll-area` | Custom scrollable area |
| Separator | `@/components/ui/separator` | Visual divider |
| Tooltip | `@/components/ui/tooltip` | Hover information |
| Toast | `@/components/ui/use-toast` | Notification toasts |
| Confirmer | `@/components/ui/confirmer` | Confirmation dialog |

### Status Colors

Use `getStatusColorClass()` and `getStatusBgClass()` from `@/design-system/utils/format` for status-based coloring:

```tsx
import { getStatusColorClass, getStatusBgClass } from '@/design-system/utils/format'

<span className={cn(getStatusBgClass(status), getStatusColorClass(status))}>
  {status}
</span>
```

---

## 8. Dialog & Z-Index Layering Standard

### Z-Index Stack (complete ladder)

| Level | Value | Usage |
|-------|-------|-------|
| Base | `z-0` | Normal content |
| Sticky | `z-10` | In-page sticky headers, toolbars, table headers |
| Desktop chrome | `z-20` | Fixed top navigation bar (TopNav) |
| Mobile chrome | `z-30` | Mobile sticky page header (MobilePageHeader) |
| Dropdowns | `z-40` | Mobile nav, sidebars |
| Overlay | `z-50` | Dialog overlays (UnifiedFormDialog, Sheet, drawers, FAB) |
| Floating window | `z-[90]` | Non-modal floating panels (GlobalChatFab panel) — below fullscreen layers, above dialogs |
| Full Screen | `z-[100]` | Full-screen layers (FullScreenDialog, SettingsDialog, dashboard fullscreen, mobile full-screen portals) |
| Full Screen Header | `z-[110]` | Full-screen dialog headers, nested dialogs, widget fullscreen viewers (image/video/map) |
| Popovers / Alerts | `z-[200]` | Select, DropdownMenu, Popover, Tooltip, AlertDialog |
| Toast | `z-[210]` | Toast viewport — must stay visible above AlertDialog scrims |
| System | `z-[300]` | Route progress bar, instance switch overlay, backend-unavailable takeover |
| Accessibility | `z-[400]` | Skip-to-content link (focused state only) |

Rules of the ladder:

- **Every fixed/overlay element must map onto a tier in this table.** Introducing a new value (e.g. `z-[55]`, `z-[70]`) is a spec violation — extend the table instead.
- **Chrome tiers (z-20/z-30) lose to everything modal.** Drawers and dialogs cover the chrome; never geometrically dodge it with `top: var(--topnav-height)` hacks.
- **Portal policy:** all fixed overlays must portal to `#dialog-root` via `getPortalRoot()` — never to `document.body`, and never rendered inline inside the page tree. Inline fixed elements can be trapped by ancestor stacking contexts (`z-index`/`opacity`/`transform`), which silently caps them below the chrome.

### Dialog Type → Z-Index Mapping

| Dialog Type | Z-Index | Backdrop | Portal |
|------------|---------|----------|--------|
| `UnifiedFormDialog` (base) | `z-50` | `bg-black/80 backdrop-blur-sm` | `#dialog-root` |
| `FullScreenDialog` (base) | `z-[100]` (via `style`) | `bg-black/20 dark:bg-black/40 backdrop-blur-sm` | `#dialog-root` |
| `FullScreenDialog` header | `z-[110]` (via `zIndex` prop) | — | — |
| Nested `UnifiedFormDialog` inside `FullScreenDialog` | `z-[110]` via `className` | `bg-black/80 backdrop-blur-sm` | `#dialog-root` |
| Nested `Dialog` inside `FullScreenDialog` (image viewer) | `z-[110]` via `className` | `bg-black/80` | `#dialog-root` |
| Widget fullscreen viewers (image/video/map/layer) | `z-[110]` | per viewer | `#dialog-root` |
| Mobile full-screen portals (config editor, item selector) | `z-[100]` | `bg-background` | `#dialog-root` |
| `AlertDialog` / `useConfirm` (`Confirmer`) | `z-[200]` (always top) | `bg-black/60` | `#dialog-root` |
| `Sheet` (side panel) | `z-50` | `bg-bg-80 backdrop-blur-sm` | `#dialog-root` |
| Drawer (SessionSidebar / DashboardListSidebar mobile) | `z-50` (mask + panel) | `bg-overlay-light backdrop-blur-sm` | `#dialog-root` |
| Popover / Select / DropdownMenu / Tooltip | `z-[200]` | none | `#dialog-root` |
| Toast notifications | `z-[210]` | none | viewport fixed |

### Nesting Rules

**Rule 1: Base dialog is z-50. FullScreen is z-[100]. Never mix without explicit z-index override.**

```tsx
// DON'T — UnifiedFormDialog at z-50 will be hidden behind FullScreenDialog at z-100
<FullScreenDialog open>
  <UnifiedFormDialog open={showEdit} />  {/* z-50 < z-100, hidden! */}
</FullScreenDialog>

// DO — Pass className override to raise z-index above FullScreenDialog
<FullScreenDialog open>
  <UnifiedFormDialog open={showEdit} className="z-[110]" />
</FullScreenDialog>
```

**Rule 2: UnifiedFormDialog inside FullScreenDialog MUST use `className="z-[110]"`.**

The `UnifiedFormDialog` auto-detects z-index from className: it extracts the value via regex (`z-\[?(\d+)\]?`) and applies it to both overlay and content. The base `DialogContent` (`ui/dialog.tsx`) does the same extraction **in both its desktop and mobile full-screen branches** — never hardcode a z value on a dialog overlay; pass it through `className` so the overlay lifts with the content.

```tsx
// Auto-detection in UnifiedFormDialog (built-in):
const zIndexMatch = className?.match(/z-\[?(\d+)\]?/)
const overlayZIndex = zIndexMatch ? `z-[${zIndexMatch[1]}]` : 'z-50'
const dialogZIndex = zIndexMatch ? `z-[${zIndexMatch[1]}]` : 'z-50'
```

**Rule 3: AlertDialog / useConfirm is ALWAYS z-[200] regardless of context.**

Safe to call from any depth — always renders on top. No z-index override needed.

```tsx
// Safe anywhere — even inside FullScreenDialog
const confirm = useConfirm()
const yes = await confirm({ title: 'Delete?', description: 'Cannot undo.' })
```

**Rule 4: Popovers/Selects inside FullScreenDialog portal to `#dialog-root` at z-[200].**

All Radix-based popovers (Select, DropdownMenu, Popover, Tooltip) use `getPortalRoot()` → `#dialog-root` with `z-[200]`. No manual override needed.

**Rule 5: NEVER open FullScreenDialog inside UnifiedFormDialog.**

Reverse the order: use FullScreenDialog as the outer container, and UnifiedFormDialog (with z-[110]) as the inner.

```tsx
// DON'T — FullScreenDialog at z-100 inside UnifiedFormDialog at z-50
<UnifiedFormDialog open>
  <FullScreenDialog open /> {/* Layering violation! */}
</UnifiedFormDialog>

// DO — FullScreenDialog outer, UnifiedFormDialog inner with z-override
<FullScreenDialog open>
  <UnifiedFormDialog open={showForm} className="z-[110]" />
</FullScreenDialog>
```

**Rule 6: FullScreenDialog uses inline `style={{ zIndex }}` (not Tailwind class).**

The `zIndex` prop defaults to `100`. For nested FullScreenDialogs, pass `zIndex={110}`.

```tsx
<FullScreenDialog open zIndex={110}>
  {/* This FullScreenDialog sits above another one */}
</FullScreenDialog>
```

---

## 9. Store & Data Fetching

### Fetch Deduplication Pattern

Every store slice must use `fetchCache` from `@/lib/utils/async` to prevent redundant API calls:

```typescript
import { fetchCache } from '@/lib/utils/async'

fetchItems: async () => {
  if (!fetchCache.shouldFetch('items')) return
  fetchCache.markFetching('items')

  set({ loading: true })
  try {
    const data = await api.getItems()
    set({ items: data })
    fetchCache.markFetched('items')
  } catch (error) {
    fetchCache.invalidate('items')
  } finally {
    set({ loading: false })
  }
}
```

**Rules:**
- TTL is 10 seconds — check before fetching
- Always `invalidate()` after mutations (add/update/delete)
- WebSocket events use optimistic updates, NOT full refetch

### WebSocket Optimistic Updates

```typescript
updateDeviceStatus: (id: string, status: string) => {
  set((state) => ({
    devices: state.devices.map(d => d.id === id ? { ...d, status } : d)
  }))
}
```

### Store Slices

All slices follow the same pattern with `fetchCache`:

| Slice | File | Key Resources |
|-------|------|---------------|
| device | `store/slices/deviceSlice.ts` | Devices, adapters, device metrics |
| agent | `store/slices/agentSlice.ts` | AI agents |
| llmBackend | `store/slices/llmBackendSlice.ts` | LLM backends |
| extension | `store/slices/extensionSlice.ts` | Extensions |
| session | `store/slices/sessionSlice.ts` | Chat sessions |
| alert | `store/slices/alertSlice.ts` | Alerts |
| dashboard | `store/slices/dashboardSlice.ts` | Dashboards |
| settings | `store/slices/settingsSlice.ts` | System settings |
| auth | `store/slices/authSlice.ts` | Authentication (JWT + API key) |
| ui | `store/slices/uiSlice.ts` | UI state |
| update | `store/slices/updateSlice.ts` | Update checks |
| aiAnalyst | `store/slices/aiAnalystSlice.ts` | AI analyst |
| instance | `store/slices/instanceSlice.ts` | Multi-instance management |
| command | `store/slices/commandSlice.ts` | Device commands |
| message | `store/slices/messageSlice.ts` | Notification messages |
| transform | `store/slices/transformSlice.ts` | Data transforms |
| storage | `store/slices/storageSlice.ts` | Storage metrics |

### useDataSource Hook

For dashboard components that bind to telemetry data. Handles fetch deduplication, 5-second TTL, and real-time WebSocket updates:

```tsx
import { useDataSource } from '@/hooks/useDataSource'

const { data, loading, error, lastUpdate, sendCommand, sending } = useDataSource(
  dataSource,       // DataSourceOrList from dashboard config
  devices,          // Device[] from store
  { refreshInterval: 5000 }
)
```

**Features:**
- Global fetch deduplication (prevents duplicate API calls across components)
- 5-second TTL cache for telemetry data
- Real-time updates via `useEvents` hook (DeviceMetric events)
- Fuzzy matching for metric value lookup
- Command sending support for interactive data sources

### Persistence Layer

Store slices persist state using a custom middleware pattern. Key persistence locations:

| Data | Storage | Key |
|------|---------|-----|
| JWT token | `localStorage` / `sessionStorage` | `neomind_token` |
| User info | `localStorage` / `sessionStorage` | `neomind_user` |
| API key | `sessionStorage` | `neomind_api_key` |
| Current instance | `localStorage` | `currentInstanceId` |
| Instance cache | `localStorage` | `neomind_instance_cache` |
| Time preferences | `localStorage` | `neomind_preferences` |

---

## 10. Portal System

All modal/popover content must render through `getPortalRoot()` from `@/lib/portal`. This ensures correct z-index stacking.

```tsx
import { getPortalRoot } from '@/lib/portal'

<SomePrimitive.Portal container={getPortalRoot()}>
  {children}
</SomePrimitive.Portal>
```

---

## 11. Error Handling

### Centralized Error System

All error handling uses `@/lib/errors.ts` and `useErrorHandler` hook. **NEVER use bare `console.error` + `toast`** — always use the centralized system.

```tsx
import { useErrorHandler } from '@/hooks/useErrorHandler'

const { handleError, showSuccess, withErrorHandling } = useErrorHandler()
```

### Error Handler API

| Method | Purpose |
|--------|---------|
| `handleError(error, options?)` | Log + show toast. Options: `{ showToast, userMessage, operation }` |
| `withErrorHandling(fn, options?)` | Async wrapper, auto-catches and handles |
| `showSuccess(message)` | Success toast |
| `getErrorMessage(error)` | Extract user-friendly message |

### Usage Patterns

```tsx
// Manual handling
try {
  await api.deleteDevice(id)
  showSuccess('Device deleted')
} catch (error) {
  handleError(error, { operation: 'Delete device' })
}

// Automatic handling
const result = await withErrorHandling(
  () => api.createDevice(data),
  { operation: 'Create device' }
)
```

### Error Classification

Import from `@/lib/errors`:

| Function | Purpose |
|----------|---------|
| `isNetworkError(error)` | Network/connection failures |
| `isAuthError(error)` | 401/authentication errors |
| `isValidationError(error)` | 400/422 validation errors |
| `isNotFoundError(error)` | 404 errors |
| `isConflictError(error)` | 409 conflicts |
| `logError(error, context?)` | Structured error logging |

### Form Submission Hook

```tsx
import { useFormSubmit } from '@/hooks/useErrorHandler'

const { isSubmitting, handleSubmit } = useFormSubmit({
  successMessage: 'Saved',
  errorOperation: 'Save settings',
})

<form onSubmit={handleSubmit(async () => await api.save(data))}>
```

---

## 12. Internationalization (i18n)

All user-visible text must use the translation system. Translation files are in `src/i18n/locales/{en,zh}/`.

### 12.1 Basic Usage

**NEVER hardcode strings** in components — always use `t()` with appropriate key.

```tsx
// ✅ Correct
const { t } = useTranslation(['common', 'devices'])
<span>{t('devices:title')}</span>

// ❌ Wrong
<span>Device Title</span>
```

### 12.2 Namespace Rules

| Rule | Description |
|------|-------------|
| **One namespace per domain** | `devices.json` for device page keys, `agents.json` for agent page keys, etc. |
| **`common.json` is the default namespace** | Only for truly shared keys (cancel, save, delete, error messages). NOT for page-specific or component-specific keys. |
| **`dashboardComponents` namespace** | ALL dashboard widget/component keys go in `dashboard-components.json`, registered as `dashboardComponents`. This includes `visualDashboard.*`, `configRenderer.*`, `dataSource.*`, etc. |
| **Register before use** | Every namespace must be: (1) imported in `config.ts`, (2) listed in `ns` array, (3) declared in `types.ts`. |
| **No orphaned files** | Locale files must be loaded by `config.ts`. If no code references a namespace, delete both the file and its registration. |

### 12.3 Key Naming Convention

```
{pageOrFeature}.{componentOrSection}.{field}
```

Examples:
- `devices.add.adapterType` — device page, add dialog, adapter type field
- `agents.card.successRate` — agents page, card component, success rate label
- `common.actions` — shared across all pages

**NEVER** put page-specific keys in `common.json`. If a key is only used by one page or component, it belongs in that page's namespace.

### 12.4 Cross-Namespace References

When a component needs keys from another namespace, use the explicit `ns:key` syntax:

```tsx
// ✅ Explicit namespace reference
const { t } = useTranslation()  // default = common
<span>{t('devices:add.adapterType')}</span>

// ✅ Or include the namespace in useTranslation
const { t } = useTranslation(['common', 'devices'])
<span>{t('add.adapterType')}</span>  // resolves in 'devices' ns
```

**Prefer `useTranslation('targetNamespace')`** for components that primarily use one namespace. Use `t('ns:key')` only when you need a few keys from other namespaces.

### 12.5 Adding New Keys Checklist

When adding i18n keys, follow this checklist:

1. **Identify the correct namespace** — which page/feature does this key belong to?
2. **Add to BOTH `en/` and `zh/` files** — EN and ZH must have identical key structure
3. **Provide real translations** — no placeholder text like "Desc", "Placeholder", "Title"
4. **Register if new namespace** — update `config.ts` (import + resource + ns array) and `types.ts`
5. **Verify** — run `npx tsc --noEmit` to catch missing keys

### 12.6 Common Mistakes to Avoid

| Mistake | Correct Approach |
|---------|-----------------|
| Adding dashboard widget keys to `common.json` | Use `dashboardComponents` namespace |
| Creating a new `.json` file without registering it | Must add to `config.ts` imports + `ns` array + `types.ts` |
| EN/ZH key structure mismatch | Both files must have the same key paths |
| Hardcoded English in ZH file | All ZH values must be Chinese translations |
| Loading unused namespaces in `useTranslation()` | Only list namespaces you actually use keys from |

---

## 13. Glass Morphism & Surfaces

| Surface | Class | Use Case |
|---------|-------|----------|
| Glass light | `bg-glass` | Floating panels |
| Glass heavy | `bg-glass-heavy` | Fixed headers/footers |
| Surface glass | `bg-surface-glass` | Overlays with backdrop-blur |
| Glass border | `border-glass-border` | Subtle borders |
| Glass border hover | `border-glass-border-hover` | Hover state |
| Card | `bg-card` | Content cards |
| Muted | `bg-muted` | Subtle backgrounds |
| Chrome | `bg-[var(--chrome)]` | App chrome: top navigation bar |

**Chrome is opaque by design.** Navigation chrome (TopNav) uses the opaque `--chrome`
token, not glass: translucent bars caused transparent→solid paint flashes on WebKit and
let content bleed through during overscroll. Glass surfaces are for *floating* elements
(panels, masks, FABs) — `PageLayout`'s fixed footer (`bg-surface-glass`) is the one
deliberate exception.

---

## 14. Mobile & Responsive

### Breakpoints (Tailwind defaults)

| Prefix | Width | Target |
|--------|-------|--------|
| (none) | < 640px | Mobile |
| `sm:` | >= 640px | Large phones |
| `md:` | >= 768px | Tablets |
| `lg:` | >= 1024px | Laptops |
| `xl:` | >= 1280px | Desktops |

### Mobile Detection Hooks

Import from `@/hooks/useMobile`:

| Hook | Returns | Purpose |
|------|---------|---------|
| `useIsMobile()` | `boolean` | Viewport < 768px |
| `useIsTouchDevice()` | `boolean` | Touch capability |
| `useDeviceType()` | `'mobile' \| 'tablet' \| 'desktop'` | Full device classification |

### Touch Interaction Hooks

| Hook | Purpose |
|------|---------|
| `useTouchHover(options?)` | Touch-friendly hover state (toggle on tap) |
| `useLongPress(options)` | Long-press gesture (500ms default, 10px threshold) |
| `useTouchHandler()` | Touch event management |

### Safe Area Insets (iPhone X+ notches)

CSS utility classes: `.safe-top`, `.safe-bottom`, `.safe-left`, `.safe-right`, `.pt-safe`, `.pb-safe`

Hook: `useSafeAreaInsets()` returns `{ top, right, bottom, left }` pixel values.

CSS variables:
```css
/* Desktop */
--topnav-height: calc(4rem + env(safe-area-inset-top, 0px));
--bottom-nav-height: 0px;
/* Mobile (set dynamically by useVisualViewport) */
--topnav-height: calc(0px + env(safe-area-inset-top, 0px));
--bottom-nav-height: calc(3.5rem + env(safe-area-inset-bottom, 0px));
--chat-content-padding-top: calc(6.5rem + env(safe-area-inset-top, 0px));
```

### Mobile Layout: Hamburger Menu + Per-Page Header

Mobile-only navigation restructure (desktop unchanged).

**No global brand bar on mobile.** `<TopNav />` is wrapped in
`{!isMobile && <TopNav />}` at the App shell. `--topnav-height` is `0px` on
mobile; each page owns its top area.

**Per-page header** (`MobilePageHeader`, `@/components/layout/MobilePageHeader`):
48px sticky bar (`h-12` + `safe-top`) with layout `[☰ drawer] [leftExtra?] [title + subtitle?] [actions?]`.
The hamburger (☰) always appears on the left and opens the nav drawer via
the `useMobileNav` store (`@/store/mobileNav`). Each page customizes title /
subtitle / actions.

- `PageLayout` renders `<MobilePageHeader title subtitle actions />` automatically
  on mobile — pages that use `PageLayout` get it for free.
- Pages NOT using `PageLayout` (chat, VisualDashboard, SystemPage) render
  `<MobilePageHeader />` explicitly as the first child of their content.

**Hamburger drawer** (`MobileNav`, `@/components/layout/MobileNav`): left
slide-in `Sheet` rendered once at the App shell. Contains brand header
(logo + name + version + user row), grouped nav items (Primary: Chat /
Agent / 设备 / 仪表板; System: 自动化 / 数据 / 消息 / 扩展 / 设置), and a
logout entry. Messages entry shows unread-alerts badge.

**`--bottom-nav-height`** is `0px` on mobile (no bottom nav in the hamburger
approach). `--topnav-height` is `0px`; page headers carry their own `safe-top`.

**Desktop is fully unchanged.**

### Touch Targets

Minimum 44x44px per iOS/Android guidelines. Small icon buttons (h-6 through h-9) are acceptable exceptions.

### Body Scroll Lock

Use `useMobileBodyScrollLock()` when a modal/dialog opens on mobile to prevent background scrolling.

---

## 15. Form Validation

The codebase uses a **custom `useForm` hook** (not react-hook-form or zod):

```tsx
import { useForm } from '@/hooks/useForm'

const { values, errors, setValue, handleSubmit, isSubmitting } = useForm({
  initialValues: { name: '', type: 'mqtt' },
  onSubmit: async (values) => { await api.createDevice(values) },
  validate: (values) => {
    const errors: Record<string, string> = {}
    if (!values.name) errors.name = 'Name is required'
    return Object.keys(errors).length > 0 ? errors : undefined
  }
})
```

**Features:**
- Live validation on value change
- Custom validation function (returns `Record<string, string>` or `undefined`)
- `isSubmitting` state
- `submitError` for submission-level errors
- `setError(key, msg)` / `clearError(key)` for manual error management
- `reset()` to restore initial values

---

## 16. Spacing & Radius

Use Tailwind's standard spacing utilities (`p-2`, `gap-4`, etc.) and the predefined radius tokens. Base `--radius` is 8px — a tight, professional feel (reference design language); Tailwind derives the ladder from it:

| Token | Value | Class |
|-------|-------|-------|
| sm | 4px | `rounded-sm` |
| md | 6px | `rounded-md` |
| lg | 8px | `rounded-lg` |
| xl | 12px | `rounded-xl` |
| 2xl | 16px | `rounded-2xl` |
| full | circle | `rounded-full` |

---

## 17. Animation

| Name | Duration | Use Case |
|------|----------|----------|
| `animate-fade-in` | 200ms | General appearance |
| `animate-fade-out` | 200ms | Disappearance |
| `animate-fade-in-up` | 300ms | Content appearing from below |
| `animate-scale-in` | 200ms | Dialogs, popovers |
| `animate-scale-out` | 200ms | Closing animations |
| `animate-slide-in` | 200ms | Slide transitions |
| `animate-slide-in-from-*` | 300ms | Directional slides (top/bottom/left/right) |
| `animate-pulse-slow` | 3s | Status indicators |
| `animate-spin-slow` | 3s | Slow loading |
| `animate-bounce-subtle` | 2s | Subtle bounce (5px Y) |
| `animate-shimmer` | 2s | Skeleton loading shimmer |
| `animate-typewriter` | 2s | Typewriter effect |
| `animate-blink` | 1s | Blinking cursor |

**Stagger delays:** `delay-0`, `delay-100`, `delay-150`, `delay-200`, `delay-300`, `delay-400`, `delay-500`

**Timing CSS variables:**
- `--duration-fast`: 150ms (hover, focus)
- `--duration-normal`: 200ms (general transitions)
- `--duration-slow`: 300ms (layout changes)

**Easing:** `--ease-out`, `--ease-in-out`, `--ease-spring`

---

## 18. Infinite Scroll

For pages with infinite scroll on mobile:

```tsx
import { useInfiniteScroll } from '@/hooks/useInfiniteScroll'

const { ref, isLoading } = useInfiniteScroll({
  hasMore: page * pageSize < total,
  isLoading: loadingMore,
  onLoadMore: () => setPage(p => p + 1),
  threshold: 200,
})
```

---

## 19. Polling

For data that needs periodic refresh:

```tsx
import { useVisiblePolling } from '@/hooks/useVisiblePolling'

// Only polls when tab is visible
useVisiblePolling(fetchData, { interval: 30000, enabled: true })
```

---

## 20. Search

### SearchBar: Global search component

```tsx
import { SearchBar } from '@/components/shared'

<SearchBar
  placeholder="Search..."
  onSearch={handleSearch}
  resultTypes={[
    { key: 'device', label: 'Device' },
    { key: 'rule', label: 'Rule' },
  ]}
/>
```

### SearchResultsDialog

Full search results overlay with categorized results.

---

## 21. API Client & Data Fetching

### Centralized API Client

All API calls go through the centralized `api` object from `@/lib/api`:

```tsx
import { api } from '@/lib/api'

const devices = await api.getDevices()
await api.createDevice(data)
```

### Instance-Aware API (Multi-Instance)

The API client supports switching between local and remote instances:

```tsx
import { getApiBase, setApiBase, getApiKey, setApiKey, isTauriEnv } from '@/lib/api'

// API base resolution order:
// 1. Dynamic override (set by InstanceSlice) — highest priority
// 2. VITE_API_BASE_URL env variable
// 3. Tauri: 'http://localhost:9375/api'
// 4. Web: '/api' (relative path)

setApiBase('http://192.168.1.100:9375/api')  // Switch to remote instance
setApiBase('')  // Reset to default
```

**Authentication priority:** JWT token (from `tokenManager`) > API key (from `sessionStorage`).

### snake_case → camelCase Transformation

Backend returns `snake_case`. The API client transforms responses to `camelCase` where needed. Type definitions in `@/types` use `camelCase`.

### Request Authentication

Every request automatically includes:
- `Authorization: Bearer <token>` header if JWT exists
- `X-API-Key: <key>` header if API key is set (remote instances)
- `AbortSignal.timeout()` for request timeouts (polyfilled for older Safari)

### Error Handling Integration

API errors flow through `onUnauthorized()` callback registry — any component can register to handle 401 errors:

```tsx
import { onUnauthorized } from '@/lib/api'

const cleanup = onUnauthorized(() => {
  // Handle session expiry (e.g., redirect to login)
})
```

---

## 22. WebSocket & Real-Time Patterns

### Chat WebSocket

`@/lib/websocket.ts` — singleton `ws` object for chat communication:

```tsx
import { ws } from '@/lib/websocket'

// Connect (auto-called by App on auth)
ws.connect()

// Send message
ws.send(JSON.stringify({ type: 'user_message', content: 'Hello' }))

// Listen for messages
ws.onMessage((data) => { /* handle */ })

// Connection state
ws.onConnection((connected, isReconnect) => { /* handle */ })
ws.isConnected()
```

**Reconnection:** Exponential backoff, max 15 attempts, max delay 30s. Auto-reconnects on network recovery. Stops reconnecting on auth rejection (code 4001).

### Extension WebSocket

`@/lib/extension-stream.ts` — `ExtensionStreamClient` class for extension streaming:

```tsx
import { ExtensionStreamClient } from '@/lib/extension-stream'

const client = new ExtensionStreamClient('weather-forecast-v2')
client.connect({ /* config */ })
client.onResult((data, dataType, sequence) => { /* handle binary data */ })
client.onSessionClosed((stats) => { /* session ended */ })
```

**Features:** Session-based streaming, binary data support, max 5 reconnect attempts with 1s base delay.

### Global Events (SSE + WS)

`@/lib/events.ts` — event streaming from the NeoMind event bus:

```tsx
import { useEvents } from '@/hooks/useEvents'

// Subscribe to specific event categories
useEvents({
  categories: ['device', 'alert'],
  onEvent: (event) => { /* handle */ },
})
```

**Transport:** Prefers SSE (`/api/events/stream`), falls back to WS (`/api/events/ws`). Supports filtering by event category and type.

**Event categories:** `device`, `rule`, `llm`, `alert`, `tool`, `agent`, `extension`, `all`

### URL Construction

All WebSocket URLs built via `buildWsUrl()` from `@/lib/urls`:

```tsx
import { buildWsUrl } from '@/lib/urls'

buildWsUrl('http://localhost:9375', '/api/chat')
// → 'ws://localhost:9375/api/chat'
```

---

## 23. Routing & Page Organization

### Route Structure

Pages are in `src/pages/`, lazy-loaded via `React.lazy()` for bundle splitting:

| Route | Page | File |
|-------|------|------|
| `/login` | LoginPage | `pages/login.tsx` |
| `/setup` | SetupPage | `pages/setup.tsx` |
| `/`, `/chat`, `/chat/:sessionId` | ChatPage | `pages/chat.tsx` |
| `/visual-dashboard`, `/visual-dashboard/:id` | VisualDashboard | `pages/dashboard-components/VisualDashboard.tsx` |
| `/data` | DataExplorerPage | `pages/data-explorer.tsx` |
| `/devices`, `/devices/:id`, `/devices/types`, `/devices/drafts` | DevicesPage | `pages/devices.tsx` |
| `/automation`, `/automation/transforms` | AutomationPage | `pages/automation.tsx` |
| `/agents`, `/agents/memory`, `/agents/skills` | AgentsPage | `pages/agents.tsx` |
| `/settings` | SettingsPage | `pages/settings.tsx` |
| `/messages`, `/messages/channels` | MessagesPage | `pages/messages.tsx` |
| `/extensions` | ExtensionsPage | `pages/extensions.tsx` |

**Settings is a dialog, not a page.** The `SettingsDialog` is mounted once at the app root (`App.tsx`); pages open it via `useStore.getState().openSettings(section?)`. The legacy `/settings` route renders a small shell (`SettingsRoute` in `App.tsx`) that opens the dialog and redirects to `/` — deep links land on chat with settings open, never a blank page. Do NOT create a settings page route.

### Protected Route Pattern

```tsx
// ProtectedRoute checks: JWT token OR API key → render children, else redirect to /login
<ProtectedRoute>
  <YourPage />
</ProtectedRoute>
```

**Flow:** Check `tokenManager.getToken()` or `getApiKey()`. If neither exists, redirect to `/login`. Background check for setup status.

### Route Guards

| Guard | Component | Purpose |
|-------|-----------|---------|
| `ProtectedRoute` | Wraps all `/` routes | Requires JWT or API key |
| `SetupRoute` | Wraps `/setup` | Only accessible when setup required |

### Page Layout Pattern

Every page uses `PageLayout` with optional `PageTabsBar` for tab-based navigation (e.g., `/devices/types`, `/agents/skills`).

---

## 24. Time & Date Handling

### useTimeFormat Hook

```tsx
import { useTimeFormat } from '@/hooks/useTimeFormat'

const {
  formatTime,          // Date → "2:30 PM" / "14:30" (based on preference)
  formatTimeShort,     // Date → "2:30 PM"
  formatDateTime,      // Date → "Jan 15, 2:30 PM"
  formatDate,          // Date → "Jan 15, 2026"
  formatRelativeTime,  // timestamp → "5 min ago"
  formatTimeWithTimezone, // Date + tz → formatted with timezone
  formatCurrentTimeInTimezone, // tz → current time in timezone
  getCurrentTimeInfo,  // tz → { time, date, offset, ... }
  preferences,         // TimePreferences (12h/24h, timezone)
  refresh,             // Reload preferences from localStorage
} = useTimeFormat()
```

**Rules:**
- **NEVER hardcode time formats** — always use `useTimeFormat` hooks
- Preferences stored in `localStorage` key `neomind_preferences`
- Supports 12h/24h toggle, timezone selection
- Auto-updates when preferences change in other tabs (via `StorageEvent`)

### useGlobalTimezone Hook

For system-wide timezone (agent scheduling, server operations):

```tsx
import { useGlobalTimezone } from '@/hooks/useTimeFormat'

const { timezone, updateTimezone, availableTimezones } = useGlobalTimezone()
```

This syncs timezone with the backend via API (`api.getTimezone()`, `api.updateTimezone()`).

### Formatting Functions

Low-level formatters in `@/lib/time`:

| Function | Purpose |
|----------|---------|
| `formatTime(date, prefs)` | Time with user preference (12h/24h) |
| `formatTimeShort(date, prefs)` | Compact time |
| `formatDateTime(date, prefs)` | Date + time |
| `formatDate(date)` | Date only |
| `formatRelativeTime(timestamp)` | Relative ("5 min ago", "2 hours ago") |
| `formatTimeWithTimezone(date, tz, prefs)` | Time in specific timezone |

---

## 25. Authentication Patterns

### Dual Authentication: JWT + API Key

The system supports two auth methods:

| Method | Use Case | Storage | Header |
|--------|----------|---------|--------|
| JWT Token | Local user login | `localStorage` / `sessionStorage` | `Authorization: Bearer <token>` |
| API Key | Remote instance access | `sessionStorage` | `X-API-Key: <key>` |

**Priority:** JWT token checked first, then API key.

### Auth Store Slice

```tsx
import { useStore } from '@/store'

const { isAuthenticated, user, login, logout } = useStore()

// Login
await useStore.getState().login(username, password, rememberMe)

// Logout
await useStore.getState().logout()

// Check auth (auto-called on mount)
useStore.getState().checkAuthStatus()
```

### Token Manager

`tokenManager` manages JWT lifecycle:

```tsx
import { tokenManager } from '@/lib/auth'  // primary module
import { tokenManager } from '@/lib/api'    // deprecated re-export

// Storage: localStorage (remember=true) or sessionStorage (remember=false)
tokenManager.getToken()
tokenManager.setToken(token, remember)
tokenManager.clearToken()
tokenManager.getUser()
tokenManager.setUser(user, remember)
tokenManager.clearUser()
```

### Multi-Instance Auth

When connecting to a remote instance, the `instanceSlice` handles:

1. Set API base URL via `setApiBase()`
2. Store API key via `setApiKey()` (persisted to `sessionStorage`)
3. Clear on switch back: `clearApiKey()` + `setApiBase('')`

```tsx
// Switch to remote instance
setApiBase('http://192.168.1.100:9375/api')
setApiKey('nm-xxx...')

// Switch back to local
clearApiKey()
setApiBase('')
```

### Protected Route Implementation

In `App.tsx`, protected routes check auth on every render (not in `useEffect`) for immediate response:

```tsx
function ProtectedRoute({ children }) {
  const token = tokenManager.getToken()
  const apiKey = getApiKey()
  if (!token && !apiKey) return <Navigate to="/login" replace />
  return <>{children}</>
}
```

---

## 26. Icon System

### Icon Library

All icons use **lucide-react**. Do NOT import from other icon libraries.

```tsx
import { Thermometer, Wifi, Settings } from 'lucide-react'
```

### Icon Size Standards

| Context | Size | Class |
|---------|------|-------|
| Button icon | 20px | `h-5 w-5` |
| Inline icon | 16px | `h-4 w-4` |
| Feature icon (card/dialog) | 20-24px | `h-5 w-5` or `h-6 w-6` |
| TopNav icon | 20px | `h-5 w-5` |
| Micro icon (badge, tag) | 14px | `h-3.5 w-3.5` |

### Entity Icon Mapping

`@/design-system/icons` provides centralized icon-to-entity-type mapping:

```tsx
import { getIconForEntity, EntityIcon, statusIcons } from '@/design-system/icons'

// Get icon component by entity type string (case-insensitive)
const Icon = getIconForEntity('temperature')  // → Thermometer
const Icon2 = getIconForEntity('humidity')     // → Droplets
const Icon3 = getIconForEntity('unknown_type') // → Activity (fallback)

// Pre-built EntityIcon component
<EntityIcon type="temperature" size={24} />

// Status icons with semantic colors
const { icon: WifiIcon, color } = statusIcons.online  // { icon: Wifi, color: 'text-success' }
```

**Entity type mappings:** `temperature`→Thermometer, `humidity`→Droplets, `pressure`→Gauge, `battery`→Battery, `power`→Power, `energy`→Zap, `light`→Lightbulb, `door`→DoorOpen, `lock`→Lock, `fan`→Fan, `location`→MapPin, `time`→Clock, etc.

**Status icon mappings:** `online`→Wifi (text-success), `offline`→WifiOff (text-muted-foreground), `error`→XCircle (text-error), `warning`→AlertTriangle (text-warning), `success`→CheckCircle2 (text-success), `loading`→Clock (text-info animate-spin)

### Icon Usage Rules

- **NEVER use emoji** as icons — always use lucide-react
- Use `getIconForEntity()` for dynamic icon rendering (e.g., device type → icon)
- Use `statusIcons` for status indicators with built-in semantic colors
- Use `Activity` as the fallback icon (via `DefaultIcon` export)

---

## 27. Dark Mode

### Theme System

Three-way theme toggle: **light** / **dark** / **system** (follows OS preference).

```tsx
import { useTheme } from '@/components/ui/theme'

const { theme, setTheme, resolvedTheme } = useTheme()
// theme: 'light' | 'dark' | 'system'
// resolvedTheme: 'light' | 'dark' (actual applied theme)
```

### Implementation

- **ThemeProvider** wraps the app root in `App.tsx`
- Theme stored in `localStorage` key `"theme"`
- Applies class `"light"` or `"dark"` to `<html>` element
- System preference detected via `matchMedia("(prefers-color-scheme: dark)")`
- Flash prevention: theme resolved synchronously on initial render

### CSS Pattern

All color variables defined in `index.css` with dual values:

```css
:root { --color-primary: oklch(0.55 0.15 260); }
.dark { --color-primary: oklch(0.75 0.12 260); }
```

**Rules:**
- All colors MUST be defined for both `:root` (light) and `.dark`
- NEVER use `dark:` Tailwind prefix for custom colors — use CSS variables instead
- The `dark:` prefix is only acceptable for Tailwind's built-in palette colors (which we don't use for custom colors)

### Theme Toggle UI

`ThemeToggle` component in `@/components/layout/ThemeToggle`:

```tsx
import { ThemeToggle } from '@/components/layout/ThemeToggle'

// Renders dropdown with Light/Dark/System options
// Icon changes based on current theme:
//   system → Monitor icon
//   dark   → Sun icon
//   light  → Moon icon
<ThemeToggle />
```

---

## 28. Navigation (AppSidebar + TopBar)

### Architecture (desktop)

- **AppSidebar** (`components/layout/AppSidebar.tsx`): persistent in-flow left column — the app's primary navigation. NOT a fixed overlay: the shell (`App.tsx`) lays out `[AppSidebar][column: TopBar + main]` as flex-row. The sidebar header and the TopBar both carry `border-b`, so the top chrome band reads as one surface with a continuous dividing line.
- **TopBar** (`components/layout/TopBar.tsx`): slim 48px (`h-12`) in-flow chrome bar — the global action cluster: instance selector, onboarding guide, alerts bell (`AlertsMenu`). Doubles as the macOS Tauri window drag region.
- **navItems.ts** (`components/layout/navItems.ts`): the shared nav definition (paths, icons, i18n keys, groups, active-route logic). The sidebar and MobileNav must not fork these.

### Sidebar structure

- Header: brand logo + collapse toggle; reserves the macOS traffic-light strip (`--titlebar-inset`) and doubles as a drag region
- PRIMARY group: Chat `/chat`, Agents `/agents`, Devices `/devices`, Visual Dashboard `/visual-dashboard`
- SYSTEM group (labelled `navShort.system`): Automation, Data Explorer, Messages, Extensions
- Footer (bottom-left): **user avatar** whose upward dropdown holds theme / language / settings / about / logout (settings has no standalone entry — the dialog opens from the avatar menu)
- Active state: **solid accent pill** — `bg-[var(--nav-active-bg)] text-[var(--nav-active-fg)]` (light: deep orange + white; dark: bright orange + dark). The sidebar itself is the `--sidebar-bg` rail (light: gray below canvas; dark: below-canvas dark), separated from content by **color contrast, no border** — the TopBar and sidebar carry no decorative borders either

### Collapse behavior

Two states: expanded `240px` (icon + label) ↔ collapsed `60px` icon rail (tooltips on `side="right"` keep names accessible; buttons retain `aria-label`). State lives in `uiSlice.appSidebarCollapsed`, persisted via the store (`neomind-store`). The current width is exported as `--app-sidebar-width` on `<html>` — fixed full-bleed surfaces (ChatPage's keyboard-aware container, PageLayout's fixed footer) offset with `left: var(--app-sidebar-width, 0px)`. **Any new fixed element must do the same.**

### Mobile Navigation (unchanged)

Mobile keeps its own system — the sidebar unmounts below `md`: per-page `MobilePageHeader` (hamburger) → `MobileNav` Sheet drawer (`PRIMARY` + `SYSTEM_ENTRIES` from navItems groups), plus per-page bottom tab bars and swipe gestures. `--topnav-height` is `0px` on mobile; on desktop it still carries the TopBar's measured height (set via `setTopNavHeight`).

### Height Management

TopBar height is tracked via `setTopNavHeight()` and exposed as `--topnav-height` for legacy consumers (e.g. ChatPage's fixed container padding). Since the TopBar is in-flow, `main` needs no compensating padding.

---

## 29. Toast & Notifications

### Toast System

Radix UI-based toast with max 1 visible toast at a time.

**Viewport**: `fixed top-0 z-[200]` on mobile (top-center), `fixed bottom-0 right-0 z-[200]` on desktop (bottom-right, max-width 420px).

### Usage Patterns

```tsx
// Inside React components
import { useToast } from '@/components/ui/use-toast'
const { toast } = useToast()
toast({ title: 'Saved', description: 'Settings updated' })

// Outside React (API calls, plain functions)
import { notifySuccess, notifyError, notifyFromError } from '@/lib/notify'
notifySuccess('Device created')
notifyError('Failed to connect')
notifyFromError(error, 'Failed to save')  // Auto-extracts message
```

### Toast Variants

| Variant | Use Case | Style |
|---------|----------|-------|
| `default` | Success, info, warning | `border bg-background text-foreground` |
| `destructive` | Errors, failures | `border-destructive bg-destructive text-destructive-foreground` |

### Centralized Notification

`@/lib/notify` provides global notification functions callable from anywhere:

| Function | Purpose |
|----------|---------|
| `notifySuccess(message, title?)` | Success toast |
| `notifyError(message, title?)` | Error toast (destructive variant) |
| `notifyWarning(message, title?)` | Warning toast |
| `notifyInfo(message, title?)` | Info toast |
| `notifyFromError(error, fallback?)` | Auto-extract error message and show destructive toast |

**Rules:**
- Use `notify*` functions from `@/lib/notify` for global notifications
- Use `useToast` hook only when you need toast actions (e.g., undo button)
- Toast auto-dismisses; swipe-to-dismiss supported

---

## 30. Data Visualization (Charts)

### Chart Library

Uses **Recharts** for all data visualization. Wrapped in dashboard components at `@/components/dashboard/generic/`.

### Chart Types

| Component | File | Use Case |
|-----------|------|----------|
| `LineChart` | `dashboard/generic/LineChart.tsx` | Time-series metrics |
| `BarChart` | `dashboard/generic/BarChart.tsx` | Category comparison |
| `PieChart` | `dashboard/generic/PieChart.tsx` | Distribution/proportion |
| `Sparkline` | `dashboard/generic/Sparkline.tsx` | Mini inline trend |
| `LEDIndicator` | `dashboard/generic/LEDIndicator.tsx` | Binary on/off state |
| `ProgressBar` | `dashboard/generic/ProgressBar.tsx` | Progress/gauge |

### Chart Color Tokens

Use `chartColors` / `chartColorsHex` from `@/design-system/tokens/` for consistent chart palettes. Maps to `--chart-1` through `--chart-6` CSS variables.

### Data Binding Pattern

Charts bind to data sources via `useDataSource` hook:

```tsx
const { data, loading, error } = useDataSource(dataSource, devices)
if (loading) return <LoadingState size="sm" />
if (error) return <div className="text-muted-foreground text-sm">{error}</div>
<LineChart data={data} config={chartConfig} />
```

### Chart Rules

- Always wrap in `ResponsiveContainer` for adaptive sizing
- Use design system color tokens, never hardcoded colors
- Show skeleton loading state while data fetches
- Handle empty data with meaningful empty state

---

## 31. Tooltip & Popover Patterns

### Tooltip

```tsx
import { Tooltip, TooltipTrigger, TooltipContent, TooltipProvider } from '@/components/ui/tooltip'

<TooltipProvider delayDuration={500}>
  <Tooltip>
    <TooltipTrigger asChild>
      <Button variant="ghost" size="icon">...</Button>
    </TooltipTrigger>
    <TooltipContent side="bottom" className="text-xs px-2 py-1">
      {t('nav.settings')}
    </TooltipContent>
  </Tooltip>
</TooltipProvider>
```

**Rules:**
- Portals to `#dialog-root` via `getPortalRoot()` at `z-[200]`
- Use `asChild` on trigger to avoid wrapping DOM
- Keep tooltip text concise (use `text-xs` or `text-[10px]`)
- `delayDuration={500}` for nav items (default is 700ms)
- Supported sides: `top`, `bottom`, `left`, `right`

### When to Use Tooltip vs Other Patterns

| Pattern | When to Use |
|---------|-------------|
| Tooltip | Icon-only buttons that need a label hint |
| Title attribute | Native HTML elements, quick access |
| Description text | Complex concepts that need explanation |
| Inline help text | Form fields needing guidance |

---

## 32. Copy/Clipboard Pattern

For copy-to-clipboard actions, use the standard pattern:

```tsx
import { Check, Copy } from 'lucide-react'

const [copied, setCopied] = useState(false)

const handleCopy = async () => {
  await navigator.clipboard.writeText(textToCopy)
  setCopied(true)
  setTimeout(() => setCopied(false), 2000)
}

<Button variant="ghost" size="icon" onClick={handleCopy}>
  {copied ? <Check className="h-4 w-4 text-success" /> : <Copy className="h-4 w-4" />}
</Button>
```

**Rules:**
- Show checkmark icon for 2 seconds after copy
- Use `text-success` color for the checkmark feedback
- Use `navigator.clipboard.writeText()` (no additional library needed)

---

## 33. Accessibility (a11y)

### Foundation: Radix UI

All interactive primitives use **Radix UI**, which provides built-in accessibility: keyboard navigation, ARIA attributes, focus management, screen reader support. Components with built-in a11y:

| Radix Component | A11y Features |
|----------------|---------------|
| `Dialog` / `AlertDialog` | Focus trap, Escape close, `aria-modal`, `aria-labelledby` |
| `Select` | Arrow key navigation, type-ahead, `aria-expanded` |
| `Tabs` | Arrow key navigation, `aria-selected` |
| `Checkbox` / `Switch` | `aria-checked`, keyboard toggle |
| `DropdownMenu` | Arrow keys, type-ahead, `aria-expanded` |
| `Tooltip` | Focus trigger, Escape dismiss |
| `Toast` | `role="status"`, `aria-live` |
| `Popover` | Focus management, Escape dismiss |
| `Accordion` | Arrow keys, Home/End, `aria-expanded` |
| `Slider` | Arrow keys, `aria-valuenow` |

### Focus Management

**Dialogs** — `UnifiedFormDialog` implements tab trapping:

```tsx
// Auto-focuses first focusable element on open
// Tab/Shift+Tab wraps between first and last focusable elements
// Escape closes dialog
```

**Chat input** — Auto-focuses after sending a message:

```tsx
inputRef.current?.focus()
```

**Rule:** When opening a dialog, always auto-focus the first interactive element (input or primary button).

### Form Accessibility

The `Field` component from `@/components/ui/field` auto-links labels, errors, and descriptions:

```tsx
// Field auto-generates:
// - <Label htmlFor={fieldId}>
// - <input id={fieldId} aria-invalid={hasError} aria-describedby={fieldId}-description>
// - Error message with id={fieldId}-description

<Field label="Name" error={errors.name} required>
  <Input />
</Field>
```

**Rules:**
- ALWAYS use `Field` wrapper — it handles `htmlFor`, `aria-invalid`, `aria-describedby` automatically
- NEVER create bare `<input>` without associated `<label>`
- Required field indicator (`*`) uses `aria-hidden="true"` — the `required` attribute on the input conveys this to screen readers

### ARIA Attribute Usage

| Attribute | When to Use | Example |
|-----------|-------------|---------|
| `aria-label` | Icon-only buttons without visible text | `<Button aria-label="Delete">` |
| `aria-hidden="true"` | Decorative elements, visual-only indicators | Required `*` marker, close icon |
| `aria-invalid` | Form fields with validation errors | Auto-handled by `Field` |
| `aria-describedby` | Link field to error/help text | Auto-handled by `Field` |
| `aria-expanded` | Collapsible sections, dropdowns | `<FormSection>` uses this |
| `role="button"` | Non-`<button>` clickable elements | `<div role="button" tabIndex={0}>` |

### Screen Reader Patterns

```tsx
// Hidden text for screen readers (visually hidden)
<span className="sr-only">{t('common:close')}</span>

// Image alt text — always provide meaningful description
<img src={icon} alt={t('devices:detailPage.preview')} />

// Decorative icons — use aria-hidden
<Clock className="h-4 w-4" aria-hidden="true" />
```

**Rules:**
- Icon-only buttons MUST have `aria-label` or a `<span className="sr-only">` label
- Decorative images/icons use `aria-hidden="true"`
- Content images MUST have meaningful `alt` text
- NEVER leave `alt` attribute empty on content images

### Keyboard Navigation

| Key | Behavior |
|-----|----------|
| `Escape` | Close dialogs, popovers, dropdowns (Radix built-in) |
| `Tab` / `Shift+Tab` | Move focus; wraps inside open dialogs (tab trap) |
| `Enter` | Activate buttons, submit forms |
| `Arrow keys` | Navigate within Select, DropdownMenu, Tabs, Slider |

### Touch Accessibility

- **Minimum touch target**: 44x44px for primary actions
- **Icon buttons**: Default `h-10 w-10` (40x40px) — acceptable for secondary actions
- **Interactive spacing**: `gap-1.5` minimum between adjacent touch targets
- **Swipe gestures**: TopNav mobile tab bar supports left/right swipe (50px threshold)

### Color Accessibility

- OKLCH color space ensures perceptual uniformity across light/dark themes
- Status colors (`text-success`, `text-error`, `text-warning`, `text-info`) designed with contrast in mind
- **NEVER** rely on color alone to convey information — always pair with text label or icon
- Chart colors (`--chart-1` through `--chart-6`) use distinct lightness values for distinguishability

---

## Quick Reference: File Locations

| Resource | Path |
|----------|------|
| CSS Variables | `web/src/index.css` |
| Tailwind Config | `web/tailwind.config.js` |
| Page Layout | `web/src/components/layout/PageLayout.tsx` |
| Shared Components | `web/src/components/shared/` |
| UI Primitives | `web/src/components/ui/` |
| Form Dialog | `web/src/components/dialog/UnifiedFormDialog.tsx` |
| Full Screen Dialog | `web/src/components/automation/dialog/FullScreenDialog.tsx` |
| Portal Utility | `web/src/lib/portal.tsx` |
| Error Handling | `web/src/lib/errors.ts` |
| Error Hook | `web/src/hooks/useErrorHandler.ts` |
| Fetch Cache | `web/src/lib/utils/async.ts` |
| Status Colors | `web/src/design-system/utils/format.ts` |
| i18n Locales | `web/src/i18n/locales/{en,zh}/` |
| Mobile Hooks | `web/src/hooks/useMobile.ts` |
| Form Hook | `web/src/hooks/useForm.ts` |
| Dialog Hook | `web/src/hooks/useDialog.ts` |
| Infinite Scroll | `web/src/hooks/useInfiniteScroll.ts` |
| Store Slices | `web/src/store/slices/` |
| API Client | `web/src/lib/api.ts` |
| URL Management | `web/src/lib/urls.ts` |
| Auth Module | `web/src/lib/auth.ts` |
| Auth Store | `web/src/store/slices/authSlice.ts` |
| Chat WebSocket | `web/src/lib/websocket.ts` |
| Extension Stream | `web/src/lib/extension-stream.ts` |
| Event System | `web/src/lib/events.ts` |
| Events Hook | `web/src/hooks/useEvents.ts` |
| Time Utilities | `web/src/lib/time.ts` |
| Time Format Hook | `web/src/hooks/useTimeFormat.ts` |
| Data Source Hook | `web/src/hooks/useDataSource.ts` |
| LoadingState | `web/src/components/shared/LoadingState.tsx` |
| Instance Store | `web/src/store/slices/instanceSlice.ts` |
| Routes | `web/src/App.tsx` |
| Pages | `web/src/pages/` |
| Icon System | `web/src/design-system/icons/index.tsx` |
| Theme Provider | `web/src/components/ui/theme.tsx` |
| Theme Toggle | `web/src/components/layout/ThemeToggle.tsx` |
| App Sidebar | `web/src/components/layout/AppSidebar.tsx` |
| Top Bar | `web/src/components/layout/TopBar.tsx` |
| Nav Items (shared) | `web/src/components/layout/navItems.ts` |
| Toast Hook | `web/src/components/ui/use-toast.ts` |
| Toast Component | `web/src/components/ui/toast.tsx` |
| Global Notify | `web/src/lib/notify.ts` |
| Chart Components | `web/src/components/dashboard/generic/` |
