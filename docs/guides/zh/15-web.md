# Web 前端模块

**版本**: 0.5.8
**完成度**: 85%
**用途**: React + TypeScript Web 应用 / Tauri 桌面应用

## 概述

Web 模块是 NeoMind 的前端应用，支持 Web 浏览器和 Tauri 桌面应用（双模式）。基于 React 18 + TypeScript + Vite 构建，使用 Zustand 进行状态管理，Radix UI + Tailwind CSS 构建用户界面。

## 重要变更 (v0.5.x)

### Plugin → Extension 迁移

- **`/plugins` 路由已迁移到 `/extensions`**
- `plugins.tsx` → `extensions.tsx`
- 扩展组件统一在 `components/extensions/` 目录
- API调用从 `api.listPlugins()` 改为 `api.listExtensions()`

## 模块结构

```
web/src/
├── main.tsx                    # 应用入口
├── App.tsx                     # 路由和认证
├── components/                 # UI 组件
│   ├── ui/                    # 基础 UI 组件 (Radix UI 封装)
│   │   ├── button.tsx
│   │   ├── input.tsx
│   │   ├── card.tsx
│   │   ├── switch.tsx
│   │   ├── tabs.tsx
│   │   ├── toast.tsx
│   │   ├── dialog.tsx
│   │   ├── dropdown-menu.tsx
│   │   ├── select.tsx
│   │   ├── slider.tsx
│   │   ├── checkbox.tsx
│   │   ├── popover.tsx
│   │   ├── tooltip.tsx
│   │   ├── scroll-area.tsx
│   │   ├── sheet.tsx
│   │   ├── accordion.tsx
│   │   ├── code-editor.tsx
│   │   └── ...
│   ├── layout/                # 布局组件
│   │   └── TopNav.tsx
│   ├── chat/                  # 聊天相关组件
│   │   ├── MarkdownMessage.tsx
│   │   ├── ThinkingBlock.tsx
│   │   ├── StreamProgress.tsx
│   │   ├── ToolCallVisualization.tsx
│   │   └── ...
│   ├── dashboard/             # 仪表板组件
│   │   ├── DashboardGrid.tsx
│   │   ├── registry/
│   │   └── config/
│   ├── automation/            # 自动化组件
│   │   ├── rule/
│   │   └── TransformTestDialog.tsx
│   ├── devices/               # 设备相关组件
│   │   ├── DeviceList.tsx
│   │   ├── DeviceDetail.tsx
│   │   └── ...
│   ├── session/               # 会话管理组件
│   ├── extensions/            # 扩展组件（统一管理Extension）
│   ├── shared/                # 共享组件
│   └── design-system/         # 设计系统
├── pages/                     # 页面组件
│   ├── login.tsx              # 登录页
│   ├── setup.tsx              # 初始化设置
│   ├── chat.tsx               # 聊天页
│   ├── devices.tsx            # 设备页
│   ├── automation.tsx         # 自动化页
│   ├── agents.tsx             # AI Agent 页
│   ├── settings.tsx           # 设置页
│   ├── commands.tsx           # 命令页
│   ├── decisions.tsx          # 决策页
│   ├── messages.tsx           # 消息页
│   └── extensions.tsx         # 扩展页
├── store/                     # Zustand 状态管理
│   ├── index.ts               # Store 入口
│   ├── slices/                # 状态切片
│   │   ├── authSlice.ts       # 认证状态
│   │   ├── sessionSlice.ts    # 会话状态
│   │   ├── uiSlice.ts         # UI 状态
│   │   ├── deviceSlice.ts     # 设备状态
│   │   ├── alertSlice.ts      # 消息/告警状态
│   │   ├── settingsSlice.ts   # 设置状态
│   │   ├── decisionSlice.ts   # 决策状态
│   │   ├── extensionSlice.ts  # 扩展状态
│   │   ├── llmBackendSlice.ts # LLM 后端状态
│   │   └── dashboardSlice.ts  # 仪表板状态
│   ├── selectors/             # 选择器
│   └── persistence/           # 持久化
├── lib/                       # 工具库
│   ├── api.ts                 # API 客户端
│   ├── websocket.ts           # WebSocket 聊天
│   ├── events.ts              # 事件流 (SSE/WS)
│   ├── auth/                  # 认证工具
│   ├── notify.ts              # 通知系统
│   └── utils/                 # 工具函数
├── hooks/                     # React Hooks
│   ├── useDialog.ts
│   ├── useForm.ts
│   ├── use-toast.ts
│   └── use-confirm.ts
├── types/                     # TypeScript 类型
│   └── index.ts               # 统一类型定义
├── design-system/             # 设计系统
│   ├── components/
│   └── tokens/
└── i18n/                      # 国际化
```

