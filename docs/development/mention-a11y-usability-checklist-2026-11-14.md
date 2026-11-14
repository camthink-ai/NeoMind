# @ 引用：IME / 无障碍自动化 + 人工可用性清单（1.3 iter C）

日期：2026-11-14。持续更新。Tip（headed 切片）：见 `c-mention-headed-a11y-2026-11-14.md`。版本未升；未推送。

## 1. 组件级自动化（vitest — 已落地）

| 场景 | 位置 | 状态 |
|------|------|------|
| IME composition 中不打开选择器 | ChatComposerMention + InlineAtEditor | 自动化 |
| compositionend 后无需额外按键即可查询 | ChatComposerMention + InlineAtEditor | 自动化 |
| IME Enter / keyCode 229 不选中、不转发发送 | ChatComposerMention + InlineAtEditor + imeSendGuard | 自动化 |
| 邮件式 `a@b.com` 不触发 | ChatComposerMention + model.test | 自动化 |
| 中文/标点边界触发 | ChatComposerMention | 自动化 |
| Escape 关闭；ArrowUp/Down + aria-selected；Enter 选中 | ChatComposerMention + InlineAtEditor | 自动化 |
| listbox / option / tablist / status / aria-activedescendant / aria-label | ChatComposerMention + InlineAtEditor | 自动化 |
| 重名 shortId+hint 文本辨识（非仅靠颜色） | ChatComposerMention + InlineAtEditor | 自动化 |
| 无效引用 data-invalid + 读屏文案 | ChatComposerMention | 自动化 |
| 移动端 Sheet 路径打开 | InlineAtEditor forceMobile | 组件级自动化 |

```bash
cd web && npx vitest run \
  src/components/chat/__tests__/ChatComposerMention.test.tsx \
  src/prototypes/inline-at/__tests__/InlineAtEditor.test.tsx \
  src/lib/chat/__tests__/imeSendGuard.test.ts \
  src/prototypes/inline-at/model.test.ts
```

## 2. 真浏览器 / 触屏模拟 / axe（Playwright headed — 2026-11-14 落地）

| 场景 | 位置 | 状态 |
|------|------|------|
| Chrome headed：原型 toolbar → listbox ARIA + 键盘选中插芯片 | `e2e/specs/mention-a11y.spec.ts` | **pass** |
| Chrome headed：原型页 axe wcag2a/aa serious/critical | 同上 | **pass**（`color-contrast` 规则关闭） |
| Chrome headed：生产 chat 历史 `contentNodes` 行内 chip + 填回输入框 | 同上 | **pass** |
| Chrome headed：生产 chat toolbar `@` 开 listbox；Escape 关 | 同上 | **pass** |
| 触屏模拟（390×844 `hasTouch`）：原型 mobile 面板 tap 选中 | 同上 | **pass**（**非**物理触屏） |
| 触屏模拟：历史 chip 可见且高度 ≥16px | 同上 | **pass** |

```bash
cd web && npm run test:e2e:mention-a11y
# headless smoke: npm run test:e2e:mention-a11y:headless
```

证据：`docs/development/c-mention-headed-a11y-2026-11-14.md`、`work/mention-a11y/playwright-headed-final.log`、`eval/manifests/1.3.0-c-mention-headed-a11y.json`。

API 全部 route-mock（同 mobile-audit）；**不**起评测 LLM、**不**占用 `:29431`。

## 3. 明确未验证 / 人工缺口（不得标为通过）

| 项 | 原因 | 计划门槛引用 |
|----|------|--------------|
| **物理触屏设备**（软键盘、系统返回先关面板、横竖屏） | 本环境无真机；仅 Playwright `hasTouch` + 窄视口 | §4.5 / §4.7 |
| **读屏实机**（VoiceOver / TalkBack 通读类型·名称·搜索状态） | 需人工听读；自动化只断言 ARIA + axe | §4.5 |
| **5 名目标用户 × 4 任务** | 需招募与主持；本机代理无法代劳 | §4.7 门槛 18/20 |

### 5 人可用性协议（待人工执行 — 当前 **0/20**）

任务（每人 4 个，记录协助次数与误操作原因，**不用平均时间掩盖失败**）：

1. **同名选择**：两个同名设备中选对目标（看 shortId/hint）。
2. **双引用来源/目标**：一句内插入两个不同引用并发送。
3. **替换句中引用**：删掉中间芯片后重选。
4. **移动端失败恢复重发**：无效引用标错后修正再发。

门槛：18/20 无需协助；错误对象发送 0；文本/引用/顺序/草稿丢失 0；关键输入无阻塞。

记录表模板：`participant_id / task / assisted(y/n) / wrong_object(y/n) / data_loss(y/n) / blocked(y/n) / notes / duration_s`。

**宣称规则：** 未跑满 20 次任务前，**禁止**把 §4.7「18/20」写成已通过。

## 4. 138 占用快照（本切片核对）

| 项 | 值 |
|----|-----|
| Controller | PID **89912** — `eval/run_eval.py` … `/tmp/neomind-iter-main-20261113/eval-138` |
| llama | PID **89904** — `127.0.0.1:29431` LISTEN |
| 本切片 | **未触碰**；未开新长跑 |

## 5. Next

1. 人工：真机触屏 + VoiceOver + 5×4（归档原始记录后再宣称门槛）。
2. **等 138 结束后**再开 live Spark 配对（端口空闲前禁止）。
3. 计划 D/E/F 剩余项。
