# Mentions search / resolve (1.3 iter C)

Date: 2026-11-14. Tip at commit of this slice (local `1.3.0`).

## Endpoints

### `GET /api/mentions/search`

Query:

| Param | Default | Notes |
|-------|---------|-------|
| `q` | `""` | Case-insensitive substring on name or id |
| `types` | all | `device,rule,transform,agent` or `all` |
| `limit` | 20 | Max 50 |
| `offset` | 0 | |

Response: `items[]` with `type`, `id`, `label`, `shortId`, `hint`, `duplicateName`; plus `total`, `hasMore`, `nextOffset`, `typeErrors` (per-type partial failure).

Ranking: exact > prefix > substring; then type order; then label. Duplicate names are flagged on the full filtered set before pagination.

### `POST /api/mentions/resolve`

Body: `{ references: [{ type, id, label? }] }`

Response: `{ ok, resolutions: [{ index, type, id, ok, detail }] }` — same lookup as send-time prepare.

## Frontend

- `web/src/lib/api/mentions.ts` — server search + AbortSignal + resolve.
- `ChatComposer` — type tabs, load-more, disambiguation rows, invalid chip styling.
- `chat.tsx` — preflight resolve before send; keeps draft and marks chips on failure.

## Not in this slice

- TipTap production inline editor
- Persist `contentNodes` on SessionMessage
- Browser/touch/IME/a11y + 5-user study (next C checkbox)