## 技术栈

### 核心框架

```json
{
  "react": "^18.3.1",
  "react-dom": "^18.3.1",
  "typescript": "^5.3.3",
  "vite": "^5.1.0"
}
```

### 状态管理

```json
{
  "zustand": "^4.5.0"
}
```

### UI 组件库

```json
{
  "@radix-ui/react-accordion": "^1.2.12",
  "@radix-ui/react-alert-dialog": "^1.1.15",
  "@radix-ui/react-checkbox": "^1.3.3",
  "@radix-ui/react-dialog": "^1.0.5",
  "@radix-ui/react-dropdown-menu": "^2.0.6",
  "@radix-ui/react-label": "^2.0.2",
  "@radix-ui/react-popover": "^1.1.15",
  "@radix-ui/react-progress": "^1.1.8",
  "@radix-ui/react-scroll-area": "^1.0.5",
  "@radix-ui/react-select": "^2.0.0",
  "@radix-ui/react-separator": "^1.1.8",
  "@radix-ui/react-slider": "^1.3.6",
  "@radix-ui/react-switch": "^1.0.3",
  "@radix-ui/react-tabs": "^1.0.4",
  "@radix-ui/react-toast": "^1.1.5",
  "@radix-ui/react-tooltip": "^1.2.8"
}
```

### 其他依赖

```json
{
  "react-router-dom": "^6.22.0",
  "react-hook-form": "^7.71.0",
  "@hookform/resolvers": "^5.2.2",
  "recharts": "^3.7.0",
  "react-grid-layout": "^2.2.2",
  "react-markdown": "^10.1.0",
  "remark-gfm": "^4.0.1",
  "lucide-react": "^0.344.0",
  "tailwindcss": "^3.4.1",
  "class-variance-authority": "^0.7.0",
  "clsx": "^2.1.0",
  "tailwind-merge": "^2.3.0",
  "zod": "^4.3.5",
  "i18next": "^25.7.4",
  "react-i18next": "^16.5.2",
  "@tauri-apps/api": "^2.0.0"
}
```

## 路由结构

```typescript
// 公开路由
/login                    // 登录页（检查是否需要初始化）
/setup                    // 初始化设置页

// 受保护路由（需要认证）
/                         // 默认跳转到聊天页
/chat                    // AI 聊天页
/chat/:sessionId         // 指定会话聊天
/devices                 // 设备管理页
/devices/:id             // 设备详情页
/devices/types           // 设备类型页
/devices/drafts          // 草稿设备页（自动入板）
/automation              // 自动化页
/automation/transforms   // 数据转换页
/agents                  // AI Agents 页
/settings                // 设置页
/commands                // 命令页
/decisions               // 决策页
/messages                // 消息页
/messages/channels       // 消息通道页
/extensions              // 扩展管理页（统一Extension系统）
/visual-dashboard        // 可视化仪表板
/visual-dashboard/:id    // 指定仪表板
```

## 状态管理 (Zustand)

### Store 结构

```typescript
export type NeoMindStore = AuthSlice
  & SessionSlice
  & UISlice
  & AlertSlice
  & DeviceSlice
  & SettingsSlice
  & DecisionSlice
  & ExtensionSlice
  & LlmBackendSlice
  & DashboardState
```

### AuthSlice - 认证状态

```typescript
interface AuthSlice {
  // 状态
  token: string | null
  user: UserInfo | null
  isAuthenticated: boolean

  // 操作
  setToken: (token: string, remember?: boolean) => void
  clearToken: () => void
  setUser: (user: UserInfo) => void
  checkAuthStatus: () => Promise<void>
}
```

### SessionSlice - 会话状态

```typescript
interface SessionSlice {
  // 状态
  messages: Message[]
  sessionId: string
  sessions: ChatSession[]
  isStreaming: boolean

  // 操作
  setMessages: (messages: Message[]) => void
  addMessage: (message: Message) => void
  updateLastMessage: (content: string, isPartial?: boolean) => void
  setSessionId: (id: string) => void
  setSessions: (sessions: ChatSession[]) => void
}
```

