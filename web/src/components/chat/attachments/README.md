# Chat Reply Attachments & Inspector Standard

Structured interactive attachments rendered beneath assistant messages in NeoMind Chat (`image`, `device`, `rule`, `transform`, `agent`, `dashboard`), together with their unified 3-host detail inspector.

---

## 1. Architecture Overview

| Layer | Responsibility | Key Files |
|---|---|---|
| **Backend extraction** | Extract verified attachments from successful tool results, dedupe/merge per turn, hydrate against `ServerState`, drop non-existent IDs (unless `deleted`). Never parse LLM prose. | `crates/neomind-api/src/handlers/chat_attachments.rs` |
| **Message rendering** | Render `media` (images), `card` (write/execute outcomes), and `chip` (read-only references) beneath assistant replies. | `web/src/components/chat/attachments/MessageAttachments.tsx` |
| **Inspector hosts** | Zero-duplication responsive hosts: desktop `/chat` docked split pane (`EntityInspectorPane`), desktop overlay drawer (`Sheet`), mobile full-screen (`UnifiedFormDialog`). **New kinds require zero host changes.** | `EntityInspectorPane.tsx`, `EntityDetailDialog.tsx` |
| **Inspector data & views** | Fetch live entity data (falling back to historical `snapshot` on 404/deleted) and dispatch to per-kind view components. | `EntityInspectorContent.tsx`, `inspector/shared.tsx`, `inspector/<Kind>InspectorView.tsx` |

---

## 2. Attachment Display Rules (`media` / `card` / `chip`)

- **`media` (`image`)**:
  - Single image: frameless, aspect-ratio-hugging (`max-h-72 max-w-[min(100%,420px)]`), no double border, no raw UUID footer strip; hover-only translucent download button and meaningful caption.
  - Multiple images: compact `grid-cols-2 max-w-md` with `h-40 object-cover`.
  - Images already inlined in markdown (`![...](url)`) are automatically deduped on the backend.
- **`card`** (`created` / `updated` / `executed`):
  - Compact rounded card (`bg-card border-border py-2`), tinted icon box, title, action/status badge, and concise subtitle.
  - **No diagonal arrows (`↗`)** on cards or chips.
- **`chip`** (`queried` / `deleted`):
  - Compact `h-7` (`28px`) pill (`bg-card border-border min-h-0 min-w-0`), icon + title + subtle status dot.
  - `list` tool outputs must never explode into per-item cards.

---

## 3. Icon & Color Restraint Standard

Always use NeoMind semantic tokens — **never** raw Tailwind palettes (`bg-blue-500`) and **never** CSS-variable `/opacity` modifiers (`bg-card/70`, `border-border/80`).

- **Monochrome-first icon containers**: Cards, chips, and inspector headers use neutral containers (`bg-muted-30 border border-border text-foreground`) with crisp `strokeWidth={1.75}` Lucide icons. Reserve color for real semantic status (`bg-success`, `bg-warning`, `bg-error`) and restrained inline `@mention` tokens.

| Kind | Icon (`lucide-react`) | `@mention` Tint Token |
|---|---|---|
| `device` | `Box` | `text-accent-cyan` |
| `rule` | `Zap` | `text-accent-orange` |
| `transform` | `Workflow` | `text-accent-purple` |
| `agent` | `Sparkles` | `text-accent-emerald` |
| `dashboard` | `LayoutGrid` | `text-accent-indigo` |

---

## 4. Inspector View Design Rules

