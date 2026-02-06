#!/usr/bin/env python3
"""
生成测试数据：Alerts、Commands、Events

使用方法:
    python scripts/generate_test_data.py

环境要求:
    - 服务器运行在 http://localhost:3000
    - Python 3.6+
"""

import requests
import json
import time
from datetime import datetime, timedelta
from typing import List, Dict

BASE_URL = "http://localhost:3000/api"

# 颜色输出
class Colors:
    GREEN = '\033[0;32m'
    YELLOW = '\033[1;33m'
    RED = '\033[0;31m'
    BLUE = '\033[0;34m'
    NC = '\033[0m'


def print_success(msg: str):
    print(f"{Colors.GREEN}✓ {msg}{Colors.NC}")


def print_info(msg: str):
    print(f"{Colors.YELLOW}{msg}{Colors.NC}")


def print_error(msg: str):
    print(f"{Colors.RED}✗ {msg}{Colors.NC}")


def create_alert(title: str, message: str, severity: str, source: str) -> Dict:
    """创建单个 Alert"""
    try:
        response = requests.post(
            f"{BASE_URL}/alerts",
            json={
                "title": title,
                "message": message,
                "severity": severity,
                "source": source
            },
            timeout=5
        )
        if response.status_code == 200:
            return response.json()
        else:
            print_error(f"创建 Alert 失败: {title} - {response.text}")
            return None
    except Exception as e:
        print_error(f"请求失败: {e}")
        return None


def create_device(device_id: str, name: str, device_type: str) -> bool:
    """创建测试设备"""
    try:
        response = requests.post(
            f"{BASE_URL}/devices",
            json={
                "id": device_id,
                "name": name,
                "type": device_type
            },
            timeout=5
        )
        return response.status_code in [200, 409]  # 409 表示已存在
    except Exception as e:
        return False


def send_command(device_id: str, command: str, params: Dict = None) -> Dict:
    """发送设备命令"""
    try:
        url = f"{BASE_URL}/devices/{device_id}/command/{command}"
        response = requests.post(
            url,
            json=params or {},
            timeout=5
        )
        if response.status_code == 200:
            return response.json()
        return None
    except Exception as e:
        return None


def send_telemetry(device_id: str, data: Dict):
    """发送遥测数据"""
    try:
        requests.post(
            f"{BASE_URL}/webhook/telemetry",
            json={
                "device": device_id,
                "timestamp": int(time.time()),
                "data": data
            },
            timeout=5
        )
    except Exception as e:
        pass


# ============================================
# 测试数据定义
# ============================================

ALERTS_DATA = [
    # Emergency (紧急)
    {
        "title": "烟雾检测",
        "message": "厨房传感器检测到烟雾，请立即确认！",
        "severity": "emergency",
        "source": "sensor/kitchen"
    },
    {
        "title": "漏水警报",
        "message": "地下室检测到漏水，水泵已启动",
        "severity": "emergency",
        "source": "sensor/basement"
    },
    {
        "title": "燃气泄漏",
        "message": "厨房燃气传感器检测到异常，请立即检查！",
        "severity": "emergency",
        "source": "sensor/gas"
    },
    # Critical (严重)
    {
        "title": "冰箱温度过高",
        "message": "冰箱内部温度达到 8°C，食物可能变质风险",
        "severity": "critical",
        "source": "sensor/fridge"
    },
    {
        "title": "门锁异常",
        "message": "前门锁连续 3 次开锁失败，可能存在异常尝试",
        "severity": "critical",
        "source": "lock/front"
    },
    {
        "title": "网络中断",
        "message": "网关设备已失去连接超过 10 分钟",
        "severity": "critical",
        "source": "network/monitor"
    },
    {
        "title": "电池电量低",
        "message": "门锁电池电量低于 10%，请及时更换",
        "severity": "critical",
        "source": "lock/front"
    },
    # Warning (警告)
    {
        "title": "温度偏高",
        "message": "客厅温度达到 28°C，超过设定阈值 26°C",
        "severity": "warning",
        "source": "sensor/living"
    },
    {
        "title": "湿度过低",
        "message": "卧室湿度降至 30%，建议开启加湿器",
        "severity": "warning",
        "source": "sensor/bedroom"
    },
    {
        "title": "设备离线警告",
        "message": "传感器 sensor/garden-01 已超过 5 分钟未上报数据",
        "severity": "warning",
        "source": "device_monitor"
    },
    {
        "title": "存储空间不足",
        "message": "系统存储空间使用率超过 85%",
        "severity": "warning",
        "source": "system/monitor"
    },
    {
        "title": "电压异常",
        "message": "检测到电压波动，可能影响设备寿命",
        "severity": "warning",
        "source": "power/monitor"
    },
    # Info (信息)
    {
        "title": "系统启动完成",
        "message": "NeoMind 系统已成功启动，所有服务正常运行",
        "severity": "info",
        "source": "system"
    },
    {
        "title": "固件更新可用",
        "message": "网关设备有新固件版本 v2.1.0 可用",
        "severity": "info",
        "source": "update_manager"
    },
    {
        "title": "定时任务完成",
        "message": "每日数据备份任务已完成",
        "severity": "info",
        "source": "scheduler"
    },
    {
        "title": "设备自动发现",
        "message": "发现 2 个新设备，等待配置",
        "severity": "info",
        "source": "discovery"
    },
    {
        "title": "场景执行成功",
        "message": "「回家模式」场景已自动执行",
        "severity": "info",
        "source": "automation"
    },
]

