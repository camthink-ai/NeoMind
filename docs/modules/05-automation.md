# Automation 模块

**包名**: `neomind-automation`
**版本**: 0.1.0
**完成度**: 75%
**用途**: 数据转换、自动化和意图分析

## 概述

Automation模块提供数据转换引擎、自然语言转自动化、设备类型生成等功能。

## 模块结构

```
crates/automation/src/
├── lib.rs                      # 公开接口
├── transform.rs                # 转换引擎
├── types.rs                    # 类型定义
├── conversion.rs               # 类型转换
├── discovery.rs                # 数据发现
├── intent.rs                   # 意图分析
├── nl2automation.rs            # NL2Automation
├── threshold_recommender.rs    # 阈值推荐
├── device_type_generator.rs    # 设备类型生成
└── store.rs                    # 存储层
```

## 核心功能

### 1. TransformEngine - 转换引擎

```rust
pub struct TransformEngine {
    /// JS执行环境
    js_runtime: Rc<RefCell<JsRuntime>>,
}

impl TransformEngine {
    /// 创建转换引擎
    pub fn new() -> Self;

    /// 执行转换
    pub async fn execute(
        &self,
        transform: &TransformAutomation,
        input: &serde_json::Value,
    ) -> Result<TransformResult>;

    /// 验证转换
    pub validate(&self, transform: &TransformAutomation) -> Result<()>;
}
```

### 2. TransformAutomation - 转换定义

```rust
pub struct TransformAutomation {
    /// 转换ID
    pub id: String,

    /// 转换名称
    pub name: String,

    /// 转换范围
    pub scope: TransformScope,

    /// 转换操作
    pub operations: Vec<TransformOperation>,
}

pub enum TransformScope {
    /// 特定设备
    Device(String),

    /// 设备类型
    DeviceType(String),

    /// 全局
    Global,
}
```

### 3. TransformOperation - 转换操作

```rust
pub enum TransformOperation {
    /// 字段映射
    Map {
        mappings: HashMap<String, String>,
    },

    /// 时间窗口聚合
    TimeWindow {
        window: TimeWindow,
        aggregation: AggregationFunc,
    },

    /// 数组聚合
    ArrayAggregation {
        json_path: String,
        aggregation: AggregationFunc,
        value_path: Option<String>,
        output_metric: String,
    },

    /// JavaScript表达式
    Expression {
        code: String,
    },

    /// 管道
    Pipeline {
        stages: Vec<TransformOperation>,
    },

    /// 条件分支
    If {
        condition: String,
        then_op: Box<TransformOperation>,
        else_op: Option<Box<TransformOperation>>,
    },

    /// 分支执行
    Fork {
        branches: Vec<TransformOperation>,
    },

    /// 自定义WASM
    Custom {
        wasm_module: Vec<u8>,
        function_name: String,
    },
}
```

### 4. JsTransformExecutor - JS执行器

```rust
pub struct JsTransformExecutor {
    /// Boa JS运行时
    runtime: Rc<RefCell<JsRuntime>>,
}

impl JsTransformExecutor {
    /// 执行JS表达式
    pub fn execute(
        &self,
        code: &str,
        input: &serde_json::Value,
    ) -> Result<serde_json::Value>;

    /// 注册自定义函数
    pub fn register_function(
        &mut self,
        name: &str,
        func: NativeFunction,
    );
}
```

**内置JS函数**:
```javascript
// 数学函数
Math.abs(x)
Math.floor(x)
Math.ceil(x)
Math.round(x)

// 字符串函数
str.toUpperCase(s)
str.toLowerCase(s)
str.substring(s, start, end)

// 数组函数
arr.sum(array)
arr.avg(array)
arr.max(array)
arr.min(array)

// 时间函数
time.now()
time.format(timestamp, format)
```

## 数据发现

```rust
pub struct DataPathExtractor {
    /// JSON路径提取器
    extractor: JsonPathExtractor,
}

impl DataPathExtractor {
    /// 从示例数据中提取路径
    pub fn extract_paths(
        &self,
        data: &serde_json::Value,
    ) -> Vec<DiscoveredPath>;

    /// 推断字段语义类型
    pub fn infer_semantic_type(
        &self,
        path: &str,
        value: &serde_json::Value,
    ) -> SemanticType;
}

pub enum SemanticType {
    Temperature,
    Humidity,
    Pressure,
    Boolean,
    Enum(Vec<String>),
    Unknown,
}
```

## NL2Automation

```rust
pub struct Nl2Automation {
    /// LLM运行时
    llm: Arc<dyn LlmRuntime>,
}

impl Nl2Automation {
    /// 从自然语言生成自动化
    pub async fn generate(
        &self,
        description: &str,
    ) -> Result<SuggestedAutomation>;

    /// 提取实体
    pub async fn extract_entities(
        &self,
        text: &str,
    ) -> Result<ExtractedEntities>;
}

pub struct ExtractedEntities {
    pub triggers: Vec<TriggerEntity>,
    pub conditions: Vec<ConditionEntity>,
    pub actions: Vec<ActionEntity>,
}
```

## 阈值推荐

```rust
pub struct ThresholdRecommender {
    /// 历史数据窗口
    window_size: usize,
}

impl ThresholdRecommender {
    /// 分析数据并推荐阈值
    pub async fn recommend(
        &self,
        data: &[f64],
        intent: ThresholdIntent,
    ) -> ThresholdRecommendation;

    /// 验证阈值合理性
    pub fn validate(
        &self,
        threshold: f64,
        statistics: &Statistics,
    ) -> ThresholdValidation;
}

pub enum ThresholdIntent {
    /// 检测异常高值
    DetectHigh,

    /// 检测异常低值
    DetectLow,

    /// 检测离群值
    DetectOutliers,

    /// 检测趋势变化
    DetectTrendChange,
}
```

