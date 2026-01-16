# Automation 模块增强计划

## 概述

本文档详细规划了 Rules（规则引擎）和 Workflows（工作流引擎）的增强功能。

---

## 第一部分：规则引擎增强

### 1.1 持久化存储

#### 后端实现

| 文件 | 任务 | 说明 |
|------|------|------|
| `crates/rules/src/store.rs` | 新建 | 规则持久化存储层 |
| `crates/rules/src/history.rs` | 扩展 | 历史记录持久化 |
| `crates/api/src/handlers/rules.rs` | 扩展 | 添加导入/导出端点 |

**存储结构：**
```rust
// 使用 redb 存储规则定义
// data/rules.redb
//   - rules_table: id -> CompiledRule JSON
//   - history_table: (rule_id, timestamp) -> ExecutionRecord
```

**API 端点：**
```
GET  /api/rules/export          # 导出所有规则 (JSON/YAML)
POST /api/rules/import          # 导入规则
GET  /api/rules/templates       # 获取规则模板
```

#### 前端实现

| 文件 | 任务 | 说明 |
|------|------|------|
| `web/src/components/automation/RulesTab.tsx` | 扩展 | 添加导入/导出按钮 |
| `web/src/components/automation/RuleFormDialog.tsx` | 新建 | 规则编辑表单对话框 |

---

### 1.2 LLM 生成规则

#### 后端实现

| 文件 | 任务 | 说明 |
|------|------|------|
| `crates/rules/src/llm_generator.rs` | 新建 | LLM 规则生成器 |

**生成流程：**
```
用户自然语言描述
    ↓
提取关键信息（设备、指标、条件、动作）
    ↓
验证资源可用性（设备是否存在、告警渠道是否配置）
    ↓
生成规则 DSL
    ↓
返回可编辑的规则结构
```

**API 端点：**
```
POST /api/rules/generate
Request: {
  "description": "当温度传感器1超过50度时发送告警",
  "context": {...}  // 当前系统上下文（可用设备、告警渠道等）
}
Response: {
  "rule": { ... },
  "explanation": "生成的规则说明...",
  "confidence": 0.95,
  "missing_resources": []
}
```

#### 前端实现

| 文件 | 任务 | 说明 |
|------|------|------|
| `web/src/components/automation/RuleLLMGenerate.tsx` | 新建 | LLM 生成规则界面 |
| `web/src/components/automation/RuleEditForm.tsx` | 新建 | 规则编辑表单 |

**交互流程：**
1. 用户点击"AI 生成规则"
2. 弹出对话框，输入自然语言描述
3. 调用生成 API
4. 展示生成的规则预览
5. 用户可编辑修正
6. 保存规则

---

### 1.3 可编辑表单

#### 表单结构

```typescript
interface RuleForm {
  name: string;
  description: string;
  enabled: boolean;

  // 条件配置
  condition: {
    deviceId: string;      // 下拉选择（从可用设备）
    metric: string;        // 下拉选择（从设备支持的指标）
    operator: '>' | '<' | '=' | '>=' | '<=';
    threshold: number;
    forDuration?: number;  // 可选，持续时长（秒）
  };

  // 动作配置
  actions: Array<{
    type: 'notify' | 'execute' | 'log';
    config: ActionConfig;
  }>;
}
```

#### 资源校验

| 校验项 | 说明 | 错误提示 |
|--------|------|----------|
| 设备存在性 | 验证设备 ID 是否在系统中 | "设备不存在" |
| 指标可用性 | 验证设备是否支持该指标 | "设备不支持此指标" |
| 告警渠道 | 验证告警渠道是否已配置 | "请先配置告警渠道" |
| 阈值范围 | 根据指标类型校验阈值合理性 | "阈值超出合理范围" |

#### 前端组件结构

```
RuleEditForm
├── BasicInfoSection        // 基本信息
│   ├── name (input)
│   ├── description (textarea)
│   └── enabled (switch)
│
├── ConditionSection         // 条件配置
│   ├── deviceId (select)
│   ├── metric (select - 动态)
│   ├── operator (select)
│   ├── threshold (number)
│   └── forDuration (number - optional)
│
├── ActionsSection           // 动作配置
│   └── ActionList
│       ├── ActionItem (notify)
│       ├── ActionItem (execute)
│       └── AddActionButton
│
├── ValidationSummary        // 校验摘要
│   └── 显示可用资源状态
│
└── ActionButtons
    ├── Test Rule (测试)
    └── Save Rule (保存)
```

