#!/usr/bin/env bash
# IM 端到端冒烟 —— 真 bot token、真手机、真通知。
#
# 自动化测试能覆盖到 bridge↔router 的接线（见 `im_bridge/telegram.rs` 的
# `a_telegram_message_survives_the_whole_round_trip`），但盖不住三件事：
# Telegram 对 token 的校验、真人扫码 /start、以及通知真的出现在手机上。
# 这个脚本把必须人工的两步留给你，其余全自动，并在等待时轮询服务端状态，
# 而不是让你盯着屏幕猜。
#
# Usage:
#   scripts/im-smoke.sh --token <BOT_TOKEN>
#   scripts/im-smoke.sh --token <BOT_TOKEN> --api http://localhost:9375 --api-key <KEY>
#
# 前置：服务在跑（scripts/smoke-serve.sh）
#
# 退出码：0 全部通过 / 1 某一步失败 / 2 用法错误
set -uo pipefail

API="http://localhost:9375"
TOKEN=""
API_KEY="${NEOMIND_API_KEY:-}"
WAIT_SECS=180
BIND_TIMEOUT=300

usage() {
  sed -n '2,20p' "$0" | sed 's/^# \{0,1\}//'
  exit 2
}

while [ $# -gt 0 ]; do
  case "$1" in
    --token)   TOKEN="${2:-}"; shift 2 ;;
    --api)     API="${2:-}"; shift 2 ;;
    --api-key) API_KEY="${2:-}"; shift 2 ;;
    -h|--help) usage ;;
    *) echo "unknown flag: $1"; usage ;;
  esac
done

[ -n "$TOKEN" ] || { echo "✗ 需要 --token <BOT_TOKEN>（@BotFather 拿）"; echo; usage; }
command -v jq >/dev/null || { echo "✗ 需要 jq（brew install jq）"; exit 2; }

CURL=(curl -sS --max-time 30)
[ -n "$API_KEY" ] && CURL+=(-H "x-api-key: $API_KEY")

step()  { printf '\n\033[1m%s\033[0m\n' "$*"; }
ok()    { printf '  \033[32m✓\033[0m %s\n' "$*"; }
bad()   { printf '  \033[31m✗\033[0m %s\n' "$*"; }
note()  { printf '    %s\n' "$*"; }

# ── 1. 服务在跑吗 ────────────────────────────────────────────────────────────
step "1/5  服务"
if ! "${CURL[@]}" "$API/api/health" >/dev/null 2>&1; then
  bad "$API 无响应 —— 先跑 scripts/smoke-serve.sh"
  exit 1
fi
ok "$API 在跑"

# 认证必须在动 bridge 之前探明。否则每个写操作都会 401，而下面那句
# "创建失败"会把缺 key 说成 token 不对 —— 让人往完全错误的方向查。
if [ "$("${CURL[@]}" "$API/api/auth/status" 2>/dev/null | jq -r '.enabled // false')" = "true" ] \
   && [ -z "$API_KEY" ]; then
  bad "服务开了认证，但没提供 key"
  note "取一个：neomind api-key list"
  note "或新建：neomind api-key create --name im-smoke"
  note "然后：NEOMIND_API_KEY=<key> $0 --token <BOT_TOKEN>"
  exit 1
fi
[ -n "$API_KEY" ] && ok "已带 API key"

# ── 2. bridge ────────────────────────────────────────────────────────────────
step "2/5  IM bridge"
existing=$("${CURL[@]}" "$API/api/im-bridges" | jq -r '[.data.bridges[]? | select(.id=="telegram" or .platform=="telegram")] | length' 2>/dev/null)
if [ "${existing:-0}" != "0" ]; then
  ok "telegram bridge 已存在，复用（要换 token 就先删掉它）"
else
  resp=$("${CURL[@]}" -X POST "$API/api/im-bridges" \
    -H 'content-type: application/json' \
    -d "$(jq -nc --arg t "$TOKEN" '{platform:"telegram", bot_token:$t}')")
  if echo "$resp" | jq -e '.data' >/dev/null 2>&1; then
    ok "bridge 已创建并启动"
  else
    msg=$(echo "$resp" | jq -r '.error.message // .error // .' 2>/dev/null | head -3)
    bad "创建失败：$msg"
    case "$msg" in
      *uth*)    note "认证问题 —— 见上面关于 API key 的提示。" ;;
      *)        note "token 无效、或该 bot 已被别处的 webhook 占用，都会在这里现形。" ;;
    esac
    exit 1
  fi
