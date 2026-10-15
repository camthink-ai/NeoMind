---
id: extension-management
name: Extension Management (Install/Market/Status)
description: Use when the user wants to INSTALL or MANAGE existing extensions — installing/uninstalling from the marketplace, listing, status, logs, config, reload. NOT for writing extension source code (that's extension-development). Covers install/market requests even without the word 'extension' (e.g. 'install the weather extension', '装个天气扩展'). FIRST check whether it's already installed — installed extensions may simply need configuration.
category: extension
origin: builtin
priority: 90
token_budget: 4500
triggers:
  keywords: [extension install, install extension, add extension, extension market, marketplace, market install, market-list, install from market, install weather extension, weather extension, market-install, extension status, extension logs, extension get, extension validate, extension config, extension reload, extension uninstall, already installed, 安装扩展, 卸载扩展, 扩展列表, 扩展市场, 扩展状态, 扩展日志, 扩展配置, 已经安装, neomind extension]
  tool_target:
    - tool: shell
      actions: [install, uninstall, list, get, status, logs, validate, config, reload, market-install, market-list]
anti_triggers:
  keywords: [create extension, build extension, extension sdk, neomind_export, FFI, Rust, 扩展开发, 开发扩展, scaffold, manifest.json]
---

# Extension Management (Install / Market / Status)

Manage installed extensions and the marketplace. (For *developing/building* an extension from source, see the `extension-development` skill.)

## CRITICAL Rules

1. **CHECK FIRST — it may already be installed**: run `neomind extension list` and look for the requested id. If present, DO NOT install anything — tell the user it's installed (show version), and offer `neomind extension config <id>` if it needs a key or setting. Installing over an existing id is not an idempotent no-op.
2. **Install from a local `.nep` file** → `neomind extension install <path.nep>`. **Install from the marketplace** → `neomind extension market-install <id>` — these are different commands.
3. **Always `validate` a local `.nep` before `install`** — `neomind extension validate <path.nep>`.
4. **RUN the command yourself and report the output** — don't just tell the user to run it.
5. After install/reload, check health with `neomind extension status <id>`; debug with `neomind extension logs <id>`.
6. **Marketplace install is ALWAYS exactly three commands, run each ONCE:**
   1. `neomind extension market-list <用户要的东西>` — pick the exact `id`
   2. `neomind extension market-install <id>`
   3. `neomind extension list` — confirm it appears; report success to the user
   Do NOT re-run `market-list` (the output is large), do NOT load this skill twice, and do NOT summarize the market listing — the user asked to INSTALL, so install.

## Command Cheat-Sheet

| Command | Purpose |
|---|---|
| `neomind extension list` | List installed extensions |
| `neomind extension get <id>` | Show one extension's info (alias: `info`) |
| `neomind extension status <id>` | Extension health/status |
| `neomind extension logs <id>` | Extension logs (debug crashes/errors) |
| `neomind extension config <id>` | Get/set extension configuration |
| `neomind extension reload <id>` | Reload an extension after changes |
| `neomind extension validate <path.nep>` | Validate a local `.nep` package before install |
| `neomind extension install <path.nep>` | Install a local `.nep` package |
| `neomind extension uninstall <id>` | Uninstall an extension |
| `neomind extension market-list <关键词>` | Search the marketplace (compact: id/name/version). ALWAYS give a keyword — the unfiltered dump is huge. `-v` for full metadata |
| `neomind extension market-install <id>` | Install an extension from the marketplace |
| `neomind extension create` | Scaffold a NEW extension (dev — see `extension-development` skill) |
| `neomind extension build` | Build extension from source (dev — see `extension-development` skill) |

### Examples

```bash
neomind extension list
neomind extension market-list weather     # compact, filtered
neomind extension market-install weather-forecast   # install the EXACT id shown
neomind extension validate ./my-ext-1.0.0.nep
neomind extension install ./my-ext-1.0.0.nep
neomind extension status weather
neomind extension logs weather
neomind extension config weather
```
