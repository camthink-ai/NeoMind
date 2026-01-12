# MDL/DSL 与 LLM 集成架构

## 概述

MDL (Machine Description Language) 和 DSL (Domain Specific Language) 是 NeoTalk 的核心配置，它们不仅用于抽象设备和规则，更是 **LLM 理解设备和规则的核心知识库**。

```
┌─────────────────────────────────────────────────────────────────┐
│                     LLM 理解架构                                 │
├─────────────────────────────────────────────────────────────────┤
│                                                                   │
│   ┌─────────────────┐         ┌─────────────────┐               │
│   │   MDL 知识库    │         │   DSL 知识库    │               │
│   │  (设备理解)     │         │  (规则理解)     │               │
│   ├─────────────────┤         ├─────────────────┤               │
│   │ • 设备类型      │         │ • 规则定义      │               │
│   │ • 能力描述      │         │ • 触发条件      │               │
│   │ • 指标定义      │         │ • 执行动作      │               │
│   │ • 命令接口      │         │ • 约束条件      │               │
│   └────────┬────────┘         └────────┬────────┘               │
│            │                           │                         │
│            └───────────┬───────────────┘                         │
│                        ▼                                         │
│           ┌─────────────────────────┐                           │
│           │   双向翻译层            │                           │
│           │ NL ⇄ MDL, NL ⇄ DSL     │                           │
│           └───────────┬─────────────┘                           │
│                       │                                          │
│                       ▼                                          │
│           ┌─────────────────────────┐                           │
│           │      LLM Tools          │                           │
│           │  • 查询设备类型          │                           │
│           │  • 获取能力描述          │                           │
│           │  • 列出/解释规则         │                           │
│           │  • 生成/修改规则         │                           │
│           └─────────────────────────┘                           │
│                                                                   │
└─────────────────────────────────────────────────────────────────┘
```

---

## 1. MDL 作为设备知识库

### 1.1 LLM 理解 MDL 的方式

LLM 通过以下方式理解设备：

```
用户查询: "客厅有哪些传感器？"
    ↓
LLM 调用: list_devices(location="客厅")
    ↓
返回: 设备实例列表
    ↓
LLM 调用: get_device_type("temperature_sensor")
    ↓
返回: MDL 定义 (温度传感器类型)
    ↓
LLM 理解: 该设备能测量温度，有 uplink 指标，可以设置阈值告警
    ↓
LLM 回答: "客厅有温度传感器，可以监测温度并告警"
```

### 1.2 MDL 知识结构

```rust
/// MDL 设备类型定义 (LLM 友好)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceTypeDefinition {
    /// 类型唯一标识
    pub type_id: String,

    /// 类型名称 (人类可读)
    pub name: String,

    /// 类型描述 (LLM 用此向用户解释)
    pub description: String,

    /// 类别 (用于 LLM 分类理解)
    pub category: DeviceCategory,

    /// 上行数据定义 (设备能报告什么)
    pub uplink: Vec<UplinkMetric>,

    /// 下行指令定义 (设备能接收什么命令)
    pub downlink: Vec<DownlinkCommand>,

    /// 参数配置定义
    pub parameters: Vec<ParameterDefinition>,

    /// LLM 提示词模板
    pub llm_prompt_template: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DeviceCategory {
    Sensor,     // 传感器 - 只读
    Actuator,   // 执行器 - 可控制
    Controller, // 控制器 - 复合设备
    Gateway,    // 网关
    Binary,     // 二进制设备 (开关)
    Enum,       // 枚举设备 (多档位)
}

/// 上行指标定义
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UplinkMetric {
    /// 指标名称
    pub name: String,

    /// 数据类型
    pub data_type: MetricDataType,

    /// 单位
    pub unit: Option<String>,

    /// 取值范围
    pub range: Option<ValueRange>,

    /// 描述 (LLM 解释用)
    pub description: String,

    /// 示例值 (LLM 学习用)
    pub examples: Vec<serde_json::Value>,
}

/// 下行命令定义
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownlinkCommand {
    /// 命令名称
    pub name: String,

    /// 描述 (LLM 理解命令用途)
    pub description: String,

    /// 参数定义
    pub parameters: Vec<CommandParameter>,

    /// 执行结果类型
    pub result_type: Option<String>,

    /// 示例调用 (LLM 学习用)
    pub examples: Vec<CommandExample>,
}
```

