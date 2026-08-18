/**
 * Page-scoped assistant config — the floating/docked chat panel specializes
 * per route. Three backend-level levers (applied when the page's panel
 * session is created — see PanelChatView):
 *
 * - systemPromptSuffix: appended to the agent's REAL system prompt
 *   (`sessionConfig.systemPromptSuffix` → CreateSessionOptions), not baked
 *   into any user message.
 * - tools: a per-session tool allowlist (`sessionConfig.allowedTools`);
 *   the interaction tools (ask_user/confirm_action/clarify_intent) are
 *   always kept server-side.
 * - skillKeywords: matched against installed skills (keywords/category/
 *   name); matches are pinned via selectedSkills on every send.
 *
 * Plus the pure-UI pieces: greeting + quick actions.
 * Copy stays inline (zh/en picked by i18n.language); migrate to chat.json
 * namespaces if the copy stabilizes.
 */

export interface PageQuickAction {
  label: { zh: string; en: string }
  prompt: { zh: string; en: string }
}

export interface PageAssistantConfig {
  /** Persistent system-prompt suffix for this page's panel session */
  systemPromptSuffix: { zh: string; en: string }
  /** Tool allowlist for the session (empty = all tools) */
  tools: string[]
  /** Keywords used to match installed skills for this page */
  skillKeywords: string[]
  /** Panel welcome line while the session is empty */
  greeting: { zh: string; en: string }
  quickActions: PageQuickAction[]
}

export interface ResolvedPageAssistant {
  /** Route key ('devices' | 'agents' | …), used for per-page session storage */
  key: string
  systemPromptSuffix: string
  tools: string[]
  skillKeywords: string[]
  greeting: string
  quickActions: { label: string; prompt: string }[]
}

const pick = (lang: string, c: { zh: string; en: string }) =>
  lang.startsWith('zh') ? c.zh : c.en

export function pickPageAssistant(pathname: string, lang: string): ResolvedPageAssistant | null {
  const key = routeKey(pathname)
  const cfg = PAGE_ASSISTANTS[key]
  if (!cfg) return null
  return {
    key,
    systemPromptSuffix: pick(lang, cfg.systemPromptSuffix),
    tools: cfg.tools,
    skillKeywords: cfg.skillKeywords,
    greeting: pick(lang, cfg.greeting),
    quickActions: cfg.quickActions.map((a) => ({
      label: pick(lang, a.label),
      prompt: pick(lang, a.prompt),
    })),
  }
}

function routeKey(pathname: string): string {
  return pathname.split('/')[1] || ''
}

/**
 * localStorage key for a page's panel session. v2: sessions are per-page so
 * each page's session can carry its own system-prompt suffix + tool profile
 * (v1 was one shared session, which froze the first page's focus).
 */
export const PANEL_SESSION_PREFIX = 'neomind:panelSession:v2:'

export function panelSessionKey(pageKey: string): string {
  return PANEL_SESSION_PREFIX + (pageKey || 'default')
}

/** Tools every page profile keeps (domain filtering only trims specialists). */
const CORE_TOOLS = ['shell', 'skill', 'memory', 'vision']
const FILE_TOOLS = ['file_write', 'file_edit']

