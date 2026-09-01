---
id: message-management
name: Message Management & Channel Configuration
description: Use when the user wants to manage messages and notification channels — sending messages, configuring channels (webhook/email/Telegram/DingTalk/Feishu/Slack), alerts, acknowledge, severity. Covers message send/channel-create/channel-get/channel-update/channel-test/channel-delete/channel-type-schema even without saying 'message' (e.g. '发个通知到飞书', '告警发我邮箱'). Includes 消息/通知/通道/告警. NOT external MQTT brokers — use connector-management for those.
category: message
origin: builtin
priority: 80
token_budget: 8000
triggers:
  keywords: [message, 消息, 通知, notification, channel, 通道, alert, 告警, 报警, send message, 发送消息, webhook channel, email channel, 邮件通道, 消息管理, severity, 严重程度, acknowledge, 确认, message channel, telegram, 钉钉, dingtalk, wecom, 企业微信, slack, feishu, 飞书, 飞书通知]
  tool_target:
    - tool: message
      actions: [list, get, send, read, ack, channel-list, channel-get, channel-create, channel-update, channel-delete, channel-test, channel-types, channel-type-schema]
anti_triggers:
  keywords: [dashboard, 仪表盘, agent, 代理, extension develop, 扩展开发, rule, 规则, device connect, 设备连接, push, 推送, data push]
---

# Message Management & Channel Configuration

Messages are platform notifications delivered through configurable channels. 7 channel types are supported: webhook, email, telegram, wecom, dingtalk, slack, feishu.

## Command Cheat-Sheet (run these via `shell`)

Always RUN the command yourself and report the real output.

| Command | Purpose |
|---|---|
| `neomind message list` | List messages |
| `neomind message get <id>` | Message details |
| `neomind message send` | Send a new message |
| `neomind message read <id>` | Acknowledge/read a message (alias: `ack`) |
| `neomind message channel-list` | List message channels |
| `neomind message channel-get <id>` | Channel details |
| `neomind message channel-types` | List available channel types |
| `neomind message channel-type-schema <TYPE>` | Config schema + examples for a channel type |
| `neomind message channel-create` | Create a message channel |
| `neomind message channel-update <id>` | Update channel configuration |
| `neomind message channel-delete <id>` | Delete a channel |
| `neomind message channel-test <id>` | Test a message channel |

## CRITICAL Rules

1. **`--title` and `--body` are required** for sending — not `--channel`
2. **Severity values**: `info`, `warning`, `critical`, `emergency`
3. **Channels must be created before rules can use them** for notifications
4. **Always discover types dynamically**: `channel-types` → `channel-type-schema <TYPE>` → `channel-create`
5. **Always test a channel** after creation with `message channel-test <NAME>`

## Command Reference

### Send Message

```bash
neomind message send --title '<TITLE>' --body '<TEXT>' [--severity <LEVEL>] [--source <SRC>]
```

| Flag | Required | Default | Description |
|------|----------|---------|-------------|
| `--title` | Yes | — | Message title |
| `--body` | Yes | — | Message body text |
| `--severity` | No | info | Severity: `info`, `warning`, `critical`, `emergency` |
| `--source` | No | — | Source identifier |

### List & Read Messages

```bash
neomind message list                              # List all messages
neomind message list --severity warning --limit 20 # Filtered list
neomind message get <ID>                          # Get message details
neomind message read <ID>                         # Mark as read (alias: ack)
```

### Channel Management

```bash
neomind message channel-types                     # List available channel types
neomind message channel-type-schema <TYPE>        # Get config schema for a type
neomind message channel-list                      # List all channels
neomind message channel-get <NAME>                # Get channel details
neomind message channel-create --name <N> --type <T> --param <k>=<v>...  # Create channel (flags, preferred)
neomind message channel-create --name <N> --type <T> --config '<JSON>'  # Create channel (full JSON form)
neomind message channel-update <NAME> --config '<JSON>'                 # Update channel config
neomind message channel-delete <NAME>             # Delete channel
neomind message channel-test <NAME>               # Test channel delivery
```

## Channel Type Discovery Workflow

**ALWAYS follow this workflow** when creating a channel for a user:

```bash
# Step 1: Discover available types
neomind message channel-types

# Step 2: Get config schema for the desired type
neomind message channel-type-schema telegram

# Step 3: Create the channel with proper config — prefer flag form, one --param per field
neomind message channel-create --name my-telegram --type telegram --param token=123:ABC --param chat_id=99887
# (equivalent JSON form: --config '{"token":"123:ABC","chat_id":"99887"}')

# Step 4: Test the channel
neomind message channel-test my-telegram
```

## Channel Types & Config Examples

### Webhook (HTTP POST)

```bash
neomind message channel-create --name webhook-alerts --type webhook \
  --config '{"url": "https://example.com/webhook", "headers": {"Authorization": "Bearer TOKEN"}, "timeout_secs": 30}'
```

**Required fields**: `url`
**Optional fields**: `headers` (object), `timeout_secs` (number)

### Email (SMTP)

```bash
neomind message channel-create --name email-alerts --type email \
  --config '{"smtp_server": "smtp.example.com", "smtp_port": 587, "username": "user@example.com", "password": "pass", "from_address": "noreply@example.com", "use_tls": true}'
```

**Required fields**: `smtp_server`, `username`, `password`, `from_address`
**Optional fields**: `smtp_port` (default 587), `use_tls` (default true)

### Telegram Bot

