# NeoMind 配置参考

面向部署与运维的配置清单。所有条目均为可选;默认值取自源码(`NEOMIND_*` 环境变量在服务启动时读取一次)。wiki.camthink.ai 的用户文档可镜像本页。

## 数据目录

| 变量 | 默认 | 说明 |
| --- | --- | --- |
| `NEOMIND_DATA_DIR` | `./data`(相对工作目录) | 所有 redb 存储与扩展安装目录的根。systemd 部署由 `WorkingDirectory` 保证。 |
| `NEOMIND_STRICT_DATA_DIR` | 未设置 | 任意非空值即生效:彻底禁用旧版"cwd 相对 `data/` 回退探测",数据目录完全由 `NEOMIND_DATA_DIR` 决定。 |

背景:设置了 `NEOMIND_DATA_DIR` 且工作目录下恰好存在旧版 `data/` 时,历史兼容逻辑可能把部分存储静默留在旧位置(数据"分裂")。服务启动时会检测这种情况并打印 **DATA-DIR SPLIT** 错误横幅列出受影响的存储;要彻底避免,设置 `NEOMIND_STRICT_DATA_DIR`(测试、工具链和多实例环境推荐始终设置)。

## 网络与 TLS

| 变量 | 默认 | 说明 |
| --- | --- | --- |
| `NEOMIND_HOST` | `0.0.0.0` | HTTP 监听地址。 |
| `NEOMIND_PORT` | `9375` | HTTP 监听端口。 |
| `NEOMIND_TLS_PORT` | `9376` | 内置 TLS 反向代理端口(明文监听保持在 loopback)。 |
| `NEOMIND_TLS_CERT` / `NEOMIND_TLS_KEY` | 未设置 | 两个都设为 PEM 路径后启用 TLS 监听。 |
| `NEOMIND_MQTT_BIND` | 未设置 | 覆盖内嵌 MQTT broker 的监听地址(桌面应用使用)。 |
| `NEOMIND_WEB_DIR` | `/var/www/neomind` | 前端静态文件目录;目录内有文件时由服务直接托管。 |

## WASM 扩展沙箱

每个扩展实例三层资源上限:

| 变量 | 默认 | 说明 |
| --- | --- | --- |
| `NEOMIND_WASM_MEMORY_MB` | `256` | 线性内存硬上限(MB)。超限的 `memory.grow` 返回失败而非拖垮宿主进程。 |
| `NEOMIND_WASM_FUEL` | `1000000` | wasmtime fuel 配额(CPU 上限)。 |
| `NEOMIND_WASM_MAX_SIZE_MB` | `50` | 单个 `.wasm` 模块文件大小上限(MB)。 |

## 存储与备份

| 变量 | 默认 | 说明 |
| --- | --- | --- |
| `NEOMIND_TELEMETRY_CACHE_MB` | `256` | 遥测 redb 页缓存上限(MB),小内存设备可调低。 |
| `NEOMIND_BACKUP_INTERVAL_SECS` | `86400`(24 小时) | 出厂默认首次启动即启用定时备份;`0` 表示出厂禁用(仍可在 Web 界面开启);生效值最小 300 秒。 |
| `NEOMIND_BACKUP_KEEP` | `3` | 备份保留份数,有效范围 1–50。 |

## 安全

| 变量 | 默认 | 说明 |
| --- | --- | --- |
| `NEOMIND_JWT_SECRET` | 未设置 → 自动生成并持久化 | 单实例无需设置;多实例共享用户库的部署必须显式设置,否则各实例签发的令牌互不认账。 |

## 日志

| 变量 | 默认 | 说明 |
| --- | --- | --- |
| `NEOMIND_LOG_JSON` | 未设置 | 设为真值时 CLI 日志输出 JSON 格式(机器可读,供桌面端等采集)。 |

## Web 设置项(非环境变量)

**会话保留**(`session_retention_hours`):在 Web **设置 → 偏好设置** 中配置。默认**永久保留**;设置后服务每小时清理一次超期的聊天会话,用于长期运行、磁盘有限 的边缘部署防止 `sessions.redb` 无限增长。

## API 附注:会话历史分页

`GET /api/sessions/:id/history` 支持 `limit` 与 `before`(记录原始索引游标)分页参数,返回含 `total` 与 `has_more`。分页边界带碎片保护:翻页起点会回退到最近一条 `user` 消息,保证不会把一轮对话(1 条 user + 3 条 assistant 记录)从中间切开。
