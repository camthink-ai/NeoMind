# ADR-0005:`.nep` ABI 版本锁(当前 3)与跨仓库协调

## 状态
生效

## 背景
扩展二进制与宿主之间是 FFI 约定;SDK 任何非兼容变更都会让全部已发布 `.nep` 失效。

## 决策
`CURRENT_ABI_VERSION`/`MIN_ABI_VERSION` 锁在 `neomind-core/src/extension/package.rs`;runner 加载时校验,不匹配即拒绝。SDK 发 crates.io,Extensions 仓库按 workspace 依赖对齐。非兼容 SDK 变更 = 必须协调四仓库同步重发。

## 后果
+ 市场里所有包永远可加载,不会出现"装上就崩"。
- 演进摩擦大:改 ABI 是重大版本事件,倾向只增不改。
