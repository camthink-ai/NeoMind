# 行内 @ 编辑器依赖取舍（1.3 迭代 B）

日期：2026-11-14  
范围：Chat 行内引用原型；**不**替换生产 `ChatComposer` 附件式 Chip。  
可运行原型：`/dev/inline-at`（`web/src/prototypes/inline-at/`）。

## 现状

| 层 | 实现 | 问题 |
|----|------|------|
| 生产输入 | `textarea` + 顶部 Chip 列表 | 去掉触发文字后，句内「来源/目标」位置丢失；同对象去重，无法表达「把 A 复制给 A」类句式 |
| 搜索 | `searchMentionOptions` 首屏本地过滤 | 计划 C 要求服务端分页；本片不改 |
| 已有富文本依赖 | CodeMirror（JS 编辑） | 不适合聊天气泡式行内 chip |
| TipTap / Lexical / Slate | **未安装** | 见下方包体 |

## 候选对比

| 方案 | 包体（npm unpacked，约） | 维护 | 行内 mention | 与现栈契合 | 结论 |
|------|--------------------------|------|--------------|------------|------|
| **A. 继续 textarea + Chip** | 0 | 已有测试 | 否（附件式） | 高 | 仅作兼容过渡 |
| **B. 自研 contentEditable** | 0 | 高（IME/选区/粘贴） | 可做但不稳 | 中 | **否决**（计划明确禁止） |
| **C. 分段 textarea + 原子 chip（本原型）** | 0 | 中（多焦点/换行弱） | 是 | 高 | **B 片采用**：验证内容模型与桌面/移动选择器 |
| **D. TipTap + Mention** | `@tiptap/react` ~576KB + starter-kit/extension-mention 另计；gzip 运行时通常数十 KB 级 | 社区活跃、扩展现成 | 一等公民 | 中（新依赖） | **推荐 C 片生产落地** |
| **E. Lexical** | `lexical` ~3.7MB unpacked | Meta 维护，样板重 | 需自写节点 | 低 | 过重，边缘设备不优先 |
| **F. Slate** | `slate` ~2.6MB unpacked | 灵活但样板多 | 需自写 | 低 | 同 E |

测量来源：`npm view <pkg> dist.unpackedSize`（2026-11-14）。未把 D/E/F 写入 `package-lock`，避免本片膨胀 lockfile。

## 原型验证了什么

1. **内容模型**：有序 `text | reference` 节点；`labelSnapshot` 仅展示；同 `type+id` 可重复出现。  
2. **选择器**：`@` 与工具栏 `@` 同一路径；桌面浮层 / 移动 `Sheet`；上下键 + Enter/Esc。  
3. **编辑**：Backspace 在 chip 后 offset 0 时整块删除；IME composition 期间不打开选择器；邮箱式 `a@b.com` 不触发（与生产正则一致）。  
4. **兼容桥**：`toLegacyUniqueReferences` 投影到现有 `ChatReference[]`（去重），明文 `toPlainText` 含 `@标签`。  
5. **上限**：引用节点 / 唯一引用 / 文本长度显式 `CapError`，禁止静默丢弃。

## 决策（给 C 片）

1. **短期**：生产仍保留 Chip 路径，避免半成品进主聊天。  
2. **中期（迭代 C）**：以本原型的 `InlineDoc` 为协议草案；生产编辑器优先接入 **TipTap Mention**（方案 D），替换分段 textarea（方案 C 的换行/选区局限）。  
3. **不做**：自研 CE；不为原型引入 Lexical/Slate。  
4. **负责人评审点**：是否接受 TipTap 进生产依赖；若否，则在分段模型上补强多行/撤销再评估。

## 未验证（本片）

- 真实触屏机软键盘避让（仅 `forceMobile` + Sheet）  
- 撤销/重做栈、富文本粘贴校验  
- HTTP/WS/图片通道节点协议（属 C）  
- 5 人可用性门槛（属 B 验收后段，未跑）

## 复现

```bash
cd web && npm run dev
# 登录后打开 /dev/inline-at
npx vitest run src/prototypes/inline-at
```