const PAGE_ASSISTANTS: Record<string, PageAssistantConfig> = {
  devices: {
    systemPromptSuffix: {
      zh: '## 当前页面专注域：设备接入\n你是 NeoMind 平台的「设备接入助手」。优先引导用户：产品设备接入（MQTT/Webhook/蓝牙）、创建模拟设备、构建设备类型（指标/命令）、待注册设备的准入审批。需要实际操作时用 neomind CLI 完成。回答优先给出具体操作步骤。',
      en: '## Current page focus: device onboarding\nYou are the NeoMind device-onboarding assistant. Prioritize: product device onboarding (MQTT/webhook/BLE), simulated devices, device types (metrics/commands), pending-device admission. Perform real operations via the neomind CLI. Prefer concrete step-by-step instructions.',
    },
    tools: [...CORE_TOOLS],
    skillKeywords: ['device', 'mqtt', 'onboarding', 'simulated', '设备', '接入'],
    greeting: {
      zh: '我在这里帮你完成设备接入 —— 扫码/MQTT 接入、模拟设备、设备类型、待注册准入',
      en: 'Here to help with device onboarding — MQTT/scan setup, simulated devices, types, and admissions',
    },
    quickActions: [
      { label: { zh: '如何接入一台设备？', en: 'How to onboard a device?' }, prompt: { zh: '详细说明接入一台新设备的完整步骤（MQTT / Webhook / 蓝牙）', en: 'Walk me through onboarding a new device (MQTT / webhook / BLE)' } },
      { label: { zh: '创建模拟设备', en: 'Create a simulated device' }, prompt: { zh: '帮我创建一个模拟温度湿度传感器设备用于测试', en: 'Help me create a simulated temperature/humidity sensor for testing' } },
      { label: { zh: '构建设备类型', en: 'Build a device type' }, prompt: { zh: '解释设备类型是什么，并指导我从零构建一个自定义设备类型（指标与命令）', en: 'Explain device types and guide me through building one (metrics & commands)' } },
      { label: { zh: '待注册设备准入', en: 'Admit a pending device' }, prompt: { zh: '待注册设备列表是做什么的？如何审批准入一个新发现的设备？', en: 'What is the pending list and how do I admit a discovered device?' } },
    ],
  },
  agents: {
    systemPromptSuffix: {
      zh: '## 当前页面专注域：智能体构建\n你是 NeoMind 平台的「智能体构建助手」。优先引导用户：创建、修改、测试 AI 智能体（提示词、工具选择、记忆、技能、定时触发），调试失败运行（执行时间线）。需要实际操作时用 neomind CLI 完成。',
      en: '## Current page focus: agent building\nYou are the NeoMind agent-building assistant. Prioritize: creating, editing, and testing AI agents (prompts, tool selection, memory, skills, schedules), debugging failed runs via the execution timeline. Perform real operations via the neomind CLI.',
    },
    tools: [...CORE_TOOLS, ...FILE_TOOLS],
    skillKeywords: ['agent', 'prompt', 'skill', 'tool', '智能体', '提示词'],
    greeting: {
      zh: '需要搭一个智能体？从提示词、工具选择到测试运行，我都可以指导',
      en: 'Building an agent? I can guide prompts, tools, and test runs',
    },
    quickActions: [
      { label: { zh: '创建第一个智能体', en: 'Create my first agent' }, prompt: { zh: '带我从零创建一个智能体：提示词怎么写、怎么选工具、怎么测试', en: 'Walk me through creating an agent from scratch: prompt, tools, testing' } },
      { label: { zh: '调试运行失败', en: 'Debug a failing run' }, prompt: { zh: '我的智能体运行失败了，如何查看执行详情定位问题？', en: 'My agent run failed — how do I inspect the execution timeline to debug?' } },
      { label: { zh: '工具与技能区别', en: 'Tools vs skills' }, prompt: { zh: '解释智能体的工具（tools）和技能（skills）的区别与用法', en: 'Explain the difference between agent tools and skills' } },
    ],
  },
  'visual-dashboard': {
    systemPromptSuffix: {
      zh: '## 当前页面专注域：可视化看板\n你是 NeoMind 平台的「看板助手」。优先引导用户：看板的创建/编辑/切换、组件管理与新增、自定义组件代码编写（清单格式/代码结构）、数据源绑定、布局修改。需要实际操作时用 neomind CLI 完成。',
      en: '## Current page focus: visual dashboards\nYou are the NeoMind dashboard assistant. Prioritize: dashboard create/edit/switch, component management, custom component code (manifest/structure), data-source binding, layout editing. Perform real operations via the neomind CLI.',
    },
    tools: [...CORE_TOOLS, ...FILE_TOOLS],
    skillKeywords: ['dashboard', 'chart', 'widget', 'visualization', '看板', '组件'],
    greeting: {
      zh: '搭建可视化看板 —— 组件、数据源绑定、布局、自定义组件，随时指导',
      en: 'Dashboard building — components, data binding, layout, custom widgets',
    },
    quickActions: [
      { label: { zh: '添加图表组件', en: 'Add a chart' }, prompt: { zh: '如何在看板中添加一个图表组件并绑定设备指标数据源？', en: 'How do I add a chart widget and bind a device metric as its data source?' } },
      { label: { zh: '调整布局', en: 'Adjust layout' }, prompt: { zh: '讲解看板编辑模式的布局调整：拖拽、缩放、网格对齐', en: 'Explain edit-mode layout: drag, resize, grid snapping' } },
      { label: { zh: '自定义组件开发', en: 'Custom component dev' }, prompt: { zh: '我想写一个自定义看板组件，请说明组件清单格式和代码结构', en: 'I want to write a custom dashboard component — manifest format and code structure' } },
    ],
  },
  automation: {
    systemPromptSuffix: {
      zh: '## 当前页面专注域：自动化\n你是 NeoMind 平台的「自动化助手」。优先引导用户：规则引擎（条件/动作/定时触发）与数据转换的构建和调试。需要实际操作时用 neomind CLI 完成。',
      en: '## Current page focus: automation\nYou are the NeoMind automation assistant. Prioritize: the rule engine (conditions/actions/schedules) and data transforms — building and debugging. Perform real operations via the neomind CLI.',
    },
    tools: [...CORE_TOOLS],
    skillKeywords: ['rule', 'transform', 'automation', 'schedule', '规则', '转换'],
    greeting: {
      zh: '自动化规则与数据转换的构建助手 —— 描述需求，我来帮你拆成规则',
      en: 'Rules & transforms assistant — describe the goal, I will shape it',
    },
    quickActions: [
      { label: { zh: '创建温度报警规则', en: 'Create a temp alert rule' }, prompt: { zh: '帮我创建一个规则：温度超过 30 度时发送告警消息', en: 'Create a rule: send an alert when temperature exceeds 30°' } },
      { label: { zh: '数据转换是什么', en: 'What are transforms?' }, prompt: { zh: '解释数据转换（transforms）的用途，并举两个典型例子', en: 'Explain data transforms with two typical examples' } },
    ],
  },
  data: {
    systemPromptSuffix: {
      zh: '## 当前页面专注域：数据分析与转发\n你是 NeoMind 平台的「数据助手」。优先引导用户：数据源浏览与分析（指标查询/趋势解读）、数据推送到外部系统（Data Push 目标配置）。需要实际操作时用 neomind CLI 完成。',
      en: '## Current page focus: data analysis & forwarding\nYou are the NeoMind data assistant. Prioritize: browsing/analyzing data sources (metric queries, trend reading) and pushing data to external systems (push targets). Perform real operations via the neomind CLI.',
    },
    tools: [...CORE_TOOLS],
    skillKeywords: ['timeseries', 'data', 'push', 'metric', '数据', '推送'],
    greeting: {
      zh: '数据分析与转发 —— 查指标、看趋势、配置推送目标',
      en: 'Data analysis & forwarding — explore metrics, configure push targets',
    },
    quickActions: [
      { label: { zh: '查看设备数据趋势', en: 'Inspect data trends' }, prompt: { zh: '如何在数据页面查询某设备最近的指标数据并分析趋势？', en: 'How do I query a device\u2019s recent metrics and read the trend?' } },
      { label: { zh: '配置数据推送', en: 'Configure data push' }, prompt: { zh: '指导我创建一个数据推送目标，把设备数据转发到外部 HTTP 服务', en: 'Guide me through a push target that forwards device data to an HTTP endpoint' } },
    ],
  },
  messages: {
    systemPromptSuffix: {
      zh: '## 当前页面专注域：消息\n你是 NeoMind 平台的「消息助手」。优先引导用户：通知消息分析、告警上报处理、通知通道（渠道）管理。需要实际操作时用 neomind CLI 完成。',
      en: '## Current page focus: messaging\nYou are the NeoMind messaging assistant. Prioritize: notification analysis, alert triage, and channel management. Perform real operations via the neomind CLI.',
    },
    tools: [...CORE_TOOLS],
    skillKeywords: ['message', 'notification', 'channel', 'alert', '消息', '告警', '通道'],
    greeting: {
      zh: '消息分析、告警处理、通道管理 —— 告诉我你想排查什么',
      en: 'Message analysis, alert triage, channel management',
    },
    quickActions: [
      { label: { zh: '分析最近告警', en: 'Analyze recent alerts' }, prompt: { zh: '帮我分析最近的告警消息，找出最需要关注的问题', en: 'Analyze recent alert messages and surface what needs attention' } },
      { label: { zh: '配置通知通道', en: 'Configure a channel' }, prompt: { zh: '如何添加一个通知通道（如邮件/Webhook）并绑定给规则使用？', en: 'How do I add a notification channel (email/webhook) for rules?' } },
    ],
  },
  extensions: {
    systemPromptSuffix: {
      zh: '## 当前页面专注域：扩展\n你是 NeoMind 平台的「扩展助手」。优先引导用户：扩展市场的安装/更新/卸载管理、扩展代码开发（SDK、能力声明、打包发布）。需要实际操作时用 neomind CLI 完成。',
      en: '## Current page focus: extensions\nYou are the NeoMind extensions assistant. Prioritize: marketplace install/update/uninstall management and extension development (SDK, capability manifest, packaging). Perform real operations via the neomind CLI.',
    },
    tools: [...CORE_TOOLS, ...FILE_TOOLS, 'web_fetch'],
    skillKeywords: ['extension', 'plugin', 'sdk', 'marketplace', '扩展', '插件'],
    greeting: {
      zh: '扩展安装管理 + 扩展开发指导 —— 市场、SDK、打包发布',
      en: 'Extension management & development — marketplace, SDK, packaging',
    },
    quickActions: [
      { label: { zh: '推荐实用扩展', en: 'Recommend extensions' }, prompt: { zh: '根据我的场景推荐几个实用扩展并说明安装步骤', en: 'Recommend useful extensions for my setup and how to install them' } },
      { label: { zh: '开发我的扩展', en: 'Develop an extension' }, prompt: { zh: '我想开发一个自定义扩展，请讲解 SDK 项目结构、能力声明和打包流程', en: 'I want to build an extension — SDK structure, capability manifest, packaging' } },
    ],
  },
}
