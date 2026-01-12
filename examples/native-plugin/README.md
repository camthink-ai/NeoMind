# NeoTalk Native 插件示例

这是一个简单的 NeoTalk native 插件示例，展示了如何创建和编译一个动态库插件。

## 功能

此插件实现了一个简单的"问候工具"，可以：
- 返回默认问候语
- 返回带名字的问候语
- 自定义问候语内容

## 构建

### Linux/macOS

```bash
cd /path/to/NeoTalk
cargo build --example native-plugin --release
```

构建输出：
- Linux: `target/release/libneotalk_example_plugin.so`
- macOS: `target/release/libneotalk_example_plugin.dylib`

### Windows

```bash
cargo build --example native-plugin --release
```

构建输出：`target\release\neotalk_example_plugin.dll`

## 测试

```bash
cargo test --example native-plugin
```

## 使用

### 方式 1: 通过 API 加载

```bash
curl -X POST http://localhost:3000/api/plugins \
  -H "Content-Type: application/json" \
  -d '{
    "path": "/path/to/target/release/libneotalk_example_plugin.so",
    "plugin_type": "tool",
    "auto_start": true
  }'
```

### 方式 2: 放入插件目录

将编译好的动态库复制到插件目录：

```bash
cp target/release/libneotalk_example_plugin.so /path/to/neotalk/plugins/
```

然后通过 API 发现：

```bash
curl -X POST http://localhost:3000/api/plugins/discover
```

## 代码结构

```
src/
└── lib.rs          # 插件主文件
    ├── GreetingPlugin         # 插件实现结构体
    ├── neotalk_plugin_create  # FFI 创建函数
    ├── neotalk_plugin_destroy # FFI 销毁函数
    └── neotalk_plugin_descriptor # 插件元数据
```

## 关键概念

### 1. FFI 导出

插件必须导出三个符号：

```rust
#[no_mangle]
pub extern "C" fn neotalk_plugin_create() -> *mut YourPlugin;

#[no_mangle]
pub extern "C" fn neotalk_plugin_destroy(plugin: *mut YourPlugin);

#[no_mangle]
pub static neotalk_plugin_descriptor: NativePluginDescriptor;
```

### 2. 插件描述符

描述符包含插件的元数据信息：

```rust
pub static neotalk_plugin_descriptor: NativePluginDescriptor = NativePluginDescriptor {
    api_version: NEOTALK_PLUGIN_API_VERSION,  // 必须匹配
    id: ...,
    name: ...,
    version: ...,
    description: ...,
    required_version: ...,
    create: neotalk_plugin_create as *const (),
    destroy: neotalk_plugin_destroy as *const () -> *const (),
};
```

### 3. 字符串处理

所有导出的字符串必须以 null 终止 (`\0`)，长度不包含 null 终止符：

```rust
const PLUGIN_ID: &str = "my-plugin\0";
id_len: PLUGIN_ID.len() - 1,  // 排除 '\0'
```

## 高级示例

### 添加配置支持

```rust
impl GreetingPlugin {
    pub fn configure(&mut self, config: &Value) -> Result<(), PluginError> {
        if let Some(greeting) = config.get("greeting").and_then(|v| v.as_str()) {
            self.greeting = greeting.to_string();
        }
        Ok(())
    }
}
```

### 添加命令处理

```rust
pub struct GreetingPlugin {
    // ...
}

impl GreetingPlugin {
    pub fn handle_command(&self, cmd: &str, args: &Value) -> Result<Value, PluginError> {
        match cmd {
            "greet" => {
                let name = args.get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("World");
                Ok(json!({"result": self.greet(Some(name))}))
            }
            "set_greeting" => {
                // 需要可变引用
                Err(PluginError::ExecutionFailed("Use update method instead".to_string()))
            }
            _ => Err(PluginError::ExecutionFailed(format!("Unknown command: {}", cmd)))
        }
    }
}
```

## 相关文档

- [插件开发指南](../../docs/plugins/PLUGIN_DEVELOPMENT.md)
- [Native 插件 API](../../docs/plugins/NATIVE_PLUGIN_API.md)
- [插件模板](../../docs/plugins/TEMPLATE.md)