---

### 1.4 实现优先级

| 优先级 | 功能 | 工作量 |
|--------|------|--------|
| P0 | 持久化存储 | 3天 |
| P0 | 资源校验 API | 2天 |
| P1 | 基础编辑表单 | 3天 |
| P1 | LLM 生成规则 | 4天 |
| P2 | 导入/导出功能 | 2天 |
| P2 | 规则模板库 | 2天 |

**小计：约 16 天**

---

## 第二部分：工作流引擎增强

### 2.1 LLM 生成器

#### 后端实现

| 文件 | 任务 | 说明 |
|------|------|------|
| `crates/workflow/src/llm_generator.rs` | 扩展 | 增强现有生成器 |
| `crates/api/src/handlers/workflows.rs` | 扩展 | 添加生成端点 |

**生成策略：**

```rust
// 提示词模板
pub struct WorkflowGenerationPrompt {
    pub user_description: String,
    pub available_devices: Vec<Device>,
    pub available_steps: Vec<StepType>,
    pub templates: Vec<WorkflowTemplate>,
}

// 生成流程
1. 分析用户需求，提取关键动作
2. 匹配合适的步骤类型
3. 生成步骤序列
4. 添加必要的条件和错误处理
5. 返回可编辑的工作流 JSON
```

**API 端点：**
```
POST /api/workflows/generate
Request: {
  "description": "每天早上8点检查所有温度传感器，如果超过30度就开空调",
  "mode": "simple" | "full"  // 简单模式或完整模式
}
Response: {
  "workflow": { ... },
  "steps_summary": ["查询温度", "条件判断", "发送命令"],
  "explanation": "工作流包含3个步骤...",
  "warnings": ["部分设备可能不支持..."]
}
```

#### 前端实现

| 文件 | 任务 | 说明 |
|------|------|------|
| `web/src/components/automation/WorkflowLLMGenerate.tsx` | 新建 | LLM 生成工作流界面 |

---

### 2.2 模板库

#### 预置模板

| 模板名称 | 场景 | 步骤数 |
|----------|------|--------|
| 定时巡检 | 定期检查设备状态 | 3 |
| 温度告警处理 | 高温时开空调并通知 | 4 |
| 设备重启 | 自动重启故障设备 | 3 |
| 数据采集汇总 | 收集并上报数据 | 2 |
| 批量控制 | 同时控制多个设备 | 2 |
| 条件分支 | 根据条件执行不同操作 | 4 |
| 异常恢复 | 检测异常并尝试恢复 | 5 |

**模板结构：**
```typescript
interface WorkflowTemplate {
  id: string;
  name: string;
  description: string;
  category: 'monitoring' | 'control' | 'automation' | 'recovery';
  difficulty: 'beginner' | 'intermediate' | 'advanced';

  // 模板参数（用户需要填写的）
  parameters: Array<{
    key: string;
    label: string;
    type: 'device' | 'metric' | 'threshold' | 'channel' | 'text';
    required: boolean;
  }>;

  // 基础工作流结构
  baseWorkflow: Workflow;
}
```

**API 端点：**
```
GET /api/workflows/templates         # 获取所有模板
GET /api/workflows/templates/:id     # 获取模板详情
POST /api/workflows/from-template    # 从模板创建工作流
```

---

### 2.3 简化模式

#### 简化模式步骤类型

| 步骤类型 | 名称 | 说明 |
|----------|------|------|
| `device_query` | 查询设备 | 获取设备状态/数值 |
| `condition` | 条件判断 | 根据结果分支 |
| `send_alert` | 发送告警 | 通知用户 |
| `send_command` | 发送命令 | 控制设备 |
| `delay` | 等待 | 延迟执行 |

**简化模式特点：**
- 隐藏高级步骤（WASM、HTTP请求、图像处理等）
- 提供预设配置选项
- 线性执行为主，减少复杂嵌套
- 更友好的表单界面

#### 前端实现

| 文件 | 任务 | 说明 |
|------|------|------|
| `web/src/components/automation/WorkflowSimpleMode.tsx` | 新建 | 简化模式编辑器 |
| `web/src/components/automation/WorkflowAdvancedMode.tsx` | 新建 | 高级模式编辑器 |