## 设备类型生成

```rust
pub struct DeviceTypeGenerator {
    /// LLM运行时
    llm: Arc<dyn LlmRuntime>,
}

impl DeviceTypeGenerator {
    /// 从示例数据生成设备类型
    pub async fn generate_from_sample(
        &self,
        sample_data: &serde_json::Value,
        device_info: &DeviceInfo,
    ) -> Result<GeneratedDeviceType>;

    /// 验证生成的类型
    pub fn validate(
        &self,
        device_type: &DeviceTypeTemplate,
    ) -> ValidationResult;
}

pub struct GeneratedDeviceType {
    pub device_type: String,
    pub name: String,
    pub description: String,
    pub metrics: Vec<MetricDefinition>,
    pub commands: Vec<CommandDefinition>,
}
```

## 自动入板

```rust
pub struct AutoOnboardManager {
    /// 设备注册表
    registry: Arc<DeviceRegistry>,

    /// 生成器
    generator: DeviceTypeGenerator,

    /// 阈值推荐器
    recommender: ThresholdRecommender,
}

impl AutoOnboardManager {
    /// 处理待确认设备
    pub async fn process_draft_device(
        &self,
        draft: DraftDevice,
    ) -> Result<RegistrationResult>;

    /// 从示例生成设备类型
    pub async fn generate_device_type(
        &self,
        sample: &DeviceSample,
    ) -> Result<GeneratedDeviceType>;
}

pub struct DraftDevice {
    pub id: String,
    pub source: String,
    pub sample_data: serde_json::Value,
    pub status: DraftDeviceStatus,
}

pub enum DraftDeviceStatus {
    Pending,
    Processing,
    Ready,
    Failed(String),
}
```

## API端点

```
# Transforms
GET    /api/transforms                       # 列出转换
POST   /api/transforms                       # 创建转换
GET    /api/transforms/:id                   # 获取转换
PUT    /api/transforms/:id                   # 更新转换
DELETE /api/transforms/:id                   # 删除转换
POST   /api/transforms/:id/enable            # 启用/禁用
POST   /api/transforms/:id/test              # 测试转换
GET    /api/transforms/:id/history           # 转换历史

# Discovery
POST   /api/transforms/discover               # 数据发现
POST   /api/transforms/generate-type         # 生成设备类型

# Thresholds
POST   /api/thresholds/recommend             # 推荐阈值
POST   /api/thresholds/validate              # 验证阈值

# Draft Devices
GET    /api/devices/pending                  # 待确认设备
POST   /api/devices/pending/:id/confirm      # 确认设备
DELETE /api/devices/pending/:id/dismiss      # 忽略设备
```

## 使用示例

### 创建数据转换

```rust
use neomind-automation::{TransformAutomation, TransformOperation, TransformScope};

let transform = TransformAutomation::new(
    "avg_temperature",
    "计算平均温度",
    TransformScope::DeviceType("sensor".to_string()),
)
.with_operation(TransformOperation::ArrayAggregation {
    json_path: "$.sensors".to_string(),
    aggregation: AggregationFunc::Mean,
    value_path: Some("temp".to_string()),
    output_metric: "temperature_avg".to_string(),
});

let result = engine.execute(&transform, &input_data).await?;
```

### JavaScript表达式

```rust
let transform = TransformAutomation::new(
    "temp_conversion",
    "温度单位转换",
    TransformScope::Global,
)
.with_operation(TransformOperation::Expression {
    code: r#"
        // 摄氏度转华氏度
        input.temp * 1.8 + 32
    "#.to_string(),
});
```

### 自然语言生成规则

```rust
use neomind-automation::Nl2Automation;

let nl2auto = Nl2Automation::new(llm);

let suggested = nl2auto.generate(
    "当温度超过30度时，发送告警"
).await?;

// suggested 包含:
// - trigger: DeviceMetric { metric: "temperature", compare: Gt, value: 30 }
// - condition: ...
// - action: SendAlert { message: "温度过高" }
```

### 阈值推荐

```rust
use neomind-automation::{ThresholdRecommender, ThresholdIntent};

let recommender = ThresholdRecommender::new(100);

let data = vec![22.5, 23.1, 22.8, 23.5, 22.9, 23.2];
let recommendation = recommender.recommend(&data, ThresholdIntent::DetectHigh).await?;

println!("推荐阈值: {}", recommendation.threshold);
println!("置信度: {}", recommendation.confidence);
```

## 转换操作状态

| 操作 | 状态 | 说明 |
|------|------|------|
| Map | ✅ | 字段映射完整实现 |
| TimeWindow | ✅ | 时间窗口聚合完整 |
| ArrayAggregation | ✅ | 数组聚合完整 |
| Expression | ✅ | JS表达式执行完整 |
| Pipeline | 🟡 | 基础实现 |
| Fork | 🟡 | 基础实现 |
| If | 🟡 | 基础实现 |
| Custom/WASM | ❌ | 未实现 |

## 设计原则

1. **JS优先**: 使用JavaScript作为转换语言
2. **类型推断**: 自动推断数据类型
3. **自然语言**: 支持从自然语言生成配置
4. **可测试**: 所有转换都可以测试
