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
 * localStorage key for a page's panel session. v3: the stored value is
 * {id, fp} — the backend applies the page profile (prompt suffix + tool
 * allowlist) ONLY at session creation, so a session outliving a profile
 * change (release update, language switch) would silently keep the OLD
 * config forever. The fingerprint detects drift and drops the stale
 * session; a fresh one with the CURRENT profile is created lazily.
 * v2 stored a bare session id — treated as stale.
 */
export const PANEL_SESSION_PREFIX = 'neomind:panelSession:v3:'

export function panelSessionKey(pageKey: string): string {
  return PANEL_SESSION_PREFIX + (pageKey || 'default')
}

/** Structural fingerprint of what rides session creation (tools + suffix). */
export function profileFingerprint(a: ResolvedPageAssistant | null): string {
  if (!a) return 'generic'
  return a.tools.join(',') + '|' + a.systemPromptSuffix
}

interface StoredPanelSession { id: string; fp: string }

/**
 * Read a page's stored session id, but ONLY when its profile fingerprint
 * still matches — a stale entry (old profile / v2 format) is removed and
 * returns null so the next send creates a session with the current config.
 */
export function readStoredPanelSession(pageKey: string, currentFp: string): string | null {
  const key = panelSessionKey(pageKey)
  try {
    const raw = localStorage.getItem(key)
    if (!raw) return null
    const parsed = JSON.parse(raw) as Partial<StoredPanelSession>
    if (parsed?.id && parsed.fp === currentFp) return parsed.id
    localStorage.removeItem(key)
    return null
  } catch {
    localStorage.removeItem(key)
    return null
  }
}

export function writeStoredPanelSession(pageKey: string, id: string, fp: string): void {
  localStorage.setItem(panelSessionKey(pageKey), JSON.stringify({ id, fp }))
}

/** Tools every page profile keeps (domain filtering only trims specialists). */
const CORE_TOOLS = ['shell', 'skill', 'memory', 'vision']
const FILE_TOOLS = ['file_write', 'file_edit']