**模式切换：**
```typescript
interface WorkflowEditorProps {
  mode: 'simple' | 'advanced';
  onModeChange: (mode: 'simple' | 'advanced') => void;
}

// 简单模式 → 高级模式：转换工作流结构
// 高级模式 → 简单模式：过滤不支持的高级步骤
```

---

### 2.4 可视化编辑器

#### 技术选型

| 方案 | 优点 | 缺点 | 推荐度 |
|------|------|------|--------|
| React Flow | 轻量、灵活 | 需要自行实现很多功能 | ⭐⭐⭐ |
| Reactflow (原 React Flow) | 文档完善、社区活跃 | 商业授权限制 | ⭐⭐⭐⭐ |
| Svelte Flow | 性能好 | 需要集成到 React 项目 | ⭐⭐ |
| 自研 | 完全可控 | 开发成本高 | ⭐ |

**推荐使用：** `Reactflow` (Apache 2.0 许可的版本)

#### 编辑器功能

**第一阶段（基础版）：**
- [ ] 节点拖拽添加
- [ ] 节点连线
- [ ] 节点属性编辑
- [ ] 删除节点/连线
- [ ] 保存/加载工作流

**第二阶段（增强版）：**
- [ ] 撤销/重做
- [ ] 复制/粘贴节点
- [ ] 画布缩放/平移
- [ ] 小地图导航
- [ ] 自动布局

**第三阶段（高级版）：**
- [ ] 实时预览执行
- [ ] 断点调试
- [ ] 执行动画
- [ ] 性能分析
- [ ] 版本对比

#### 节点类型定义

```typescript
interface FlowNode {
  id: string;
  type: NodeType;  // 设备查询、条件、告警等
  position: { x: number; y: number };
  data: {
    label: string;
    config: StepConfig;
    status?: 'idle' | 'running' | 'success' | 'error';
    error?: string;
  };
}

interface FlowEdge {
  id: string;
  source: string;
  target: string;
  sourceHandle?: string;  // then/else for condition nodes
  targetHandle?: string;
  label?: string;
}
```

#### 组件结构

```
WorkflowVisualEditor
├── Toolbar
│   ├── AddNodeButton (dropdown)
│   ├── UndoButton
│   ├── RedoButton
│   ├── ZoomControls
│   ├── AutoLayoutButton
│   └── SaveButton
│
├── NodePalette (drag source)
│   ├── DeviceQueryNode
│   ├── ConditionNode
│   ├── SendAlertNode
│   ├── SendCommandNode
│   └── DelayNode
│
├── FlowCanvas (ReactFlow)
│   ├── Nodes (dropped)
│   ├── Edges (connections)
│   ├── Background
│   └── Minimap
│
├── PropertiesPanel (right sidebar)
│   ├── NodeProperties (when node selected)
│   │   ├── BasicInfo
│   │   ├── StepConfig
│   │   └── ValidationStatus
│   └── WorkflowProperties (when canvas selected)
│       ├── Name/Description
│       ├── Triggers
│       └── Variables
│
└── BottomPanel
    ├── ExecutionLog
    └── ValidationErrors
```

---

### 2.5 实现优先级

| 优先级 | 功能 | 工作量 |
|--------|------|--------|
| P0 | 模板库后端 + API | 3天 |
| P0 | LLM 生成器增强 | 4天 |
| P1 | 简化模式编辑器 | 5天 |
| P1 | 可视化编辑器（基础版） | 8天 |
| P2 | 可视化编辑器（增强版） | 5天 |
| P2 | 可视化编辑器（高级版） | 5天 |

**小计：约 30 天**

---

## 总体实施计划

### 阶段划分