### 1.3 LLM 查询 MDL 的工具

```rust
/// LLM 工具: 查询设备类型
#[async_trait]
impl Tool for ListDeviceTypes {
    fn name(&self) -> &str {
        "list_device_types"
    }

    fn description(&self) -> &str {
        "查询系统中所有可用的设备类型，可按类别筛选"
    }

    fn parameters(&self) -> JsonSchema {
        json_schema!({
            "type": "object",
            "properties": {
                "category": {
                    "type": "string",
                    "enum": ["sensor", "actuator", "controller", "gateway"],
                    "description": "设备类别筛选"
                }
            }
        })
    }

    async fn execute(&self, args: serde_json::Value) -> ToolResult {
        let category = args.get("category").and_then(|c| c.as_str());

        let types = MdlRegistry::global()
            .list_types(category)
            .into_iter()
            .map(|t| {
                json!({
                    "type_id": t.type_id,
                    "name": t.name,
                    "category": t.category,
                    "description": t.description,
                    "metrics_count": t.uplink.len(),
                    "commands_count": t.downlink.len(),
                })
            })
            .collect::<Vec<_>>();

        Ok(json!({ "device_types": types }))
    }
}

/// LLM 工具: 获取设备类型详情
#[async_trait]
impl Tool for GetDeviceType {
    fn name(&self) -> &str {
        "get_device_type"
    }

    fn description(&self) -> &str {
        "获取指定设备类型的完整定义，包括所有指标和命令"
    }

    async fn execute(&self, args: serde_json::Value) -> ToolResult {
        let type_id = args["type_id"].as_str().ok_or("missing type_id")?;

        let definition = MdlRegistry::global()
            .get_definition(type_id)
            .ok_or_else(|| format!("Device type '{}' not found", type_id))?;

        Ok(json!(definition))
    }
}

/// LLM 工具: 自然语言解释设备类型
#[async_trait]
impl Tool for ExplainDeviceType {
    fn name(&self) -> &str {
        "explain_device_type"
    }

    fn description(&self) -> &str {
        "用自然语言向用户解释某种设备类型的功能和用途"
    }

    async fn execute(&self, args: serde_json::Value) -> ToolResult {
        let type_id = args["type_id"].as_str().ok_or("missing type_id")?;
        let lang = args.get("lang").and_then(|l| l.as_str()).unwrap_or("zh");

        let definition = MdlRegistry::global()
            .get_definition(type_id)
            .ok_or_else(|| format!("Device type '{}' not found", type_id))?;

        // 使用 LLM 生成自然语言解释
        let prompt = format!(
            "你是 NeoTalk 智能家居助手。请用{}向用户解释以下设备类型：\n\n\
            设备名称: {}\n\
            类别: {:?}\n\
            描述: {}\n\n\
            上行指标: {}\n\n\
            下行命令: {}\n\n\
            请用通俗易懂的语言说明这个设备能做什么，如何使用。",
            lang,
            definition.name,
            definition.category,
            definition.description,
            format_metrics(&definition.uplink),
            format_commands(&definition.downlink)
        );

        let explanation = self.llm.generate(&prompt).await?;

        Ok(json!({ "explanation": explanation }))
    }
}
```

### 1.4 MDL → 自然语言生成

```rust
/// 将 MDL 定义转换为自然语言描述
pub fn mdl_to_natural_language(
    definition: &DeviceTypeDefinition,
    language: &str,
) -> String {
    match language {
        "zh" => describe_in_chinese(definition),
        "en" => describe_in_english(definition),
        _ => describe_in_chinese(definition),
    }
}

fn describe_in_chinese(def: &DeviceTypeDefinition) -> String {
    let mut desc = format!("**{}** ({})\n\n{}\n\n",
        def.name, def.category, def.description);

    if !def.uplink.is_empty() {
        desc.push_str("### 能监测的数据\n\n");
        for metric in &def.uplink {
            desc.push_str(&format!("- **{}**: {}",
                metric.name,
                metric.description
            ));
            if let Some(unit) = &metric.unit {
                desc.push_str(&format!(" (单位: {})", unit));
            }
            if let Some(range) = &metric.range {
                desc.push_str(&format!("，范围: {}", range));
            }
            desc.push('\n');
        }
        desc.push('\n');
    }

    if !def.downlink.is_empty() {
        desc.push_str("### 支持的命令\n\n");
        for cmd in &def.downlink {
            desc.push_str(&format!("- **{}**: {}\n",
                cmd.name, cmd.description));
        }
    }

    desc
}
```

