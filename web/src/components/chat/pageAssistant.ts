/**
 * Page-scoped assistant config — the floating/docked chat panel specializes
 * per route: a focus directive appended to the first message's page context,
 * a greeting, and quick-action prompts. Groundwork for the full
 * "page-dedicated agent" (which will ride the agent/skills system).
 *
 * v1 keeps copy inline (zh/en picked by i18n.language); migrate to
 * chat.json namespaces if the copy stabilizes.
 */

export interface PageQuickAction {
  label: { zh: string; en: string }
  prompt: { zh: string; en: string }
}

export interface PageAssistantConfig {
  /** Appended to the [context] header sent with the first panel message */
  focusHint: { zh: string; en: string }
  /** Panel welcome line while the session is empty */
  greeting: { zh: string; en: string }
  quickActions: PageQuickAction[]
}

const pick = (lang: string, c: { zh: string; en: string }) =>
  lang.startsWith('zh') ? c.zh : c.en

export function pickPageAssistant(
  pathname: string,
  lang: string
): { focusHint: string; greeting: string; quickActions: { label: string; prompt: string }[] } | null {
  const cfg = PAGE_ASSISTANTS[routeKey(pathname)]
  if (!cfg) return null
  return {
    focusHint: pick(lang, cfg.focusHint),
    greeting: pick(lang, cfg.greeting),
    quickActions: cfg.quickActions.map((a) => ({
      label: pick(lang, a.label),
      prompt: pick(lang, a.prompt),
    })),
  }
}

function routeKey(pathname: string): string {
  const seg = pathname.split('/')[1] || ''
  return seg
}

const PAGE_ASSISTANTS: Record<string, PageAssistantConfig> = {
  devices: {
    focusHint: {
      zh: '你当前作为「设备接入助手」：专注引导用户接入产品设备（MQTT/Webhook/蓝牙）、创建模拟设备、构建设备类型、处理待注册设备的准入审批。回答优先给出具体操作步骤。',
      en: 'You are the device-onboarding assistant: guide device onboarding (MQTT/webhook/BLE), simulated devices, device types, and pending-device admission. Prefer concrete step-by-step instructions.',
    },
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
    focusHint: {
      zh: '你当前作为「智能体构建助手」：专注引导用户创建、修改、测试 AI 智能体（提示词、工具、记忆、技能、定时触发）。',
      en: 'You are the agent-building assistant: guide creating, editing, and testing AI agents (prompts, tools, memory, skills, schedules).',
    },
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
    focusHint: {
      zh: '你当前作为「可视化看板助手」：专注看板的创建/编辑/切换、组件管理与新增、自定义组件代码编写、数据源绑定、布局修改。',
      en: 'You are the dashboard assistant: dashboards (create/edit/switch), component management, custom component code, data-source binding, layout editing.',
    },
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
    focusHint: {
      zh: '你当前作为「自动化助手」：专注规则引擎（条件/动作/定时）与数据转换的构建和调试。',
      en: 'You are the automation assistant: rule engine (conditions/actions/schedules) and data transforms — building and debugging.',
    },
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
    focusHint: {
      zh: '你当前作为「数据助手」：专注数据源浏览与分析、数据推送到外部系统（Data Push）。',
      en: 'You are the data assistant: browsing/analyzing data sources and pushing data to external systems.',
    },
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
    focusHint: {
      zh: '你当前作为「消息助手」：专注通知消息分析、告警上报处理、通知通道（渠道）管理。',
      en: 'You are the messaging assistant: notification analysis, alert triage, and channel management.',
    },
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
    focusHint: {
      zh: '你当前作为「扩展助手」：专注扩展市场的安装/更新/卸载管理，以及扩展代码开发（SDK、能力声明、打包）。',
      en: 'You are the extensions assistant: marketplace install/update/uninstall management and extension development (SDK, capabilities, packaging).',
    },
    greeting: {
      zh: '扩展安装管理 + 扩展开发指导 —— 市场、SDK、打包发布',
      en: 'Extension management & development — marketplace, SDK, packaging',
    },
    quickActions: [
      { label: { zh: '推荐实用扩展', en: 'Recommend extensions' }, prompt: { zh: '根据我的场景推荐几个实用扩展并说明安装步骤', en: 'Recommend useful extensions for my setup and how to install them' } },
      { label: { zh: '开发我的扩展', en: 'Develop an extension' }, prompt: { zh: '我想开发一个自定义扩展，请讲解 SDK 项目结构、能力声明和打包流程', en: 'I want to build a custom extension — SDK structure, capability manifest, packaging' } },
    ],
  },
}