### DeviceSlice - 设备状态

```typescript
interface DeviceSlice {
  // 状态
  devices: Device[]
  deviceTypes: DeviceType[]
  loading: boolean

  // 操作
  setDevices: (devices: Device[]) => void
  setDeviceTypes: (types: DeviceType[]) => void
  updateDevice: (id: string, updates: Partial<Device>) => void
  fetchDevices: () => Promise<void>
}
```

## API 客户端

### fetchAPI - 统一请求函数

```typescript
async function fetchAPI<T>(
  path: string,
  options: FetchOptions = {}
): Promise<T>

interface FetchOptions extends RequestInit {
  skipAuth?: boolean          // 跳过 JWT 认证
  skipGlobalError?: boolean    // 跳过全局错误处理
  skipErrorToast?: boolean     // 跳过错误提示
  successMessage?: string      // 成功提示消息
}
```

### API 端点分组

```typescript
export const api = {
  // 认证
  login: (username: string, password: string) => Promise<LoginResponse>
  logout: () => Promise<{ message: string }>
  getCurrentUser: () => Promise<UserInfo>

  // 设备
  getDevices: () => Promise<{ devices: Device[]; count: number }>
  getDevice: (id: string) => Promise<Device>
  addDevice: (req: AddDeviceRequest) => Promise<{ device_id: string }>
  deleteDevice: (id: string) => Promise<{ deleted: boolean }>
  sendCommand: (deviceId: string, command: string, params: Record<string, unknown>) => Promise<...>

  // 自动化
  listAutomations: (params?: AutomationFilter) => Promise<AutomationListResponse>
  createAutomation: (req: CreateAutomationRequest) => Promise<...>
  analyzeAutomationIntent: (description: string) => Promise<IntentResult>

  // Agents
  listAgents: () => Promise<AgentListResponse>
  getAgent: (id: string) => Promise<AiAgentDetail>
  executeAgent: (id: string, req?: ExecuteAgentRequest) => Promise<...>

  // ... 更多端点
}
```

## WebSocket 聊天

### ChatWebSocket 类

```typescript
export class ChatWebSocket {
  // 连接
  connect(initialSessionId?: string): void
  disconnect(): void
  manualReconnect(): void
  isConnected(): boolean

  // 消息发送
  sendMessage(content: string, images?: ChatImage[]): void
  sendRequest(request: ClientChatMessage): void

  // 会话管理
  setSessionId(sessionId: string): void
  getSessionId(): string | null
  setActiveBackend(backendId: string | null): void

  // 事件监听
  onMessage(handler: (message: ServerMessage) => void): () => void
  onConnection(handler: (connected: boolean, isReconnect?: boolean) => void): () => void
  onStateChange(handler: (state: ConnectionState) => void): () => void

  // 状态
  getState(): ConnectionState
}

// 单例
export const ws = new ChatWebSocket()
```

### 服务器消息类型

```typescript
type ServerMessage =
  | { type: 'Thinking'; content: string; sessionId: string }
  | { type: 'Content'; content: string; sessionId: string }
  | { type: 'ToolCallStart'; tool: string; arguments: Record<string, unknown>; sessionId: string }
  | { type: 'ToolCallEnd'; tool: string; result: string; sessionId: string; success?: boolean }
  | { type: 'Progress'; elapsed: number; stage?: string; message?: string; sessionId: string }
  | { type: 'Intent'; category: string; displayName: string; confidence?: number; sessionId: string }
  | { type: 'Warning'; message: string; elapsed?: number; remainingTime?: number; sessionId: string }
  | { type: 'Error'; message: string; sessionId: string }
  | { type: 'end'; sessionId: string }
  | { type: 'ping'; timestamp: number }
```

## 事件流 (SSE/WebSocket)

### EventsWebSocket 类

```typescript
export class EventsWebSocket {
  // 连接
  connect(): void
  disconnect(): void
  isConnected(): boolean

  // 订阅
  onEvent(handler: (event: NeoMindEvent) => void): () => void
  on<T extends EventType>(eventType: T, handler: (event: Extract<NeoMindEvent, { type: T }>) => void): () => void
  onDeviceEvent(handler: (event: DeviceMetricEvent) => void): () => void
  onRuleEvent(handler: (event: RuleTriggeredEvent) => void): () => void
  onConnection(handler: (connected: boolean) => void): () => void

  // 配置
  updateConfig(config: Partial<EventsConfig>): void
}

// 获取连接
export function getEventsConnection(key?: string, config?: EventsConfig): EventsWebSocket
```