---

## 2. DSL 作为规则知识库

### 2.1 LLM 理解 DSL 的方式

```
用户查询: "当前有哪些温度告警规则？"
    ↓
LLM 调用: list_rules(filter="温度告警")
    ↓
返回: 规则列表 (DSL)
    ↓
LLM 调用: explain_rule(rule_id="rule_001")
    ↓
返回: 自然语言解释
    ↓
LLM 回答: "有3条温度告警规则..."
```

### 2.2 DSL 规则结构 (LLM 友好)

```rust
/// DSL 规则定义
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DslRule {
    /// 规则唯一 ID
    pub rule_id: String,

    /// 规则名称
    pub name: String,

    /// 规则描述 (LLM 理解用)
    pub description: String,

    /// 是否启用
    pub enabled: bool,

    /// 触发条件
    pub trigger: TriggerClause,

    /// 执行动作
    pub actions: Vec<ActionClause>,

    /// 规则元数据
    pub metadata: RuleMetadata,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TriggerClause {
    /// 条件表达式
    pub expression: String,

    /// 持续时间 (FOR 子句)
    pub duration: Option<Duration>,

    /// 自然语言描述 (LLM 生成)
    pub natural_language: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionClause {
    /// 动作类型
    pub action_type: ActionType,

    /// 目标设备
    pub target: String,

    /// 参数
    pub parameters: HashMap<String, serde_json::Value>,

    /// 自然语言描述
    pub natural_language: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ActionType {
    SendMessage,   // 发送消息
    ExecuteCommand,// 执行设备命令
    SetParameter,  // 设置参数
    TriggerWorkflow, // 触发工作流
    LlmDecision,   // LLM 决策
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleMetadata {
    /// 创建时间
    pub created_at: i64,

    /// 创建者 (user/llm)
    pub created_by: String,

    /// 修改历史
    pub history: Vec<RuleChange>,

    /// 执行统计
    pub stats: RuleStats,
}
```

### 2.3 LLM 操作 DSL 的工具