```
┌─────────────────────────────────────────────────────────────────┐
│                        总工期：约 6-8 周                         │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│  第 1-2 周  │  规则引擎增强                                      │
│             │  - 持久化存储                                      │
│             │  - 资源校验 API                                    │
│             │  - 基础编辑表单                                    │
│                                                                  │
│  第 3-4 周  │  LLM 生成功能                                      │
│             │  - 规则 LLM 生成                                   │
│             │  - 工作流 LLM 生成                                 │
│             │  - 生成结果编辑界面                                │
│                                                                  │
│  第 5-6 周  │  工作流模板库 + 简化模式                           │
│             │  - 模板库设计和实现                                │
│             │  - 简化模式编辑器                                  │
│                                                                  │
│  第 7-8 周  │  可视化编辑器                                      │
│             │  - ReactFlow 集成                                  │
│             │  - 基础编辑功能                                    │
│                                                                  │
│  后续迭代   │  高级功能                                          │
│             │  - 可视化编辑器增强                                │
│             │  - 执行调试功能                                    │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘
```

### 文件清单

#### 规则引擎增强

```
crates/rules/src/
├── store.rs                    # 新建：持久化存储
├── llm_generator.rs            # 新建：LLM 生成器
├── validator.rs                # 新建：资源校验
├── templates.rs                # 新建：规则模板
└── lib.rs                      # 修改：导出新模块

crates/api/src/handlers/
├── rules.rs                    # 修改：添加新端点
└── mod.rs                      # 修改：注册路由

web/src/components/automation/
├── RuleEditDialog.tsx          # 新建：规则编辑对话框
├── RuleEditForm.tsx            # 新建：规则编辑表单
├── RuleLLMGenerate.tsx         # 新建：LLM 生成界面
├── RuleTemplateDialog.tsx      # 新建：模板选择对话框
├── RulesTab.tsx                # 修改：集成新功能
└── RuleValidationSummary.tsx   # 新建：校验摘要组件
```

#### 工作流引擎增强

```
crates/workflow/src/
├── llm_generator.rs            # 修改：增强生成器
├── templates.rs                # 新建：模板定义
├── simple_mode.rs              # 新建：简化模式支持
└── lib.rs                      # 修改：导出新模块

crates/api/src/handlers/
├── workflows.rs                # 修改：添加新端点
└── mod.rs                      # 修改：注册路由

web/src/components/automation/
├── WorkflowLLMGenerate.tsx     # 新建：LLM 生成界面
├── WorkflowSimpleMode.tsx      # 新建：简化模式编辑器
├── WorkflowAdvancedMode.tsx    # 新建：高级模式编辑器
├── WorkflowTemplateDialog.tsx  # 新建：模板选择
├── WorkflowVisualEditor/
│   ├── index.tsx               # 新建：可视化编辑器主组件
│   ├── NodePalette.tsx         # 新建：节点面板
│   ├── PropertiesPanel.tsx     # 新建：属性面板
│   ├── Toolbar.tsx             # 新建：工具栏
│   ├── nodes/
│   │   ├── DeviceQueryNode.tsx # 新建：设备查询节点
│   │   ├── ConditionNode.tsx   # 新建：条件节点
│   │   ├── SendAlertNode.tsx   # 新建：告警节点
│   │   ├── SendCommandNode.tsx # 新建：命令节点
│   │   └── index.ts
│   └── types.ts                # 新建：类型定义
└── WorkflowsTab.tsx            # 修改：集成新功能
```

### 依赖安装

```bash
# 前端依赖
cd web
npm install @xyflow/react  # ReactFlow
npm install @xyflow/minimap
npm install @xyflow/background
npm install @radix-ui/react-select
npm install @radix-ui/react-switch
npm install @radix-ui/react-tabs
npm install react-hook-form
npm install zod  # 表单校验
```

---

## 风险与挑战

| 风险 | 影响 | 缓解措施 |
|------|------|----------|
| LLM 生成质量不稳定 | 用户体验差 | 提供编辑功能，允许用户修正 |
| 可视化编辑器复杂度高 | 开发周期延长 | 分阶段实施，先基础后高级 |
| 资源校验依赖其他模块 | 需要协调 | 定义清晰的接口契约 |
| 前端状态管理复杂 | 维护困难 | 使用状态管理库 (Zustand) |

---

## 成功标准

### 规则引擎
- [x] 规则可持久化存储，重启不丢失
- [x] 用户可通过自然语言生成规则
- [x] 编辑表单提供实时资源校验
- [x] 支持 5+ 种规则模板

### 工作流引擎
- [x] 用户可通过自然语言生成工作流
- [x] 提供 7+ 种预置模板
- [x] 简化模式只暴露 5 种步骤类型
- [x] 可视化编辑器支持拖拽创建工作流
