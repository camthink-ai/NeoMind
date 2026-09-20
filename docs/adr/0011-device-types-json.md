# ADR-0011:设备类型 = JSON 模板

## 状态
生效

## 背景
上百种传感器/相机,逐个写代码不现实;设备接入的核心是"指标+命令的声明",不是逻辑。

## 决策
设备类型是 DeviceTypes 仓库里的 JSON 文件(device_type/name/metrics/commands),核心平台只实现 `DeviceTypeTemplate` 加载器。加类型 = 提交一个 JSON,零代码零发版。

## 后果
+ 生态可扩展性极强(已 129 个模板);非程序员也能贡献。
- JSON schema 与 Rust 结构体的耦合:改必填字段破坏全部存量模板(跨仓库破坏性变更清单)。