```rust
/// LLM 工具: 列出规则
#[async_trait]
impl Tool for ListRules {
    fn name(&self) -> &str {
        "list_rules"
    }

    fn description(&self) -> &str {
        "列出系统中的所有规则，可按状态或设备筛选"
    }

    async fn execute(&self, args: serde_json::Value) -> ToolResult {
        let filter = args.get("filter");

        let rules = RuleEngine::global()
            .list_rules(filter)
            .into_iter()
            .map(|r| {
                json!({
                    "rule_id": r.rule_id,
                    "name": r.name,
                    "description": r.description,
                    "enabled": r.enabled,
                    "trigger_summary": r.trigger.natural_language,
                })
            })
            .collect::<Vec<_>>();

        Ok(json!({ "rules": rules }))
    }
}

/// LLM 工具: 获取规则详情
#[async_trait]
impl Tool for GetRule {
    fn name(&self) -> &str {
        "get_rule"
    }

    fn description(&self) -> &str {
        "获取指定规则的完整 DSL 定义"
    }

    async fn execute(&self, args: serde_json::Value) -> ToolResult {
        let rule_id = args["rule_id"].as_str().ok_or("missing rule_id")?;

        let rule = RuleEngine::global()
            .get_rule(rule_id)
            .ok_or_else(|| format!("Rule '{}' not found", rule_id))?;

        Ok(json!(rule))
    }
}

/// LLM 工具: 解释规则
#[async_trait]
impl Tool for ExplainRule {
    fn name(&self) -> &str {
        "explain_rule"
    }

    fn description(&self) -> &str {
        "用自然语言向用户解释规则的含义和执行逻辑"
    }

    async fn execute(&self, args: serde_json::Value) -> ToolResult {
        let rule_id = args["rule_id"].as_str().ok_or("missing rule_id")?;
        let lang = args.get("lang").and_then(|l| l.as_str()).unwrap_or("zh");

        let rule = RuleEngine::global()
            .get_rule(rule_id)
            .ok_or_else(|| format!("Rule '{}' not found", rule_id))?;

        let explanation = generate_rule_explanation(rule, lang)?;

        Ok(json!({
            "rule_id": rule_id,
            "name": rule.name,
            "explanation": explanation,
        }))
    }
}

/// LLM 工具: 生成规则 DSL
#[async_trait]
impl Tool for GenerateRuleDsl {
    fn name(&self) -> &str {
        "generate_rule_dsl"
    }

    fn description(&self) -> &str {
        "根据用户的自然语言描述生成规则 DSL"
    }

    async fn execute(&self, args: serde_json::Value) -> ToolResult {
        let description = args["description"].as_str()
            .ok_or("missing description")?;

        // 1. 获取相关设备类型信息
        let device_context = extract_device_context(description);

        // 2. 构建 prompt
        let prompt = format!(
            "你是 NeoTalk 规则生成器。请根据以下用户描述生成 DSL 规则：\n\n\
            用户描述: {}\n\n\
            可用设备类型: {}\n\n\
            请生成标准的 DSL 格式规则。",
            description,
            serde_json::to_string_pretty(&device_context)?
        );

        // 3. 调用 LLM 生成 DSL
        let dsl = self.llm.generate(&prompt).await?;

        // 4. 验证 DSL
        let validated = RuleDslParser::new()
            .parse(&dsl)
            .map_err(|e| format!("Invalid DSL: {}", e))?;

        Ok(json!({
            "dsl": dsl,
            "rule": validated,
        }))
    }
}

/// LLM 工具: 创建规则
#[async_trait]
impl Tool for CreateRule {
    fn name(&self) -> &str {
        "create_rule"
    }

    fn description(&self) -> &str {
        "创建一条新规则并保存到系统"
    }

    async fn execute(&self, args: serde_json::Value) -> ToolResult {
        let name = args["name"].as_str().ok_or("missing name")?;
        let description = args["description"].as_str()
            .ok_or("missing description")?;
        let dsl = args["dsl"].as_str().ok_or("missing dsl")?;

        // 解析 DSL
        let rule = RuleDslParser::new()
            .parse(dsl)
            .map_err(|e| format!("Parse error: {}", e))?;

        // 保存规则
        let rule_id = RuleEngine::global()
            .create_rule(name, description, rule)
            .await?;

        Ok(json!({
            "rule_id": rule_id,
            "status": "created",
        }))
    }
}
```

### 2.4 DSL ↔ 自然语言 双向翻译

```rust
/// DSL 转 natural language
pub fn dsl_to_natural_language(rule: &DslRule, lang: &str) -> String {
    match lang {
        "zh" => explain_rule_chinese(rule),
        "en" => explain_rule_english(rule),
        _ => explain_rule_chinese(rule),
    }
}

fn explain_rule_chinese(rule: &DslRule) -> String {
    format!(
        "规则名称: {}\n\
        规则描述: {}\n\
        触发条件: {}\n\
        执行动作: {}\n\
        状态: {}",
        rule.name,
        rule.description,
        rule.trigger.natural_language
            .as_ref()
            .unwrap_or(&rule.trigger.expression),
        rule.actions.iter()
            .map(|a| a.natural_language.clone().unwrap_or_default())
            .collect::<Vec<_>>()
            .join("; "),
        if rule.enabled { "启用" } else { "禁用" }
    )
}

/// Natural language 转 DSL
pub async fn natural_language_to_dsl(
    description: &str,
    llm: &LlmRuntime,
    available_devices: &[DeviceTypeDefinition],
) -> Result<String, String> {
    let prompt = format!(
        "你是 NeoTalk DSL 生成器。请将以下自然语言描述转换为 DSL 格式：\n\n\
        描述: {}\n\n\
        可用设备类型:\n{}\n\n\
        请输出标准的 DSL 格式：\n\
        RULE \"规则名称\"\n\
        WHEN 触发条件\n\
        FOR 持续时间 (可选)\n\
        DO\n\
          执行动作\n\
        END",
        description,
        format_device_types_for_llm(available_devices)
    );

    llm.generate(&prompt).await
}
```

---

## 3. 上下文注入策略

### 3.1 智能上下文选择

不是所有 MDL/DSL 都需要注入 LLM 上下文，需要智能选择：