fi

# ── 3. 邀请 → 人工扫码 ───────────────────────────────────────────────────────
step "3/5  绑定（需要你扫码）"
inv=$("${CURL[@]}" -X POST "$API/api/im-bridges/telegram/invites")
link=$(echo "$inv" | jq -r '.data.deep_link // empty')
token=$(echo "$inv" | jq -r '.data.token // empty')
if [ -z "$token" ]; then
  bad "生成邀请失败：$(echo "$inv" | jq -c '.')"
  exit 1
fi
if [ -n "$link" ]; then
  ok "邀请链接："
  printf '\n      \033[36m%s\033[0m\n\n' "$link"
else
  bad "拿不到 deep_link —— bot 未识别（token 失效，或 getMe 不通）"
  note "可手动发送：/start $token"
fi
note "用手机 Telegram 打开上面的链接（或给 bot 发 /start $token）"
note "等最多 $((BIND_TIMEOUT/60)) 分钟，我在轮询绑定状态…"

bound=0
for _ in $(seq 1 $((BIND_TIMEOUT/3))); do
  n=$("${CURL[@]}" "$API/api/im-bridges/telegram/sessions" | jq -r '.data.sessions | length' 2>/dev/null)
  if [ "${n:-0}" != "0" ]; then bound=1; break; fi
  sleep 3
done
if [ "$bound" = "1" ]; then
  chat=$("${CURL[@]}" "$API/api/im-bridges/telegram/sessions" | jq -r '.data.sessions[0].chat_id // "?"' 2>/dev/null)
  ok "已绑定，chat_id=$chat"
else
  bad "等不到绑定（$BIND_TIMEOUT 秒）"
  exit 1
fi

# ── 4. 对话：人工发一条 ──────────────────────────────────────────────────────
step "4/5  收发（需要你发一条消息）"
before=$("${CURL[@]}" "$API/api/im-bridges/telegram/sessions" | jq -r '.data.sessions[0].last_active // 0' 2>/dev/null)
note "在 Telegram 里给 bot 发一句话（例如：你好）"
note "确认你收到了回复，然后回到这里按回车…"
read -r -p "    [回车继续] " _
after=$("${CURL[@]}" "$API/api/im-bridges/telegram/sessions" | jq -r '.data.sessions[0].last_active // 0' 2>/dev/null)
if [ "${after:-0}" != "${before:-0}" ]; then
  ok "会话 last_active 已更新 —— 消息确实到达并被处理"
else
  bad "last_active 没变：消息可能没到，或者回复没发出去"
  note "查服务日志：grep 'category=\"im\"' data/logs/*.log"
  exit 1
fi

# ── 5. 通知：agent → IM ──────────────────────────────────────────────────────
step "5/5  通知回流（这是本轮新做的）"
agent=$("${CURL[@]}" -X POST "$API/api/agents" -H 'content-type: application/json' -d "$(jq -nc '{
  name: "IM 冒烟",
  user_prompt: "回答：冒烟通过。",
  schedule: { schedule_type: "manual" },
  notify: { channels: ["IM"], on: "always" }
}')")
aid=$(echo "$agent" | jq -r '.data.id // empty')
if [ -z "$aid" ]; then
  bad "建 agent 失败：$(echo "$agent" | jq -c '.')"
  exit 1
fi
ok "agent 已建（$aid），notify 指向 IM"

run=$("${CURL[@]}" -X POST "$API/api/agents/$aid/invoke" -H 'content-type: application/json' -d '{}')
status=$(echo "$run" | jq -r '.data.status // "?"' 2>/dev/null)
ok "已触发，run status=$status"

cat <<EOF

  ─────────────────────────────────────────────────────────────
  最后一步只能你来：**看手机上有没有收到那条通知**。

  收到 → IM 端到端链路（入站 + 出站 + 通知回流）全通。
  没收到 → 大概率是这两个之一：
    · agent 的 notify.channels 里 "IM" 拼写不对（要一字不差）
    · 这条 run 因为结构化 agent 无输入而被拒（看日志里的 error）
  ─────────────────────────────────────────────────────────────
  清理：curl -X DELETE $API/api/agents/$aid
EOF
