# NeoTalk 部署指南

## 系统要求

### 最低配置

- **CPU**: 2 核心
- **内存**: 4 GB RAM
- **存储**: 10 GB 可用空间
- **操作系统**: Linux (Ubuntu 20.04+, Debian 11+, RHEL 8+)

### 推荐配置

- **CPU**: 4+ 核心
- **内存**: 8+ GB RAM
- **存储**: 50+ GB SSD
- **操作系统**: Ubuntu 22.04 LTS

### LLM 功能要求

如果使用本地 LLM (Ollama):
- **CPU**: 支持 AVX2 的 x86_64 处理器 或 Apple Silicon
- **内存**: 模型参数量 × 2-4 bytes (例如: 7B 模型需要 14-28 GB)

如果使用云端 LLM:
- **网络**: 稳定的互联网连接

## 安装

### 1. 从源码构建

```bash
# 克隆仓库
git clone https://github.com/your-org/NeoTalk.git
cd NeoTalk

# 构建发布版本
cargo build --release

# 二进制文件位置
# - ./target/release/neotalk-server (主服务)
# - ./target/release/neotalk-cli (命令行工具)
```

### 2. 使用预构建二进制

```bash
# 下载最新版本
wget https://github.com/your-org/NeoTalk/releases/latest/download/neotalk-linux-amd64.tar.gz

# 解压
tar xzf neotalk-linux-amd64.tar.gz

# 安装到系统路径
sudo cp neotalk-server /usr/local/bin/
sudo cp neotalk-cli /usr/local/bin/
```

## 配置

### 配置文件位置

默认配置文件位置（按优先级排序）：

1. `./neotalk.toml` (当前目录)
2. `$HOME/.config/neotalk/neotalk.toml`
3. `/etc/neotalk/neotalk.toml`

### 最小配置

```toml
# neotalk.toml
[server]
host = "0.0.0.0"
port = 3000

[llm]
backend = "ollama"
model = "qwen2.5:7b"
endpoint = "http://localhost:11434"

[storage]
data_dir = "/var/lib/neotalk"
```

### 完整配置示例

```toml
[server]
host = "0.0.0.0"
port = 3000
workers = 4

[llm]
backend = "ollama"
model = "qwen2.5:7b"
endpoint = "http://localhost:11434"
api_key = ""

# 云端 LLM 配置 (可选)
# backend = "openai"
# endpoint = "https://api.openai.com/v1"
# api_key = "sk-..."
# model = "gpt-4"

[mqtt]
listen_address = "0.0.0.0"
port = 1883
discovery_prefix = "homeassistant"
auto_discovery = true

[devices]
# 内置 MQTT broker
embedded_broker = true

# 外部 MQTT broker (可选)
# external_brokers = [
#     { id = "broker1", name = "Main Broker", host = "192.168.1.100", port = 1883 }
# ]

[rules]
enabled = true
evaluation_interval_ms = 1000

[workflows]
enabled = true
max_concurrent_executions = 10

[agent]
enabled = true
session_timeout_seconds = 1800

[agent.autonomous]
enabled = true
review_interval_seconds = 3600
auto_execute_threshold = 0.85
max_decision_history = 1000

[storage]
data_dir = "/var/lib/neotalk"
backup_dir = "/var/lib/neotalk/backups"
retention_days = 30

[storage.timeseries]
retention_hours = 720  # 30 days

[storage.events]
retention_hours = 168  # 7 days

[logging]
level = "info"
format = "json"
file = "/var/log/neotalk/neotalk.log"

[logging.file_rotation]
max_size_mb = 100
max_files = 10
```

## 部署方式

### 1. 直接运行

```bash
neotalk-server --config /path/to/neotalk.toml
```

### 2. Systemd 服务

创建服务文件 `/etc/systemd/system/neotalk.service`:

