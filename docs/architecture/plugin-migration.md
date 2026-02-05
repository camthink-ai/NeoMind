# Plugin 到 Extension 迁移分析

> NeoMind v0.4.2
> 创建时间: 2025-02-05

## 当前状态

### 两个并行系统

| 系统 | 位置 | 主要用途 | 状态 |
|------|------|----------|------|
| **Plugin** | `neomind-core/src/plugin/` | 设备适配器插件 | 活跃使用 |
| **Extension** | `neomind-core/src/extension/` | 第三方扩展 | 新系统 |

### Plugin 系统

**功能**:
- 编译时注册 + 运行时加载
- 支持 WASM 和 Native (.so/.dylib/.dll)
- 动态配置热重载
- 版本兼容性检查

**使用者**:
- `neomind-devices/src/adapters/plugins.rs` - 设备适配器插件
- `neomind-devices/src/plugin_adapter.rs` - 插件适配器
- 测试代码中的插件加载

**核心组件**:
```rust
pub trait Plugin {
    fn metadata(&self) -> &PluginMetadata;
    fn initialize(&mut self, config: &Value) -> Result<()>;
    fn is_initialized(&self) -> bool;
    fn shutdown(&mut self) -> Result<()>;
}

pub struct UnifiedPluginRegistry {
    // 管理所有插件类型
}

pub struct WasmPluginLoader {
    // WASM 插件加载器
}

pub struct NativePluginLoader {
    // Native 插件加载器
}
```

### Extension 系统

**功能**:
- 运行时动态加载 (.so/.dylib/.dll/.wasm)
- 生命周期管理 (start/stop)
- 健康检查
- 发现机制

**使用者**:
- `neomind-cli` - CLI 工具的扩展管理
- API 端点: `/api/extensions/*`

**核心组件**:
```rust
pub trait Extension {
    fn metadata(&self) -> &ExtensionMetadata;
    fn start(&mut self) -> Result<()>;
    fn stop(&mut self) -> Result<()>;
    fn state(&self) -> ExtensionState;
}

pub struct ExtensionRegistry {
    // 管理扩展生命周期
}

pub struct WasmExtensionLoader {
    // WASM 扩展加载器
}

pub struct NativeExtensionLoader {
    // Native 扩展加载器
}
```

---

## 两个系统的差异

| 特性 | Plugin 系统 | Extension 系统 |
|------|------------|---------------|
| **注册方式** | compile-time + runtime | runtime only |
| **生命周期** | initialize/shutdown | start/stop |
| **状态管理** | PluginState (Loaded, Running, etc.) | ExtensionState (Loaded, Running, Stopped) |
| **元数据** | PluginMetadata | ExtensionMetadata |
| **热重载** | 支持 (ConfigWatcher) | 不直接支持 |
| **健康检查** | 无 | 有 (health endpoint) |
| **API 端点** | 无 | `/api/extensions/*` |
| **发现机制** | 无 | `discover_extensions()` |
| **ABI 版本** | `PLUGIN_ABI_VERSION` | 无 |

---

## 迁移策略

### 方案 A: 统一到 Extension 系统（推荐）

**优点**:
- 单一系统，减少维护成本
- 更完整的生命周期管理
- 有 HTTP API 支持
- 有健康检查机制

**缺点**:
- 需要大量重构
- 可能破坏现有功能
- 需要重新实现 Plugin 的某些功能（如热重载）

**迁移步骤**:

1. **扩展 Extension trait**
   ```rust
   pub trait Extension {
       fn metadata(&self) -> &ExtensionMetadata;
       fn start(&mut self) -> Result<()>;
       fn stop(&mut self) -> Result<()>;
       fn state(&self) -> ExtensionState;
       
       // 添加 Plugin 兼容方法
       fn initialize(&mut self, config: &Value) -> Result<()> {
           // 默认实现，调用 start
           self.start()
       }
   }
   ```

2. **合并 Registry**
   ```rust
   pub struct UnifiedExtensionRegistry {
       // 合并两者的功能
       plugins: HashMap<String, DynExtension>,
       loaders: HashMap<ExtensionType, Box<dyn ExtensionLoader>>,
   }
   ```

3. **更新设备适配器**
   - 修改 `neomind-devices/src/adapters/plugins.rs`
   - 使用 Extension trait 替代 Plugin trait

4. **保留热重载功能**
   - 在 ExtensionRegistry 中添加 ConfigWatcher

---

### 方案 B: 并行保留（保守）

**优点**:
- 无需重构现有代码
- 零风险

**缺点**:
- 两个系统并行，维护成本高
- 概念混淆
- 未来技术债务增加

**实施**:
- 保持现状
- 文档化两个系统的使用场景
- 在新代码中优先使用 Extension 系统

---

## 建议

### 短期 (v0.4.x)
采用 **方案 B** - 并行保留:
1. 文档化两个系统的职责边界
2. 新功能使用 Extension 系统
3. Plugin 系统保持不变

### 中期 (v0.5.x)
开始 **方案 A** 的迁移:
1. 扩展 Extension trait 以支持 Plugin 功能
2. 创建适配层保持兼容
3. 逐步迁移设备适配器

### 长期 (v0.6.x)
完全统一到 Extension 系统:
1. 删除 Plugin 系统
2. 所有扩展使用统一 API
3. 清理技术债务

---

## 当前推荐

**保持现状，暂不迁移**。

原因:
1. Plugin 系统在设备适配器中工作正常
2. Extension 系统主要用于第三方扩展
3. 两者服务不同的使用场景
4. 强行迁移风险大于收益

**文档更新**:
- Plugin 系统: 用于设备适配器插件（内置功能）
- Extension 系统: 用于第三方开发者扩展

---