const PAGE_ASSISTANTS: Record<string, PageAssistantConfig> = {
  devices: {
    systemPromptSuffix: {
      zh: `## 当前页面专注域：设备接入
你是 NeoMind 平台的「设备接入助手」。优先引导用户：产品设备接入（MQTT/Webhook/蓝牙）、创建模拟设备、构建设备类型（指标/命令）、待注册设备的准入审批。需要实际操作时用 neomind CLI 完成。回答优先给出具体操作步骤。

## 模拟设备 SOP（客户没有实体设备时的标准路径）
客户说"没有设备 / 想先体验 / 需要测试数据"时，按此 SOP 代办（每步可解释）。设备不局限于内置类型——客户想要什么就模拟什么（温湿度、摄像头、门磁、水质、电表……）：
1. 明确场景：如果客户还没说清要什么设备，先问一句用途/场景（要测什么量、有没有控制命令），再继续。
2. 匹配类型：\`neomind device types list\`（可 \`types get <id>\` 看指标明细）找贴近的内置类型；有就用它。
3. 没有就定制：\`neomind device types create --id water_quality --name "水质检测仪" --metrics '[{"name":"ph","display_name":"pH 值","data_type":"Float","unit":""},{"name":"turbidity","display_name":"浊度","data_type":"Float","unit":"NTU"}]'\`（--id 必须是 ASCII 英文/数字/下划线，中文名不会自动生成 ID；按客户场景定义指标，可选 --commands 控制命令；用户也可以在页面上用「AI 生成设备类型」）。
4. 创建模拟设备（webhook 适配器，无需真实硬件）：\`neomind device create --name "模拟<场景>设备" --device-type <类型ID> --adapter-type webhook --id sim-<场景>-01\`。
5. 模拟上报：\`neomind device write-metric sim-<场景>-01 --metric <指标名> --value <合理值>\`（每个指标都写，2-3 个渐变值形成趋势；指标名以第 2/3 步的类型定义为准，值要符合物理直觉）。
6. 验证：\`neomind device get <ID>\` 看到 current_values 更新；设备页面会自动刷新（DataChanged），指给用户看；下一步可在可视化看板绑定该设备。
说明：MQTT 路线等效（设备 topic 向内嵌 broker 1883 发布同样 JSON）；持续模拟可建议客户接入真实设备或写定时规则。`,
      en: `## Current page focus: device onboarding
You are the NeoMind device-onboarding assistant. Prioritize: product device onboarding (MQTT/webhook/BLE), simulated devices, device types (metrics/commands), pending-device admission. Perform real operations via the neomind CLI. Prefer concrete step-by-step instructions.

## Simulated-device SOP (standard path when the customer has no hardware)
When the customer says "no devices / want to try it / need test data", execute this SOP on their behalf (explain each step). Not limited to built-in types — simulate whatever the customer wants (TH sensor, camera, door contact, water quality, power meter, …):
1. Clarify the scenario: if the customer hasn't said what device they want, ask one question first (what to measure, any control commands), then proceed.
2. Match a type: \`neomind device types list\` (and \`types get <id>\` for metric details) for a close built-in type; use it if it fits.
3. Otherwise create a custom type: \`neomind device types create --id water_quality --name "Water Quality Probe" --metrics '[{"name":"ph","display_name":"pH","data_type":"Float","unit":""},{"name":"turbidity","display_name":"Turbidity","data_type":"Float","unit":"NTU"}]'\` (--id must be ASCII letters/digits/underscores — CJK names do not auto-generate an id; define metrics for the scenario, optional --commands; the user can also use the page's AI type generator).
4. Create the simulated device (webhook adapter — no real hardware): \`neomind device create --name "Simulated <scenario>" --device-type <type-id> --adapter-type webhook --id sim-<scenario>-01\`.
5. Simulate data: \`neomind device write-metric sim-<scenario>-01 --metric <metric> --value <plausible value>\` (write every metric, 2-3 varying values to form a trend; metric names come from the type defined in steps 2/3, values should be physically plausible).
6. Verify: \`neomind device get <ID>\` shows updated current_values; the devices page refreshes automatically (DataChanged) — point it out; next step is binding the device on a dashboard.
Notes: the MQTT route works equally (publish the same JSON to the device topic on the embedded broker, port 1883); for continuous simulation suggest a real device or a scheduled rule.`,
    },
    tools: [...CORE_TOOLS],
    skillKeywords: ['device', 'mqtt', 'onboarding', 'simulated', '设备', '接入', '模拟'],
    greeting: {
      zh: '我在这里帮你完成设备接入 —— 没有实体设备？我可以直接帮你创建模拟设备跑通全流程，也支持 MQTT/扫码接入、设备类型、待注册准入',
      en: 'Here to help with device onboarding — no hardware? I can create a simulated device and walk the full flow, plus MQTT/scan setup, types, and admissions',
    },
    quickActions: [
      { label: { zh: '创建模拟设备（任意场景）', en: 'Create a simulated device' }, prompt: { zh: '我想要一个模拟设备。请先问我想模拟什么场景/设备，然后按模拟设备 SOP 帮我代办：匹配或定制设备类型、创建模拟设备、上报几轮数据，完成后告诉我如何验证和在看板上使用。', en: 'I want a simulated device. Ask me what scenario/device to simulate first, then follow the simulated-device SOP: match or create a device type, create the device, report a few rounds of data, and tell me how to verify and use it on a dashboard.' } },
      { label: { zh: '如何接入一台设备？', en: 'How to onboard a device?' }, prompt: { zh: '详细说明接入一台新设备的完整步骤（MQTT / Webhook / 蓝牙）', en: 'Walk me through onboarding a new device (MQTT / webhook / BLE)' } },
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
      zh: '搭建可视化看板 —— 新建看板、加组件、改组件、计算指标(平均/温差)、自定义组件，随时指导',
      en: 'Dashboard building — create boards, add & tweak widgets, computed metrics (avg/diff), custom widgets',
    },
    quickActions: [
      { label: { zh: '新建看板', en: 'Create a dashboard' }, prompt: { zh: '帮我创建一个新看板。先问我想要监控什么（哪些设备/指标），然后创建看板并添加合适的组件、绑定好数据源。', en: 'Create a new dashboard for me. Ask what I want to monitor (devices/metrics) first, then create the board, add fitting widgets, and bind their data sources.' } },
      { label: { zh: '添加图表组件', en: 'Add a chart' }, prompt: { zh: '在看板中添加一个图表组件并绑定设备指标数据源', en: 'Add a chart widget to this dashboard and bind a device metric as its data source' } },
      { label: { zh: '修改组件配置', en: 'Tweak a widget' }, prompt: { zh: '我想修改当前看板里某个组件的配置（标题、数据绑定、时间窗口、单位等），先看看现在有哪些组件再帮我改', en: 'I want to change a widget on this dashboard (title, data binding, time window, unit…) — list the current widgets first, then apply my change' } },
      { label: { zh: '多设备计算值', en: 'Computed metric' }, prompt: { zh: '我想在看板上显示多个设备指标的计算值（比如两个传感器的平均温度或温差），不要用 transform，直接帮我加一个表达式组件', en: "I want a card showing a computed value across devices (e.g. average temp or temp difference of two sensors) — add an expression widget directly, no transform" } },
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
      { label: { zh: '转换没输出排查', en: 'Debug a transform' }, prompt: { zh: '我建的数据转换没有产生指标，帮我查最近的执行记录，定位是代码问题还是没触发', en: "My transform produces no metrics — check its recent execution records and tell me whether the code failed or it never triggered" } },
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