```ini
[Unit]
Description=NeoTalk Server
After=network.target

[Service]
Type=simple
User=neotalk
Group=neotalk
WorkingDirectory=/var/lib/neotalk
ExecStart=/usr/local/bin/neotalk-server --config /etc/neotalk/neotalk.toml
Restart=always
RestartSec=10

# 资源限制
MemoryLimit=4G
CPUQuota=300%

# 安全加固
NoNewPrivileges=true
PrivateTmp=true
ProtectSystem=strict
ProtectHome=true
ReadWritePaths=/var/lib/neotalk /var/log/neotalk

[Install]
WantedBy=multi-user.target
```

启动服务:

```bash
# 创建用户
sudo useradd -r -s /bin/false neotalk

# 创建目录
sudo mkdir -p /var/lib/neotalk /var/log/neotalk /etc/neotalk
sudo chown -R neotalk:neotalk /var/lib/neotalk /var/log/neotalk

# 复制配置文件
sudo cp neotalk.toml /etc/neotalk/

# 启用并启动服务
sudo systemctl enable neotalk
sudo systemctl start neotalk

# 查看状态
sudo systemctl status neotalk
```

### 3. Docker

创建 `Dockerfile`:

```dockerfile
FROM rust:1.83 as builder
WORKDIR /app
COPY . .
RUN cargo build --release

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/neotalk-server /usr/local/bin/

EXPOSE 3000 1883
CMD ["neotalk-server"]
```

构建并运行:

```bash
# 构建镜像
docker build -t neotalk:latest .

# 运行容器
docker run -d \
  --name neotalk \
  -p 3000:3000 \
  -p 1883:1883 \
  -v neotalk-data:/var/lib/neotalk \
  -v $(pwd)/neotalk.toml:/etc/neotalk/neotalk.toml \
  neotalk:latest
```

### 4. Docker Compose

创建 `docker-compose.yml`:

```yaml
version: '3.8'

services:
  neotalk:
    image: neotalk:latest
    container_name: neotalk
    ports:
      - "3000:3000"
      - "1883:1883"
    volumes:
      - neotalk-data:/var/lib/neotalk
      - ./neotalk.toml:/etc/neotalk/neotalk.toml:ro
    environment:
      - RUST_LOG=info
    restart: unless-stopped
    depends_on:
      - ollama

  ollama:
    image: ollama/ollama:latest
    container_name: ollama
    ports:
      - "11434:11434"
    volumes:
      - ollama-data:/root/.ollama
    restart: unless-stopped

volumes:
  neotalk-data:
  ollama-data:
```

运行:

```bash
docker-compose up -d
```

## 配置 Ollama

### 安装 Ollama

```bash
# Linux
curl -fsSL https://ollama.com/install.sh | sh

# macOS
brew install ollama

# 启动服务
ollama serve
```

### 下载模型

```bash
# 轻量级模型 (适合边缘设备)
ollama pull qwen2.5:3b

# 推荐模型
ollama pull qwen2.5:7b

# 多模态模型
ollama pull llava:7b
```

### 配置 NeoTalk 使用 Ollama

```toml
[llm]
backend = "ollama"
model = "qwen2.5:7b"
endpoint = "http://localhost:11434"
```

## 网络配置

### 防火墙规则

```bash
# Web API
sudo ufw allow 3000/tcp

# MQTT
sudo ufw allow 1883/tcp

# MQTT over TLS
sudo ufw allow 8883/tcp
```

### 反向代理 (Nginx)

```nginx
server {
    listen 80;
    server_name neotalk.example.com;

    location / {
        proxy_pass http://localhost:3000;
        proxy_http_version 1.1;
        proxy_set_header Upgrade $http_upgrade;
        proxy_set_header Connection "upgrade";
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
    }

    # WebSocket 支持
    location /api/chat {
        proxy_pass http://localhost:3000;
        proxy_http_version 1.1;
        proxy_set_header Upgrade $http_upgrade;
        proxy_set_header Connection "upgrade";
    }

    # Server-Sent Events
    location /api/events/stream {
        proxy_pass http://localhost:3000;
        proxy_set_header Connection '';
        proxy_buffering off;
        chunked_transfer_encoding on;
    }
}
```

## 数据备份

### 手动备份