```bash
neomind message channel-create --name tg-alerts --type telegram \
  --config '{"token": "123456:ABCdefGHIjklMNO", "chat_id": "-1001234567890"}'
```

**Required fields**: `token`, `chat_id`
**How to get**: Create bot via @BotFather, get chat_id via `https://api.telegram.org/bot<TOKEN>/getUpdates`

### WeCom (企业微信) Webhook

```bash
neomind message channel-create --name wecom-alerts --type wecom \
  --config '{"key": "xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx"}'
```

**Required fields**: `key`
**How to get**: Group robot settings in WeCom admin console

### DingTalk (钉钉) Robot

```bash
neomind message channel-create --name dingtalk-alerts --type dingtalk \
  --config '{"access_token": "xxxx", "secret": "SECxxxx"}'
```

**Required fields**: `access_token`
**Optional fields**: `secret` (for sign verification)

### Slack Webhook

```bash
neomind message channel-create --name slack-alerts --type slack \
  --config '{"webhook_url": "https://hooks.slack.com/services/T00/B00/xxx"}'
```

**Required fields**: `webhook_url`

### Feishu (飞书) Webhook

```bash
neomind message channel-create --name feishu-alerts --type feishu \
  --config '{"hook_id": "xxxxxxxx", "secret": "optional_sign_secret"}'
```

**Required fields**: `hook_id`
**Optional fields**: `secret` (for sign verification)

## Workflow Examples

### Set Up DingTalk Notification Channel

```bash
# Step 1: Verify dingtalk is available
neomind message channel-types

# Step 2: Get config schema
neomind message channel-type-schema dingtalk

# Step 3: Create the channel
neomind message channel-create --name dingtalk-ops --type dingtalk \
  --config '{"access_token": "your-token", "secret": "SECyour-secret"}'

# Step 4: Test delivery
neomind message channel-test dingtalk-ops

# Step 5: Send a test message
neomind message send --title 'Test' --body 'DingTalk channel setup complete' --severity info
```

### Send Warning Alert

```bash
neomind message send --title 'Low Battery' \
  --body 'Sensor-001 battery at 15%' \
  --severity warning \
  --source sensor-001
```

### Send Emergency Alert

```bash
neomind message send --title 'System Overheating' \
  --body 'Temperature sensor reads 95°C — immediate action required' \
  --severity emergency
```

### Review & Acknowledge Messages

```bash
# List unread warnings and critical messages
neomind message list --severity warning
neomind message list --severity critical

# Acknowledge a message
neomind message read <ID>
```

### Update Channel Config

```bash
neomind message channel-update alerts \
  --config '{"url": "https://new-hook.example.com/alert", "headers": {"Authorization": "Bearer new-token"}}'
```

### Delete a Channel

```bash
neomind message channel-delete alerts
```

## How Rule Notifications Reach Channels (CRITICAL)

When a rule's `notify` action fires, it creates a **Message** in the message system. That message is then delivered to **ALL configured channels** that match the channel's filter.

**Key insight**: By default, every channel has an empty filter = **receives ALL messages**. So the most common IoT workflow is just 2 steps:

```bash
# Step 1: Create a notification channel (receives all messages by default)
neomind message channel-create --name telegram-alerts --type telegram \
  --config '{"token": "BOT_TOKEN", "chat_id": "CHAT_ID"}'

# Step 2: Create a rule with a notify action — that's it!
neomind rule create --body '{"name":"High Temp Alert","condition":{"condition_type":"comparison","source":"device:sensor-001:temperature","operator":"greater_than","threshold":35},"actions":[{"type":"notify","message":"Temperature {value}°C on sensor-001","severity":"critical"}]}'
# → When temp > 35, the rule creates a critical message → delivered to telegram-alerts automatically
```

**Severity matters**: The rule's `severity` field determines the message severity. Channels with a `min_severity` filter will only receive messages at or above that level. By default (no filter), all severities are delivered.

### Channel Filters (Advanced Routing)

Channel filters allow routing specific messages to specific channels. **There is no CLI command for filters** — they are configured via the web UI or API (`PUT /api/messages/channels/:name/filter`).

Filter fields:
| Field | Effect | Example |
|-------|--------|---------|
| `min_severity` | Only receive messages at or above this severity | `"warning"` → receives warning, critical, emergency |
| `source_types` | Only receive messages from these sources | `["rule"]` → only rule-triggered messages |
| `categories` | Only receive messages in these categories | `["alert"]` |

Empty array / null = accept all for that field.

## Common Errors & Solutions

| Error | Cause | Solution |
|-------|-------|----------|
| "Channel name is required" | Missing --name | Use `--name <NAME>` |
| "Unknown channel type 'discord'" | Unsupported type | Run `message channel-types` for available types |
| "Invalid config JSON" | Malformed JSON in --config | Validate JSON syntax. Run `channel-type-schema <TYPE>` for examples |
| "Missing required field: title" | Using wrong flag names | Use `--title` and `--body` (not `--channel`) |
| "Channel not found" | Wrong channel name | Run `message channel-list` for valid names |
| Webhook not delivering | Wrong URL or network issue | Test with `message channel-test <NAME>` |
| "SMTP auth failed" | Wrong email credentials | Verify SMTP settings and update with `channel-update` |
| "Message not found" | Wrong message ID | Run `message list` for valid IDs |
| Telegram "Unauthorized" | Invalid bot token | Verify token from @BotFather, update with `channel-update` |