```rust
/// 上下文选择器
pub struct ContextSelector {
    mdl_store: Arc<MdlStore>,
    dsl_store: Arc<DslStore>,
    vector_store: Arc<VectorStore>,
}

impl ContextSelector {
    /// 根据用户查询选择相关上下文
    pub async fn select_context(
        &self,
        user_query: &str,
        session_history: &[Message],
    ) -> ContextBundle {
        let mut bundle = ContextBundle::default();

        // 1. 分析用户查询意图
        let intent = IntentAnalyzer::analyze(user_query);

        match intent {
            Intent::DeviceQuery => {
                // 查询相关设备类型
                let device_types = self.find_relevant_device_types(user_query).await;
                bundle.mdl_definitions = device_types;
            }
            Intent::RuleQuery => {
                // 查询相关规则
                let rules = self.find_relevant_rules(user_query).await;
                bundle.dsl_rules = rules;
            }
            Intent::RuleCreation => {
                // 创建规则需要设备类型和示例规则
                bundle.mdl_definitions = self.get_all_device_types().await;
                bundle.dsl_rules = self.get_example_rules().await;
                bundle.dsl_examples = true;
            }
            Intent::DeviceControl => {
                // 设备控制需要目标设备的命令定义
                if let Some(device_id) = extract_device_id(user_query) {
                    bundle.device_commands = self.get_device_commands(device_id).await;
                }
            }
            Intent::GeneralChat => {
                // 一般聊天可能不需要额外上下文
            }
        }

        // 2. 检查上下文大小，必要时精简
        self.trim_context_size(&mut bundle);

        bundle
    }

    /// 语义搜索相关设备类型
    async fn find_relevant_device_types(&self, query: &str) -> Vec<DeviceTypeDefinition> {
        // 使用向量存储进行语义搜索
        let results = self.vector_store
            .search(query, top_k=5)
            .await;

        results.into_iter()
            .filter_map(|r| self.mdl_store.get(&r.id))
            .collect()
    }

    /// 语义搜索相关规则
    async fn find_relevant_rules(&self, query: &str) -> Vec<DslRule> {
        let results = self.vector_store
            .search(query, top_k=5)
            .await;

        results.into_iter()
            .filter_map(|r| self.dsl_store.get(&r.id))
            .collect()
    }
}

#[derive(Debug, Clone, Default)]
pub struct ContextBundle {
    /// MDL 设备类型定义
    pub mdl_definitions: Vec<DeviceTypeDefinition>,

    /// DSL 规则定义
    pub dsl_rules: Vec<DslRule>,

    /// 设备命令详情
    pub device_commands: HashMap<String, Vec<DownlinkCommand>>,

    /// 是否包含 DSL 示例
    pub dsl_examples: bool,

    /// 额外的系统提示词
    pub system_prompts: Vec<String>,
}
```

### 3.2 上下文格式化

```rust
/// 将上下文格式化为 LLM prompt
pub fn format_context_for_llm(bundle: &ContextBundle) -> String {
    let mut prompt = String::new();

    if !bundle.mdl_definitions.is_empty() {
        prompt.push_str("# 设备类型定义 (MDL)\n\n");
        for def in &bundle.mdl_definitions {
            prompt.push_str(&format_mdal_for_llm(def));
            prompt.push_str("\n---\n\n");
        }
    }

    if !bundle.dsl_rules.is_empty() {
        prompt.push_str("# 现有规则 (DSL)\n\n");
        for rule in &bundle.dsl_rules {
            prompt.push_str(&format_rule_for_llm(rule));
            prompt.push_str("\n---\n\n");
        }
    }

    if bundle.dsl_examples {
        prompt.push_str("# DSL 规则示例\n\n");
        prompt.push_str(DSL_EXAMPLES_ZH);
        prompt.push_str("\n---\n\n");
    }

    prompt
}

fn format_mdal_for_llm(def: &DeviceTypeDefinition) -> String {
    format!(
        "## {} ({})\n\
        描述: {}\n\
        类别: {:?}\n\
        上行指标: {}\n\
        下行命令: {}",
        def.name,
        def.type_id,
        def.description,
        def.category,
        def.uplink.iter()
            .map(|m| format!("{}({})", m.name, m.data_type))
            .collect::<Vec<_>>()
            .join(", "),
        def.downlink.iter()
            .map(|c| &c.name)
            .collect::<Vec<_>>()
            .join(", ")
    )
}
```

