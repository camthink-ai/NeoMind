# §12 C — history contentNodes inline chip redraw (2026-11-14)

## Scope
Re-render persisted / optimistic user messages as in-sentence chips from
`contentNodes`, plus fix-and-resend (“reuse in composer”) and invalid-send draft
save. Tip start `20ba6ca6` on branch `1.3.0`; no side branch / push; 138 untouched.

## Behavior
- `UserMessageContent`: if nodes include any `reference`, paint ordered text +
  `.nm-mention` chips (same visual family as TipTap); otherwise keep
  `MarkdownMessage`.
- `mergeMessagesForDisplay` normalizes `contentNodes` / `content_nodes` and
  `labelSnapshot` / `label_snapshot` so history reload redraws chips.
- Optimistic echo already carries `contentNodes` on send — same path.
- Pencil action → `onReuseUserMessage` restores plain text + unique legacy
  refs + ordered nodes into ChatComposer / PanelChatView and `saveSessionDraft`.
- Invalid mention preflight keeps the editable draft and persists it to
  localStorage for refresh recovery.

## Gates
```bash
cd web && npx vitest run \
  src/components/chat/inline-mention \
  src/components/chat/__tests__/ChatComposerMention.test.tsx \
  src/components/chat/__tests__/ChatMessages.test.tsx \
  src/lib/chat/__tests__/contentProtocol.test.ts \
  src/lib/__tests__/messageUtils.test.ts \
  src/prototypes/inline-at
# 54/54 (jsdom getClientRects TipTap noise may still print; tests pass)
npx tsc --noEmit
```

## Not in this slice
- Headed Playwright / real touch / VoiceOver / 5-person usability.
- Live Spark / 138 (untouched).

## Next
1. C headed a11y + 5×4 tasks (≥18/20).
2. Live pairing after 138 releases Spark.
3. D / E / F remaining plan items.
