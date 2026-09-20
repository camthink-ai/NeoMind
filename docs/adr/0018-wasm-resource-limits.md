# ADR-0018:WASM 三层资源限制

## 状态
生效(2026-09 补齐第三层)

## 背景
wasmtime 26→36 升级(修 RUSTSEC-2026-0096 aarch64 沙箱逃逸)移除了 `static_memory_maximum_size`;该 API 本也只是虚拟内存分配优化,不是真实内存上限 —— 失控扩展可耗尽宿主内存。

## 决策
三层:fuel 计量(CPU,NEOMIND_WASM_FUEL,默认 1M)、模块大小(NEOMIND_WASM_MAX_SIZE_MB,默认 50MB)、每 store `StoreLimits` ResourceLimiter 线性内存上限(NEOMIND_WASM_MEMORY_MB,默认 256MB)。回归测试:wat 模块限额内 grow 成功、超限返回 -1。

## 后果
+ 不可信 WASM 的资源面完整闭环;环境变量与升级前同名同默认。
- 极端合法扩展(>256MB 工作集)需显式调参 —— 文档化即可。