```bash
# 停止服务
sudo systemctl stop neotalk

# 备份数据目录
sudo tar -czf neotalk-backup-$(date +%Y%m%d).tar.gz /var/lib/neotalk

# 恢复
sudo tar -xzf neotalk-backup-YYYYMMDD.tar.gz -C /

# 启动服务
sudo systemctl start neotalk
```

### 自动备份

创建 cron 任务 `/etc/cron.daily/neotalk-backup`:

```bash
#!/bin/bash
BACKUP_DIR="/var/backups/neotalk"
DATA_DIR="/var/lib/neotalk"
DATE=$(date +%Y%m%d)

mkdir -p "$BACKUP_DIR"
tar -czf "$BACKUP_DIR/neotalk-$DATE.tar.gz" -C "$DATA_DIR" .

# 保留最近30天的备份
find "$BACKUP_DIR" -name "neotalk-*.tar.gz" -mtime +30 -delete
```

## 监控

### 健康检查

```bash
curl http://localhost:3000/api/health
```

响应:

```json
{
  "status": "healthy",
  "version": "0.1.0",
  "uptime_seconds": 3600,
  "components": {
    "mqtt": "ok",
    "storage": "ok",
    "llm": "ok",
    "event_bus": "ok"
  }
}
```

### Prometheus 指标

NeoTalk 在 `/metrics` 端点暴露 Prometheus 格式的指标：

```bash
curl http://localhost:3000/metrics
```

可用指标：

- `neotalk_events_total` - 事件总数
- `neotalk_devices_online` - 在线设备数
- `neotalk_rules_triggered_total` - 规则触发次数
- `neotalk_llm_requests_total` - LLM 请求总数
- `neotask_workflow_executions_total` - 工作流执行次数

## 日志

### 日志位置

- **系统日志**: `/var/log/neotalk/neotalk.log`
- **Systemd 日志**: `journalctl -u neotalk -f`

### 日志级别

```toml
[logging]
level = "debug"  # trace, debug, info, warn, error
```

### 查看日志

```bash
# 实时查看
sudo journalctl -u neotalk -f

# 查看最近100行
sudo journalctl -u neotalk -n 100

# 按时间过滤
sudo journalctl -u neotalk --since "1 hour ago"
```

## 故障排查

### 服务无法启动

```bash
# 检查配置文件
neotalk-server --config /etc/neotalk/neotalk.toml --check-config

# 查看详细日志
sudo journalctl -u neotalk -n 50 --no-pager
```

### LLM 连接失败

```bash
# 检查 Ollama 服务
curl http://localhost:11434/api/tags

# 检查模型是否已下载
ollama list

# 测试 LLM
ollama run qwen2.5:7b "Hello, NeoTalk!"
```

### MQTT 连接问题

```bash
# 检查端口是否监听
sudo netstat -tlnp | grep 1883

# 测试 MQTT 连接
mosquitto_pub -h localhost -t test -m "hello"
```

### 性能问题

```bash
# 查看资源使用
htop

# 检查数据库大小
du -sh /var/lib/neotalk

# 查看事件积压
curl http://localhost:3000/api/events/stats
```

## 升级

### 升级步骤

```bash
# 1. 备份数据
sudo systemctl stop neotalk
sudo tar -czf neotalk-backup-upgrade.tar.gz /var/lib/neotalk

# 2. 下载新版本
wget https://github.com/your-org/NeoTalk/releases/latest/download/neotalk-linux-amd64.tar.gz

# 3. 替换二进制文件
sudo cp neotalk-server /usr/local/bin/neotalk-server.new
sudo mv /usr/local/bin/neotalk-server /usr/local/bin/neotalk-server.old
sudo mv /usr/local/bin/neotalk-server.new /usr/local/bin/neotalk-server

# 4. 运行数据迁移（如果需要）
neotalk-server --migrate --config /etc/neotalk/neotalk.toml

# 5. 启动服务
sudo systemctl start neotalk

# 6. 验证
sudo systemctl status neotalk
curl http://localhost:3000/api/health
```

### 回滚

```bash
sudo systemctl stop neotalk
sudo mv /usr/local/bin/neotalk-server.old /usr/local/bin/neotalk-server
sudo systemctl start neotalk
```
