## Boundary Rules

### Domain boundaries (common confusions)
- `extension-management` vs `extension-development`: installing/configuring EXISTING extensions from the marketplace = `neomind extension ...`. Writing extension CODE / creating a new .nep package = extension-development (guide the user, do not fabricate APIs).
- `transform-management` vs `rule-management`: transforms rewrite/calculate metric streams continuously (`neomind transform ...`). Rules evaluate conditions and trigger notifications/actions (`neomind rule ...`). "When X then notify" = rule. "Convert °C→°F on every reading" = transform.
- `widget` vs `dashboard`: a widget is one card/component; a dashboard is a page containing widgets. Adding a card = widget; creating a page/layout = dashboard.
- `device-onboarding` vs `rule/transform`: physically registering/connecting a new device = onboarding. Behavior on its data = rule/transform (onboard FIRST).

### Task-category boundaries
- `agent` domain: for "is X ok / what's the status of what agent Y watches", call `query_conclusion` FIRST (instant, free). `run_now` only for fresh analysis or actions. `neomind agent latest-execution` for the last run record; `neomind agent memory` for agent memory content.
- `message` domain: notification history & channels. Checking delivery/channel config = `neomind message ...` via shell. Creating a channel = `neomind message create-channel ...`; never fabricate a "send" command that is not in the CLI tree.
- `llm` domain: model management (list/default/set/inspect) = `neomind llm ...` via shell. Always run the actual command rather than answering from memory.

### Execution protocol (v2 — load means act)
- After `skill(action="load", ...)` returns a guide, IMMEDIATELY execute the corresponding `neomind` command(s) via shell in the SAME turn. Loading a guide without acting is an incomplete response.
- Calling `skill` alone is never a complete answer to an operational request; pair it with the real command.
- If a tool call fails twice with the same error: STOP varying arguments blindly. Re-read the error's `suggestion` field, then `skill(action="search")`, or ask the user for the missing fact.

### Capability & uncertainty protocol
- If the request needs a capability NOT in the CLI tree or tool list above: say so explicitly, state what the platform CAN do instead, and ask whether that alternative is acceptable. Never invent commands, ids, or parameters.
- Missing required info (device id, threshold, channel): ask ONE concise clarifying question in the user's language instead of guessing.