1. **Header**: Title only (`title` + icon + Refresh + Close). **Never** add a subtitle/description line below the title (project-wide dialog & drawer standard).
2. **Read & verify first**: Keep the side panel focused on clarity, structure, and quick verification rather than heavy write controls. Reserve complex editing for the full page (`前往 XX 页`) and follow-up edits for `引用到聊天` (`@mention`).
3. **Standard layout rhythm** (reuse `inspector/shared.tsx`):
   - **Hero Summary Card**: Status badge on left + `<CopyIdPill />` on right, followed by a 2–3 column `<InfoCell />` metadata grid.
   - **Segmented Tabs** (`<SegmentedTabs />`): Use 2 lightweight tabs when an entity has both primary structure/state and secondary history/commands/list (e.g., `实时数据 / 支持命令`, `规则逻辑 / 触发记录`, `最新结论 / 配置与历史`, `布局预览 / 组件清单`). Omit tab chrome if the secondary view is empty/unnecessary.
   - **Deleted / 404 fallback**: Handled centrally by `EntityInspectorContent` via `<SnapshotKV snapshot={attachment.snapshot} />`.
   - **State reset**: Always mount `<KindInspectorView key={entityId} ... />` so switching entities resets internal tabs/filters cleanly.
4. **Reuse domain components** instead of raw text/JSON dumps (e.g., `DeviceStatusBadge`, `ZoomableImage`, `RuleWhyNotTriggeredPanel`, `ConclusionContent`, `TransformTestDialog`).
5. **Footer**: Standard actions only — `引用到聊天` (when `onAttachReference` is provided), optional safe diagnostic trigger (e.g., Transform `试运行`), and `前往 XX 页` (`deepLink`, validated as safe same-origin `/...` path).

---

## 5. Checklist: Adding a New Card / Entity Kind

### A. Reply Card + Side Inspector (Required)
1. **Backend (`crates/neomind-api/src/handlers/chat_attachments.rs`)**:
   - Add variant to `ChatAttachmentKind` and `as_str()`.
   - Map tool calls (`tool_name` + action) to `ChatAttachmentCandidate` (`card` for mutations/executions, `chip` for reads).
   - Hydrate against `ServerState` to populate verified `title`, `status`, `deep_link`, and fallback `snapshot`.
   - Add unit tests covering extraction, dedupe/merge, and non-existent ID filtering.
2. **Frontend Types (`web/src/types/chat.ts`)**:
   - Add the new kind to `ChatAttachmentKind`.
3. **Card / Chip Meta (`web/src/components/chat/attachments/EntityInspectorContent.tsx` & `MessageAttachments.tsx`)**:
   - Register icon, accent tokens, and ZH/EN labels in `getEntityKindMeta()`.
   - Ensure `MessageAttachments.tsx` renders its icon/accent cleanly.
4. **Inspector View (`web/src/components/chat/attachments/inspector/<Kind>InspectorView.tsx`)**:
   - Build the view using `CopyIdPill`, `SegmentedTabs`, `InfoCell` from `inspector/shared.tsx`.
   - Wire data loading (always pass `{ skipErrorToast: true }` so background/404 loads never pop global error toasts) and view dispatch in `EntityInspectorContent.tsx`.
5. **Tests**:
   - Add/update tests in `web/src/components/chat/__tests__/MessageAttachments.test.tsx`.
   - Run `cd web && npx tsc --noEmit` and `cargo test -p neomind-api --lib chat_attachments`.

### B. `@mention` Support (Optional — if users should reference it in chat)
1. **Backend**:
   - `crates/neomind-api/src/handlers/mentions.rs`: add category & search handler.
   - `crates/neomind-api/src/handlers/chat_refs.rs` & `models/mod.rs`: resolve `ChatReference` into grounded context.
2. **Frontend**:
   - `web/src/types/chat.ts`: add to `ChatReferenceType` / `MentionCategory`.
   - `web/src/components/chat/mentionChipStyles.ts`: assign matching accent color style.
   - `web/src/lib/api/mentions.ts`, `web/src/components/chat/ChatComposer.tsx`, `web/src/components/chat/composer/inlineDoc.ts` / `contentProtocol.ts` / `docBridge.ts`: wire filter tab & serialization.
   - `web/src/i18n/locales/{zh,en}/chat.json`: add localized tab & placeholder labels.
   - Update `web/src/components/chat/__tests__/ChatComposerMention.test.tsx`.