### 事件类型

```typescript
export type EventType =
  | 'DeviceOnline' | 'DeviceOffline' | 'DeviceMetric' | 'DeviceCommandResult'
  | 'RuleEvaluated' | 'RuleTriggered'
  | 'AgentExecutionStarted' | 'AgentThinking' | 'AgentDecision' | 'AgentProgress' | 'AgentExecutionCompleted'
  | 'AlertCreated' | 'AlertAcknowledged'
  | 'LlmDecisionProposed' | 'LlmDecisionExecuted'
  | 'ToolExecutionStart' | 'ToolExecutionSuccess' | 'ToolExecutionFailure'
  | 'Custom'

export interface NeoMindEvent {
  id: string
  type: EventType
  timestamp: number
  source: string
  data: unknown
}
```

## UI 组件

### 基础组件

基于 Radix UI 的封装组件，使用 `class-variance-authority` 管理变体：

```typescript
// Button 示例
const buttonVariants = cva(
  "inline-flex items-center justify-center rounded-md text-sm font-medium",
  {
    variants: {
      variant: {
        default: "bg-primary text-primary-foreground hover:bg-primary/90",
        destructive: "bg-destructive text-destructive-foreground hover:bg-destructive/90",
        outline: "border border-input bg-background hover:bg-accent",
        ghost: "hover:bg-accent hover:text-accent-foreground",
        link: "text-primary underline-offset-4 hover:underline",
      },
      size: {
        default: "h-10 px-4 py-2",
        sm: "h-9 rounded-md px-3",
        lg: "h-11 rounded-md px-8",
        icon: "h-10 w-10",
      },
    },
  }
)
```

### 共享组件

| 组件 | 描述 |
|------|------|
| `StatsCard` | 统计卡片 |
| `BulkActionBar` | 批量操作栏 |
| `Pagination` | 分页组件 |
| `EmptyState` | 空状态提示 |
| `LoadingState` | 加载状态 |
| `StatusBadge` | 状态徽章 |
| `ActionBar` | 操作栏 |
| `MonitorStatsGrid` | 监控统计网格 |

### 聊天组件

| 组件 | 描述 |
|------|------|
| `MarkdownMessage` | Markdown 消息渲染 |
| `ThinkingBlock` | 思考过程展示 |
| `StreamProgress` | 流式输出进度 |
| `ToolCallVisualization` | 工具调用可视化 |
| `SessionSidebar` | 会话侧边栏 |
| `ConnectionStatus` | 连接状态指示 |

## Tauri 桌面应用

### 检测 Tauri 环境

```typescript
const isTauri = !!(window as any).__TAURI__

// Tauri 使用本地后端
const API_BASE = isTauri ? 'http://localhost:9375/api' : '/api'
const wsHost = isTauri ? 'localhost:9375' : window.location.host
```

### 启动加载

```typescript
// Tauri 应用等待后端就绪
if (isTauri && !backendReady) {
  return <StartupLoading onReady={() => setBackendReady(true)} />
}
```

## 开发命令

```bash
# 开发模式
npm run dev

# 构建
npm run build

# 预览
npm run preview

# Tauri 开发
npm run tauri:dev

# Tauri 构建
npm run tauri:build

# 测试 (Playwright)
npm run test
npm run test:ui
npm run test:headed

# 代码检查
npm run lint
```

## 构建配置

### Vite 配置

```typescript
// vite.config.ts
export default defineConfig({
  plugins: [react()],
  resolve: {
    alias: {
      '@': path.resolve(__dirname, './src'),
    },
  },
  server: {
    port: 5173,
    proxy: {
      '/api': {
        target: 'http://localhost:9375',
        changeOrigin: true,
      },
    },
  },
  build: {
    target: 'esnext',
    minify: 'terser',
  },
})
```

### Tailwind 配置