DEVICES_DATA = [
    {"id": "light/living", "name": "客厅灯", "type": "light"},
    {"id": "light/bedroom", "name": "卧室灯", "type": "light"},
    {"id": "light/kitchen", "name": "厨房灯", "type": "light"},
    {"id": "switch/fan", "name": "客厅风扇", "type": "switch"},
    {"id": "switch/ac", "name": "空调", "type": "hvac"},
    {"id": "sensor/temp", "name": "温湿度传感器", "type": "sensor"},
    {"id": "sensor/door", "name": "门磁传感器", "type": "sensor"},
    {"id": "sensor/motion", "name": "人体感应", "type": "sensor"},
    {"id": "lock/front", "name": "前门锁", "type": "lock"},
    {"id": "curtain/living", "name": "客厅窗帘", "type": "curtain"},
]

TELEMETRY_SAMPLES = [
    {"temperature": 25.5, "humidity": 60, "pressure": 1013},
    {"temperature": 26.2, "humidity": 58, "pressure": 1012},
    {"temperature": 24.8, "humidity": 62, "pressure": 1014},
    {"temperature": 27.1, "humidity": 55, "pressure": 1011},
    {"temperature": 23.5, "humidity": 65, "pressure": 1015},
]


# ============================================
# 主函数
# ============================================

def main():
    print(f"\n{Colors.GREEN}{'='*50}")
    print("  生成测试数据：Alerts、Commands、Events")
    print(f"{'='*50}{Colors.NC}\n")

    # 检查服务器连接
    try:
        response = requests.get(f"{BASE_URL}/health", timeout=5)
        if response.status_code != 200:
            print_error("服务器未响应，请确保服务器运行在 http://localhost:3000")
            return
    except Exception as e:
        print_error(f"无法连接到服务器: {e}")
        print("请确保服务器运行在 http://localhost:3000")
        return

    # 1. 创建 Alerts
    print_info("创建 Alerts...")
    alerts_created = 0
    for alert_data in ALERTS_DATA:
        result = create_alert(**alert_data)
        if result:
            alerts_created += 1
            time.sleep(0.1)  # 避免请求过快
    print_success(f"创建了 {alerts_created} 条 Alerts")

    # 2. 创建设备
    print_info("\n创建测试设备...")
    devices_created = 0
    for device in DEVICES_DATA:
        if create_device(**device):
            devices_created += 1
    print_success(f"创建了 {devices_created} 个设备")

    # 3. 发送 Commands
    print_info("\n发送 Commands...")
    commands = [
        ("light/living", "on", {"brightness": 80}),
        ("light/living", "off", {}),
        ("light/bedroom", "on", {"brightness": 50}),
        ("switch/fan", "set_speed", {"speed": 3}),
        ("curtain/living", "open", {"position": 80}),
    ]
    commands_sent = 0
    for device_id, cmd, params in commands:
        if send_command(device_id, cmd, params):
            commands_sent += 1
        time.sleep(0.1)
    print_success(f"发送了 {commands_sent} 条 Commands")

    # 4. 生成遥测数据
    print_info("\n生成遥测数据...")
    for i, data in enumerate(TELEMETRY_SAMPLES):
        send_telemetry("sensor/temp", data)
        send_telemetry("sensor/door", {"door_open": i % 2 == 0})
        time.sleep(0.1)
    print_success(f"生成了 {len(TELEMETRY_SAMPLES)} 条遥测记录")

    # 5. 显示统计
    print(f"\n{Colors.GREEN}{'='*50}")
    print("  数据统计")
    print(f"{'='*50}{Colors.NC}")

    try:
        # 获取 Alerts 统计
        response = requests.get(f"{BASE_URL}/alerts", timeout=5)
        if response.status_code == 200:
            data = response.json()
            print(f"\n📊 Alerts 总数: {Colors.YELLOW}{data.get('count', 0)}{Colors.NC}")

            # 按严重程度分组
            alerts = data.get('alerts', [])
            severity_count = {}
            for alert in alerts:
                sev = alert.get('severity', 'unknown')
                severity_count[sev] = severity_count.get(sev, 0) + 1

            print(f"  Emergency: {Colors.RED}{severity_count.get('emergency', 0)}{Colors.NC}")
            print(f"  Critical:  {Colors.RED}{severity_count.get('critical', 0)}{Colors.NC}")
            print(f"  Warning:   {Colors.YELLOW}{severity_count.get('warning', 0)}{Colors.NC}")
            print(f"  Info:      {Colors.GREEN}{severity_count.get('info', 0)}{Colors.NC}")

            # 显示最近的几条
            print(f"\n最近的 Alerts:")
            for alert in alerts[:5]:
                print(f"  • [{alert.get('severity', 'info').upper()}] {alert.get('title', 'N/A')}")

        # 获取设备列表
        response = requests.get(f"{BASE_URL}/devices", timeout=5)
        if response.status_code == 200:
            data = response.json()
            devices = data.get('devices', [])
            print(f"\n🔧 设备总数: {Colors.YELLOW}{len(devices)}{Colors.NC}")

    except Exception as e:
        print_error(f"获取统计信息失败: {e}")

    print(f"\n{Colors.GREEN}{'='*50}")
    print_success("测试数据生成完成！")
    print(f"{'='*50}{Colors.NC}\n")

    print(f"前端访问地址: {Colors.BLUE}http://localhost:3000{Colors.NC}")
    print(f"- Alerts 页面: /alerts")
    print(f"- Devices 页面: /devices")
    print(f"- Events 页面: /events\n")


if __name__ == "__main__":
    main()
