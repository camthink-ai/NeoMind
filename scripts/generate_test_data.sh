#!/bin/bash
# 生成测试数据：Alerts、Commands、Events

BASE_URL="http://localhost:3000/api"

# 颜色输出
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
NC='\033[0m' # No Color

echo -e "${GREEN}=== 生成测试数据 ===${NC}\n"

# ============================================
# 1. 创建 Alerts
# ============================================
echo -e "${YELLOW}创建 Alerts...${NC}"

# Info 级别
curl -s -X POST "$BASE_URL/alerts" \
  -H "Content-Type: application/json" \
  -d '{
    "title": "系统启动完成",
    "message": "NeoMind 系统已成功启动，所有服务正常运行",
    "severity": "info",
    "source": "system"
  }' | jq '.'

# Warning 级别
curl -s -X POST "$BASE_URL/alerts" \
  -H "Content-Type: application/json" \
  -d '{
    "title": "设备离线警告",
    "message": "传感器 sensor/garden-01 已超过 5 分钟未上报数据",
    "severity": "warning",
    "source": "device_monitor"
  }' | jq '.'

curl -s -X POST "$BASE_URL/alerts" \
  -H "Content-Type: application/json" \
  -d '{
    "title": "温度偏高",
    "message": "客厅温度达到 28°C，超过设定阈值 26°C",
    "severity": "warning",
    "source": "sensor/living"
  }' | jq '.'

# Critical 级别
curl -s -X POST "$BASE_URL/alerts" \
  -H "Content-Type: application/json" \
  -d '{
    "title": "冰箱温度过高",
    "message": "冰箱内部温度达到 8°C，食物可能变质风险",
    "severity": "critical",
    "source": "sensor/fridge"
  }' | jq '.'

curl -s -X POST "$BASE_URL/alerts" \
  -H "Content-Type: application/json" \
  -d '{
    "title": "门锁异常",
    "message": "前门锁连续 3 次开锁失败，可能存在异常尝试",
    "severity": "critical",
    "source": "lock/front"
  }' | jq '.'

# Emergency 级别
curl -s -X POST "$BASE_URL/alerts" \
  -H "Content-Type: application/json" \
  -d '{
    "title": "烟雾检测",
    "message": "厨房传感器检测到烟雾，请立即确认！",
    "severity": "emergency",
    "source": "sensor/kitchen"
  }' | jq '.'

curl -s -X POST "$BASE_URL/alerts" \
  -H "Content-Type: application/json" \
  -d '{
    "title": "漏水警报",
    "message": "地下室检测到漏水，水泵已启动",
    "severity": "emergency",
    "source": "sensor/basement"
  }' | jq '.'

# 更多 Info 级别
curl -s -X POST "$BASE_URL/alerts" \
  -H "Content-Type: application/json" \
  -d '{
    "title": "固件更新可用",
    "message": "网关设备有新固件版本 v2.1.0 可用",
    "severity": "info",
    "source": "update_manager"
  }' | jq '.'

curl -s -X POST "$BASE_URL/alerts" \
  -H "Content-Type: application/json" \
  -d '{
    "title": "定时任务完成",
    "message": "每日数据备份任务已完成",
    "severity": "info",
    "source": "scheduler"
  }' | jq '.'

echo -e "\n${GREEN}✓ Alerts 创建完成${NC}"

# ============================================
# 2. 创建设备 (用于 Commands)
# ============================================
echo -e "\n${YELLOW}创建测试设备...${NC}"

# 创建一些测试设备
curl -s -X POST "$BASE_URL/devices" \
  -H "Content-Type: application/json" \
  -d '{
    "id": "light/living",
    "name": "客厅灯",
    "type": "light"
  }' > /dev/null

curl -s -X POST "$BASE_URL/devices" \
  -H "Content-Type: application/json" \
  -d '{
    "id": "switch/fan",
    "name": "客厅风扇",
    "type": "switch"
  }' > /dev/null

curl -s -X POST "$BASE_URL/devices" \
  -H "Content-Type: application/json" \
  -d '{
    "id": "sensor/temp",
    "name": "温度传感器",
    "type": "sensor"
  }' > /dev/null

curl -s -X POST "$BASE_URL/devices" \
  -H "Content-Type: application/json" \
  -d '{
    "id": "curtain/window",
    "name": "窗帘",
    "type": "curtain"
  }' > /dev/null

echo -e "${GREEN}✓ 设备创建完成${NC}"

# ============================================
# 3. 发送 Commands (模拟设备控制)
# ============================================
echo -e "\n${YELLOW}发送 Commands...${NC}"

# 开灯命令
curl -s -X POST "$BASE_URL/devices/light/living/command/on" \
  -H "Content-Type: application/json" \
  -d '{"brightness": 80}' | jq '.'

# 关灯命令
curl -s -X POST "$BASE_URL/devices/light/living/command/off" | jq '.'

# 设置风扇速度
curl -s -X POST "$BASE_URL/devices/switch/fan/command/set_speed" \
  -H "Content-Type: application/json" \
  -d '{"speed": 3}' | jq '.'

# 打开窗帘
curl -s -X POST "$BASE_URL/devices/curtain/window/command/open" \
  -H "Content-Type: application/json" \
  -d '{"position": 80}' | jq '.'

echo -e "${GREEN}✓ Commands 发送完成${NC}"

# ============================================
# 4. 生成遥测数据
# ============================================
echo -e "\n${YELLOW}生成遥测数据...${NC}"

# 通过 webhook 发送遥测数据
curl -s -X POST "$BASE_URL/webhook/telemetry" \
  -H "Content-Type: application/json" \
  -d '{
    "device": "sensor/temp",
    "timestamp": '$(date +%s)',
    "data": {
      "temperature": 25.5,
      "humidity": 60,
      "pressure": 1013
    }
  }' > /dev/null

curl -s -X POST "$BASE_URL/webhook/telemetry" \
  -H "Content-Type: application/json" \
  -d '{
    "device": "sensor/temp",
    "timestamp": '$(date +%s)',
    "data": {
      "temperature": 26.2,
      "humidity": 58,
      "pressure": 1012
    }
  }' > /dev/null

echo -e "${GREEN}✓ 遥测数据生成完成${NC}"

# ============================================
# 5. 显示统计
# ============================================
echo -e "\n${GREEN}=== 数据统计 ===${NC}"

echo -e "\n${YELLOW}Alerts 数量:${NC}"
curl -s "$BASE_URL/alerts" | jq '.count'

echo -e "\n${YELLOW}Alerts 列表:${NC}"
curl -s "$BASE_URL/alerts" | jq '.alerts[] | {id, title, severity, status}'

echo -e "\n${YELLOW}设备列表:${NC}"
curl -s "$BASE_URL/devices" | jq '.devices[] | {id, name, type}' 2>/dev/null || echo "需要启动服务器"

echo -e "\n${GREEN}=== 测试数据生成完成 ===${NC}"
