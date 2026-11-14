# §12 C — production TipTap inline `@` (2026-11-14)

## Scope
Replace production ChatComposer trailing chips with TipTap Mention (B-slice recommendation D).

## Behavior
- When `onReferencesChange` is set, ChatComposer mounts `InlineMentionEditor` (TipTap + `EntityMention`).
- `@` trigger (typing or toolbar) opens the existing server-search listbox (type filters / pagination / a11y unchanged).
- Selecting inserts an atomic mention; the same `type+id` may appear more than once in one sentence.
- `onPayloadChange` projects plain text + unique legacy `references` + ordered `contentNodes` for send/persist.
- Hosts without mention handlers keep the classic textarea (legacy path).

## Packages
`@tiptap/react` / `starter-kit` / `extension-mention` / `extension-placeholder` / `suggestion` @ 3.34.0.

## Gates
```bash
cd web && npx vitest run \
  src/components/chat/inline-mention \
  src/components/chat/__tests__/ChatComposerMention.test.tsx \
  src/lib/chat/__tests__/contentProtocol.test.ts \
  src/prototypes/inline-at
# 35/35
npx tsc --noEmit
```

## Not in this slice
- Headed Playwright / real touch / VoiceOver / 5-person usability.
- History bubble re-render of persisted contentNodes as inline chips.
- Live Spark / 138 (untouched).

## Next
1. History message display from `contentNodes`.
2. C a11y checklist headed + 5×4 tasks.
3. Live pairing after 138 releases Spark.
