# @ 引用：IME / 无障碍自动化 + 人工可用性清单（1.3 iter C）

日期：2026-11-14。Tip：本切片提交（基于 `57e01a38`）。版本未升；未推送。

## 1. 本切片自动化覆盖（已落地）

| 场景 | 位置 | 状态 |
|------|------|------|
| IME composition 中不打开选择器 | ChatComposerMention + InlineAtEditor | 自动化 |
| compositionend 后无需额外按键即可查询 | ChatComposerMention + InlineAtEditor | 自动化（本片修复） |
| IME Enter / keyCode 229 不选中、不转发发送 | ChatComposerMention + InlineAtEditor + imeSendGuard | 自动化 |
| 邮件式 `a@b.com` 不触发 | ChatComposerMention + model.test | 自动化 |
| 中文/标点边界触发 | ChatComposerMention | 自动化 |
| Escape 关闭；ArrowUp/Down + aria-selected；Enter 选中 | ChatComposerMention + InlineAtEditor | 自动化 |
| listbox / option / tablist / status / aria-activedescendant / aria-label | ChatComposerMention + InlineAtEditor | 自动化 |
| 重名 shortId+hint 文本辨识（非仅靠颜色） | ChatComposerMention + InlineAtEditor | 自动化 |
| 无效引用 data-invalid + 读屏文案 | ChatComposerMention | 自动化 |
| 移动端 Sheet 路径打开 | InlineAtEditor forceMobile | 组件级自动化 |

复现：

```bash
cd web && npx vitest run \
  src/components/chat/__tests__/ChatComposerMention.test.tsx \
  src/prototypes/inline-at/__tests__/InlineAtEditor.test.tsx \
  src/lib/chat/__tests__/imeSendGuard.test.ts \
  src/prototypes/inline-at/model.test.ts
```

## 2. 明确未验证 / 人工缺口（本机无法完成）

| 项 | 原因 | 计划门槛引用 |
|----|------|--------------|
| **真实浏览器**（Chrome/Safari 手工或 Playwright headed 全链路） | 本切片未起登录态+API 的 e2e 长跑；避免干扰 138 评测进程 | §4.7「组件测试之外必须真实浏览器验证」 |
| **真实触屏设备**（软键盘、返回键先关面板、横竖屏） | 本环境无物理触屏；仅 `forceMobile` 组件路径 | §4.5 / §4.7 |
| **读屏实机**（VoiceOver / TalkBack 通读类型·名称·搜索状态） | 需人工听读；自动化只断言 ARIA 属性 | §4.5 |
| **5 名目标用户 × 4 任务** | 需招募与主持；本机代理无法代劳 | §4.7 门槛 18/20 |

### 5 人可用性协议（待人工执行）

任务（每人 4 个，记录协助次数与误操作原因，**不用平均时间掩盖失败**）：

1. **同名选择**：两个同名设备中选对目标（看 shortId/hint）。
2. **双引用来源/目标**：一句内插入两个不同引用并发送。
3. **替换句中引用**：删掉中间芯片后重选。
4. **移动端失败恢复重发**：无效引用标错后修正再发。

门槛：18/20 无需协助；错误对象发送 0；文本/引用/顺序/草稿丢失 0；关键输入无阻塞。

记录表模板：`participant_id / task / assisted(y/n) / wrong_object(y/n) / data_loss(y/n) / blocked(y/n) / notes / duration_s`。

## 3. 建议的下一步人工步骤（不阻塞本切片合入）

1. 在隔离端口起 API + `web`，对 `/chat` 与 `/dev/inline-at` 做 headed Playwright + axe（现有 `e2e/specs/a11y.spec.ts` 可扩 mention 场景）。
2. 一台真机（iOS/Android）走 Soft keyboard + 返回键关面板。
3. VoiceOver 通读选择器：加载中 / 空结果 / 重名项 / 无效芯片。
4. 招募 5 人跑上表 4 任务，归档原始记录后再宣称 §4.7 门槛。

## 4. Next（计划清单）

本 C 段（浏览器/触屏/IME/无障碍自动化 + 缺口说明）完成后，计划下一项：

**D：Transform/Rule/命令/Agent 状态界面闭环**
