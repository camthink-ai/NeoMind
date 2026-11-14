# Chat content-nodes protocol v1（1.3 迭代 C 首片）

日期：2026-11-14  
Tip 基线：`fdca2b3a` 之上本片提交。

## 目标

有序 `text | reference` 节点；后端**原位**渲染类型+权威 ID+服务端名；旧 `message`+`references` 兼容；超限硬失败（不静默丢弃）。

## 请求字段

| 字段 | 说明 |
|------|------|
| `message` | 明文回退（`@label`）；有 `contentNodes` 时模型正文由节点生成 |
| `references` | 旧芯片路径 → preamble（超限截断+注明） |
| `contentVersion` | 仅 `1` |
| `contentNodes` | `{kind:"text",text}` 或 `{kind:"reference",type,id,labelSnapshot}` |

`contentNodes` 非空时优先于 `references` preamble。

## 模型正文形态

```
检查 【rule `uuid` "机房高温"】 是否启用
```

未找到：`【device \`id\`: NOT FOUND (…) — Do NOT substitute】`  
客户端 `labelSnapshot` **不**进入模型上下文。

## 上限（节点路径硬错误）

- 节点数 ≤ 64  
- 引用节点 / 唯一引用 ≤ 20  
- 文本字符 ≤ 8000  

HTTP → 400 + `code`；WS → `{type:"Error", code, message, limit, actual}`。

## 草稿

`web/src/lib/chat/contentProtocol.ts`：`neomind:chat-draft:v1:<sessionId>`，会话隔离；失败可用 `markDraftForRetry` 恢复。

## 本片未做

- 生产 ChatComposer 改行内编辑器（仍 Chip；TipTap 在后续）  
- SessionMessage 持久化 contentNodes（历史重载仍靠明文/`@label`；乐观 UI 可带 nodes）  
- 服务端搜索分页（下一 C 子序）

## 测试

```bash
cargo test -p neomind-api --test handlers chat_refs --locked
cd web && npx vitest run src/lib/chat/__tests__/contentProtocol.test.ts src/prototypes/inline-at
```
