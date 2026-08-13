# NeoMind Markdown 渲染规范

AI 回复的 markdown 由 `web/src/components/chat/MarkdownMessage.tsx` 渲染。本文档定义 NeoMind 专属的 markdown 元素样式标准——是 `DESIGN_SPEC.md` 在「模型回复内容」这一域的延伸。

**核心原则**:所有可见颜色/间距走 design token,禁止 raw 色;改样式只改 `MarkdownMessage.tsx` 的 prose className 或 `CodeBlock` 组件。

## 技术栈

| 层 | 库 | 作用 |
|----|----|------|
| 解析 | `react-markdown` ^10 | markdown → React |
| GFM | `remark-gfm` ^4 | 表格 / 删除线 / 任务列表 |
| 高亮 | `rehype-highlight` | 代码块语法高亮(hljs) |
| 排版 | `@tailwindcss/typography` | `prose` 作为结构起点,再覆盖 |

## 字号层级

| 元素 | 字号 | 说明 |
|------|------|------|
| 正文 | 13px(移动) / 14px(桌面 `sm:`) | `text-[13px] sm:text-sm` |
| h1 | 16px | |
| h2 | 15px | |
| h3 | 14px | 与桌面正文同号,靠 `font-semibold` 区分 |
| 行内代码 | 12px | |
| 代码块 | 继承正文 | |

**硬规则:标题不得小于正文。** `h1 > h2 > h3 ≥ 正文`。曾出现正文升 14px 后 h2=13/h3=12 小于正文的倒挂——改正文字号时必须同步检查标题层级。

## 元素样式标准

### 段落
- `leading-relaxed`,上下 margin `my-1`(4px)
- `break-words overflow-wrap-anywhere`:长词/长 URL 强制断行,不撑破气泡

### 标题
- `font-semibold`,`mt-4 mb-2`(16px/8px,标题「抱团」下文);字号见上表

### 链接
- 颜色**继承正文**(`text-inherit`),靠 `underline + underline-offset-2` 提供可辨识度
- **不设 prose-a color**。gotcha:亮色主题 `--primary` 近白,会和用户气泡的 `--msg-user-bg`(也近白)冲突,导致用户消息里的 URL 隐形

### 强调
- `strong`: `font-semibold`;`em`: 默认斜体

### 行内代码
- `bg-muted` + `rounded` + `px-1 py-0.5` + 12px 等宽(`font-mono`)
- `break-all whitespace-pre-wrap`:长内联代码可换行

### 代码块(CodeBlock 组件)
- **不走 prose-pre 默认**,由 `MarkdownMessage.tsx` 内的 `CodeBlock` 组件渲染
- 结构:`header`(语言标签 + 复制按钮)+ `pre`
- 视觉:`bg-muted` + `rounded-lg`,**无 border**(沉浸式,和内容无缝)
- header:细条(`pt-1.5 pb-1`),语言名 10px 大写小字,复制按钮纯图标(24×24),`Copy`→`Check` 反馈
- 语法高亮色走 `--syn-*` token(亮/暗各一套,见 `index.css`),`!important` 覆盖 bubble prose 的颜色压平规则

### 引用 blockquote
- `border-l-2 border-muted-foreground` + 浅背景 `bg-muted-30` + `pl-3 pr-3 py-1 rounded-r-md` + `italic`

### 列表
- `ul`: `list-disc`;`ol`: `list-decimal`;均 `pl-4 my-1`
- `li`: `my-0.5`;标记(`::marker`)色 `text-muted-foreground`(淡于正文,不抢内容)

### 表格
- 整体 `text-[13px]`
- **横线分隔风(booktabs)**:表头下 2px 实线(`border-b-2 border-border`)+ `font-semibold` + 浅灰 `bg-muted-50`;行间 1px 浅横线(`border-b border-muted-30`);**无竖线**
- `th`/`td` 均 `px-2 py-1.5`

### 分隔线 hr
- `my-2` + `border-border`

## 主题适配

- 亮色 token 在 `:root`,暗色在 `.dark`(Tailwind class 模式)
- 语法高亮色 `--syn-text/keyword/string/comment/number/function/builtin/variable/tag` 亮暗各一套
- 代码块/引用/表格的背景、边框都走 token,自动随主题切换

## 图片

- 用户消息附件图片走 `MessageImages`(单图全宽 / 多图 2 列网格,`max-h-64` 懒加载)
- AI 回复内联图片当前走 prose 默认,尚未定制

## 维护约定

1. 改 markdown 样式 → 只动 `MarkdownMessage.tsx` 的 prose className 或 `CodeBlock`
2. 新颜色 → 一律新增/复用 design token,禁止 raw hex
3. 复制按钮组件 `CopyMessageButton` 负责整条消息复制(复制 markdown 原文,非渲染文本);代码块内的复制逻辑在 `CodeBlock`
4. 流式渲染期间 `MarkdownMessage` 是 `React.memo`,组件覆盖对象(`MARKDOWN_COMPONENTS`)hoist 到模块作用域,不要移到组件内(否则每 chunk 重建)