---

## 4. 向量化存储

为了支持语义搜索，MDL/DSL 定义需要向量化存储：

```rust
/// MDL/DSL 向量化存储
pub struct LlmKnowledgeBase {
    vector_store: Arc<VectorStore>,
    mdl_store: Arc<MdlStore>,
    dsl_store: Arc<DslStore>,
}

impl LlmKnowledgeBase {
    /// 索引 MDL 定义
    pub async fn index_mdl(&self, definition: &DeviceTypeDefinition) {
        // 构建可搜索的文本
        let text = format!(
            "{} {} {} {} {}",
            definition.name,
            definition.description,
            definition.category,
            definition.uplink.iter()
                .map(|m| &m.name).join(" "),
            definition.downlink.iter()
                .map(|c| &c.name).join(" ")
        );

        // 生成嵌入向量
        let embedding = self.embed_text(&text).await;

        // 存储到向量数据库
        self.vector_store.insert(VectorDocument {
            id: definition.type_id.clone(),
            vector: embedding,
            metadata: json!({
                "type": "mdl",
                "name": definition.name,
                "category": definition.category,
            }),
            payload: json!(definition),
        }).await;
    }

    /// 索引 DSL 规则
    pub async fn index_dsl(&self, rule: &DslRule) {
        let text = format!(
            "{} {} {} {}",
            rule.name,
            rule.description,
            rule.trigger.expression,
            rule.actions.iter()
                .map(|a| format!("{:?}", a.action_type))
                .join(" ")
        );

        let embedding = self.embed_text(&text).await;

        self.vector_store.insert(VectorDocument {
            id: rule.rule_id.clone(),
            vector: embedding,
            metadata: json!({
                "type": "dsl",
                "name": rule.name,
                "enabled": rule.enabled,
            }),
            payload: json!(rule),
        }).await;
    }

    /// 语义搜索
    pub async fn semantic_search(
        &self,
        query: &str,
        filter_type: Option<&str>,  // "mdl" or "dsl"
        top_k: usize,
    ) -> Vec<SearchResult> {
        let embedding = self.embed_text(query).await;

        self.vector_store
            .search_with_filter(embedding, |doc| {
                filter_type.map_or(true, |t| {
                    doc.metadata["type"].as_str() == Some(t)
                })
            }, top_k)
            .await
    }
}
```

---

## 5. LLM System Prompt 模板

```rust
/// LLM System Prompt 模板
pub const LLM_SYSTEM_PROMPT: &str = r#"
你是一个 NeoTalk 智能家居助手，能够帮助用户管理设备和规则。

## 能力

### 设备管理
- 查询可用设备类型和设备实例
- 解释设备的功能和用途
- 执行设备命令 (开关、设置参数等)
- 监控设备状态和数据

### 规则管理
- 查看现有规则
- 解释规则的执行逻辑
- 根据用户描述创建新规则
- 修改或删除现有规则

### 数据分析
- 查询历史数据
- 生成数据报告
- 分析趋势和异常

## 可用工具

{{#if tools}}
{{#each tools}}
- `{{this.name}}`: {{this.description}}
{{/each}}
{{/if}}

## 回答规范

1. 使用简洁、专业的语言
2. 涉及设备或规则时，引用其 ID 或名称
3. 建议使用工具查询最新数据
4. 不确定时主动询问用户
5. 执行操作前确认用户意图

## DSL 规则语法

```
RULE "规则名称"
WHEN 触发条件表达式
FOR 持续时间 (可选)
DO
  执行动作列表
END
```

### 触发条件示例
- `device("sensor_001").temperature > 30` - 温度超过 30 度
- `device("door_001").status == "open"` - 门打开状态
- `time() >= "22:00"` - 时间到达 22:00

### 执行动作示例
- `send_message("温度告警")` - 发送消息
- `device("ac_001").set_temperature(26)` - 设置空调温度
- `trigger_workflow("night_mode")` - 触发工作流
"#;

/// 带上下文的 System Prompt
pub fn build_system_prompt(context: &ContextBundle) -> String {
    let mut prompt = LLM_SYSTEM_PROMPT.to_string();

    if !context.mdl_definitions.is_empty() {
        prompt.push_str("\n## 当前相关设备类型\n\n");
        for def in &context.mdl_definitions {
            prompt.push_str(&format!("- {}: {}\n",
                def.name, def.description));
        }
    }

    if !context.dsl_rules.is_empty() {
        prompt.push_str("\n## 当前相关规则\n\n");
        for rule in &context.dsl_rules {
            prompt.push_str(&format!("- {}: {}\n",
                rule.name, rule.description));
        }
    }

    prompt
}
```

