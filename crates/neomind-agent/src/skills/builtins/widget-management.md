---
id: widget-management
name: Widget Management (Install/Market/List)
category: widget
origin: builtin
priority: 90
token_budget: 4500
triggers:
  keywords: [widget list, widget get, widget bundle, widget install, widget uninstall, widget market, widget market-install, widget market-list, dashboard widget, 安装组件, 卸载组件, 组件列表, 组件市场, 市场组件, 获取组件, neomind widget]
  tool_target:
    - tool: shell
      actions: [list, get, bundle, install, uninstall, market-install, market-list]
anti_triggers:
  keywords: [create widget, scaffold widget, IIFE, bundle.js, React, 开发组件, 开发 widget, manifest.json, dashboard component develop]
---

# Widget Management (Install / Market / List)

Manage dashboard widgets and the widget marketplace. (For *developing* a widget from scratch, see the `widget-development` skill.)

## CRITICAL Rules

1. **Install from a local scaffold/zip** → `neomind widget install <path>`. **Install from the marketplace** → `neomind widget market-install <id>` — different commands.
2. **`neomind widget bundle <id>`** returns the widget's JS/CSS bundle (for inspection/embedding) — NOT `file_write`/`file_read`/`web_fetch`.
3. **`neomind widget market-list`** lists marketplace widgets; **`neomind widget list`** lists already-installed ones.
4. RUN the command yourself and report the output — don't narrate.

## Command Cheat-Sheet

| Command | Purpose |
|---|---|
| `neomind widget list` | List installed widgets |
| `neomind widget get <id>` | Show widget details |
| `neomind widget bundle <id>` | Get a widget's bundle (JS/CSS) |
| `neomind widget install <path>` | Install from a scaffolded dir or `.zip` |
| `neomind widget uninstall <id>` | Uninstall a widget |
| `neomind widget market-list` | List widgets available in the marketplace |
| `neomind widget market-install <id>` | Install a widget from the marketplace |
| `neomind widget create` | Scaffold a NEW widget (dev — see `widget-development` skill) |

### Examples

```bash
neomind widget list
neomind widget market-list
neomind widget market-install gauge-chart
neomind widget bundle gauge-chart
neomind widget get gauge-chart
neomind widget install ./my-widget/
neomind widget uninstall gauge-chart
```
