---
id: extension-management
name: Extension Management (Install/Market/Status)
category: extension
origin: builtin
priority: 90
token_budget: 4500
triggers:
  keywords: [extension install, extension list, extension market, marketplace, market-install, market-list, extension status, extension logs, extension get, extension validate, extension config, extension reload, extension uninstall, 安装扩展, 卸载扩展, 扩展列表, 扩展市场, 扩展状态, 扩展日志, 扩展配置, neomind extension]
  tool_target:
    - tool: shell
      actions: [install, uninstall, list, get, status, logs, validate, config, reload, market-install, market-list]
anti_triggers:
  keywords: [create extension, build extension, extension sdk, neomind_export, FFI, Rust, 扩展开发, 开发扩展, scaffold, manifest.json]
---

# Extension Management (Install / Market / Status)

Manage installed extensions and the marketplace. (For *developing/building* an extension from source, see the `extension-development` skill.)

## CRITICAL Rules

1. **Install from a local `.nep` file** → `neomind extension install <path.nep>`. **Install from the marketplace** → `neomind extension market-install <id>` — these are different commands.
2. **Always `validate` a local `.nep` before `install`** — `neomind extension validate <path.nep>`.
3. **RUN the command yourself and report the output** — don't just tell the user to run it.
4. After install/reload, check health with `neomind extension status <id>`; debug with `neomind extension logs <id>`.

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
| `neomind extension market-list` | List extensions available in the marketplace |
| `neomind extension market-install <id>` | Install an extension from the marketplace |
| `neomind extension create` | Scaffold a NEW extension (dev — see `extension-development` skill) |
| `neomind extension build` | Build extension from source (dev — see `extension-development` skill) |

### Examples

```bash
neomind extension list
neomind extension market-list
neomind extension market-install weather
neomind extension validate ./my-ext-1.0.0.nep
neomind extension install ./my-ext-1.0.0.nep
neomind extension status weather
neomind extension logs weather
neomind extension config weather
```