---

## 6. 完整工作流示例

### 场景 1: 用户创建规则

```
用户: "当客厅温度超过 30 度持续 5 分钟时，打开空调并设置 26 度"

    ↓ 意图分析

LLM 调用:
1. list_devices(location="客厅") → 找到 temperature_sensor, air_conditioner
2. get_device_type("temperature_sensor") → 获取温度传感器 MDL
3. get_device_type("air_conditioner") → 获取空调 MDL

    ↓ 理解设备能力

LLM 知道:
- 温度传感器有 uplink: temperature (float)
- 空调有 downlink: power_on, set_temperature

    ↓ 生成 DSL

LLM 生成:
```
RULE "客厅高温自动开空调"
WHEN device("living_room_temp").temperature > 30
FOR 5m
DO
  device("living_room_ac").power_on()
  device("living_room_ac").set_temperature(26)
END
```

    ↓ 确认并保存

LLM: "我将创建规则：当客厅温度超过 30 度持续 5 分钟时，
      自动打开空调并设置到 26 度。确认创建吗？"

用户: "确认"

LLM 调用 create_rule() → 规则保存
```

### 场景 2: 用户查询规则

```
用户: "有哪些温度相关的规则？"

    ↓ 语义搜索

LLM 调用:
1. list_rules(filter="温度") → 返回相关规则列表
2. explain_rule(rule_id="rule_001") → 解释每条规则

    ↓ 自然语言生成

LLM 回答:
"当前有 3 条与温度相关的规则：

1. **客厅高温告警**
   当客厅温度超过 30 度持续 5 分钟时触发，
   会发送告警消息。

2. **夜间恒温控制**
   在夜间 22:00-06:00，如果温度低于 20 度，
   自动打开暖气设置到 24 度。

3. **空调节能模式**
   当检测到无人且温度适宜时，自动关闭空调。"
```

---

## 7. 实现任务清单

### P0: 核心 MDL/DSL LLM 集成
- [ ] 实现 `ListDeviceTypes` LLM 工具
- [ ] 实现 `GetDeviceType` LLM 工具
- [ ] 实现 `ExplainDeviceType` LLM 工具
- [ ] 实现 `ListRules` LLM 工具
- [ ] 实现 `GetRule` LLM 工具
- [ ] 实现 `ExplainRule` LLM 工具

### P1: 规则生成
- [ ] 实现 `GenerateRuleDsl` LLM 工具
- [ ] 实现 `CreateRule` LLM 工具
- [ ] 实现 DSL 验证器
- [ ] 添加规则创建确认流程

### P2: 上下文管理
- [ ] 实现 `ContextSelector`
- [ ] 实现语义搜索集成
- [ ] 实现 MDL/DSL 向量化索引
- [ ] 优化上下文大小管理

### P3: 双向翻译
- [ ] 实现 MDL → Natural Language 转换
- [ ] 实现 DSL → Natural Language 转换
- [ ] 实现 Natural Language → DSL 转换
- [ ] 添加多语言支持 (中文/英文)

---

## 总结

MDL 和 DSL 是 LLM 理解 NeoTalk 系统的核心知识库：

1. **MDL** 让 LLM 理解设备能力
   - 设备能报告什么数据
   - 设备能接收什么命令
   - 如何用自然语言描述设备

2. **DSL** 让 LLM 理解自动化规则
   - 规则的触发条件
   - 规则的执行动作
   - 如何解释和生成规则

3. **双向翻译** 实现 LLM 与系统的无缝交互
   - NL → MDL/DSL: 用户意图转换为系统配置
   - MDL/DSL → NL: 系统配置转换为用户可理解的说明

4. **智能上下文** 确保高效对话
   - 按需注入相关 MDL/DSL
   - 语义搜索快速定位
   - 控制上下文大小

这使得 LLM 能够真正深入到 NeoTalk 的业务流程中，成为系统的大脑而不仅是对话界面。