```javascript
// tailwind.config.js
module.exports = {
  darkMode: ['class'],
  content: ['./src/**/*.{ts,tsx}'],
  theme: {
    extend: {
      colors: {
        border: 'hsl(var(--border))',
        input: 'hsl(var(--input))',
        ring: 'hsl(var(--ring))',
        background: 'hsl(var(--background))',
        foreground: 'hsl(var(--foreground))',
        primary: {
          DEFAULT: 'hsl(var(--primary))',
          foreground: 'hsl(var(--primary-foreground))',
        },
        // ...
      },
      borderRadius: {
        lg: 'var(--radius)',
        md: 'calc(var(--radius) - 2px)',
        sm: 'calc(var(--radius) - 4px)',
      },
    },
  },
  plugins: [require('@tailwindcss/typography')],
}
```

## 设计系统

### 颜色系统

```css
:root {
  --background: 0 0% 100%;
  --foreground: 240 10% 3.9%;
  --card: 0 0% 100%;
  --card-foreground: 240 10% 3.9%;
  --popover: 0 0% 100%;
  --popover-foreground: 240 10% 3.9%;
  --primary: 240 5.9% 10%;
  --primary-foreground: 0 0% 98%;
  --secondary: 240 4.8% 95.9%;
  --secondary-foreground: 240 5.9% 10%;
  --muted: 240 4.8% 95.9%;
  --muted-foreground: 240 3.8% 46.1%;
  --accent: 240 4.8% 95.9%;
  --accent-foreground: 240 5.9% 10%;
  --destructive: 0 84.2% 60.2%;
  --destructive-foreground: 0 0% 98%;
  --border: 240 5.9% 90%;
  --input: 240 5.9% 90%;
  --ring: 240 5.9% 10%;
  --radius: 0.5rem;
}

.dark {
  --background: 240 10% 3.9%;
  --foreground: 0 0% 98%;
  --card: 240 10% 3.9%;
  --card-foreground: 0 0% 98%;
  --popover: 240 10% 3.9%;
  --popover-foreground: 0 0% 98%;
  --primary: 0 0% 98%;
  --primary-foreground: 240 5.9% 10%;
  --secondary: 240 3.7% 15.9%;
  --secondary-foreground: 0 0% 98%;
  --muted: 240 3.7% 15.9%;
  --muted-foreground: 240 5% 64.9%;
  --accent: 240 3.7% 15.9%;
  --accent-foreground: 0 0% 98%;
  --destructive: 0 62.8% 30.6%;
  --destructive-foreground: 0 0% 98%;
  --border: 240 3.7% 15.9%;
  --input: 240 3.7% 15.9%;
  --ring: 240 4.9% 83.9%;
}
```

### 图标系统

使用 `lucide-react` 图标库，统一风格：

```typescript
import {
  Bot,   // AI/Agent
  Cpu,   // 设备
  Zap,   // 自动化
  Bell,  // 消息/通知
  Settings,  // 设置
  // ...
} from 'lucide-react'
```

## 国际化

使用 `i18next` + `react-i18next`：

```typescript
import i18n from 'i18next'
import { initReactI18next } from 'react-i18next'
import LanguageDetector from 'i18next-browser-languagedetector'

i18n
  .use(LanguageDetector)
  .use(initReactI18next)
  .init({
    resources: {
      en: { translation: { /* ... */ } },
      zh: { translation: { /* ... */ } },
    },
    fallbackLng: 'en',
    interpolation: { escapeValue: false },
  })
```

## 性能优化

### 代码分割

```typescript
// 路由懒加载
const ChatPage = lazy(() => import('@/pages/chat').then(m => ({ default: m.ChatPage })))
const DevicesPage = lazy(() => import('@/pages/devices').then(m => ({ default: m.DevicesPage })))
```

### 虚拟列表

```typescript
// 大列表使用虚拟滚动
import { VirtualList } from '@/components/ui/VirtualList'

<VirtualList
  items={items}
  itemHeight={50}
  renderItem={(item) => <ListItem item={item} />}
/>
```

### 防抖/节流

```typescript
import { useDebounce, useThrottle } from '@/hooks/useDebounce'

const debouncedSearch = useDebounce(searchTerm, 300)
const throttledSave = useThrottle(saveData, 1000)
```

## 设计原则

1. **组件化**: 小而专注的组件，单一职责
2. **类型安全**: 完整的 TypeScript 类型定义
3. **响应式**: 移动端和桌面端适配
4. **可访问性**: 遵循 WCAG 标准，使用 ARIA
5. **性能优先**: 代码分割、懒加载、虚拟滚动
