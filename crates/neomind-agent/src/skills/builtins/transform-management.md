---
id: transform-management
name: Transform Management & Data Processing
description: Use when the user wants to process or transform data — computing derived/virtual metrics, unit conversion, formulas, JS scripts, averaging/summing telemetry. Covers transform create/test-code/metrics even without saying 'transform' (e.g. '温度转华氏', '算个平均值'). Includes 数据转换/计算/公式/虚拟指标.
category: transform
origin: builtin
priority: 80
token_budget: 10000
triggers:
  keywords: [transform, 转换, 数据转换, data transform, 计算, calculate, formula, 公式, virtual metric, 虚拟指标, 转换数据, process data, 数据处理, 脚本, script, JS code, javascript, 华氏, 摄氏, 单位转换, unit conversion, transform create, transform test-code]
  tool_target:
    - tool: transform
      actions: [list, get, create, update, delete, test-code, metrics, data-sources]
anti_triggers:
  keywords: [dashboard, 仪表盘, agent, 代理, extension develop, 扩展开发, device connect, 设备连接, rule, 规则]
---

# Transform Management & Data Processing

## Command Cheat-Sheet (run these via `shell`)

Always RUN the command yourself and report the real output.

| Command | Purpose |
|---|---|
| `neomind transform executions <id>` | Recent run records (status/error/output) — check when output is empty or failing |
| `neomind transform list` | List all transforms |
| `neomind transform get <id>` | Transform details |
| `neomind transform create` | Create a new transform |
| `neomind transform update <id>` | Update a transform |
| `neomind transform enable <id>` | Enable a transform |
| `neomind transform disable <id>` | Disable a transform |
| `neomind transform delete <id>` | Delete a transform |
| `neomind transform metrics` | List virtual metrics from transforms |
| `neomind transform test-code` | Test transform code |
| `neomind transform data-sources` | List transform data sources |

Transforms process raw metric data into derived values using JavaScript code. They create virtual metrics that can be used in dashboards just like device data.

## CRITICAL: Transform Code Format

### `input` Semantics (Auto-unwrap)

Device data is auto-unwrapped for convenience:

- **Single-key object** (e.g. `{"value": 42}`) → `input = 42` (auto-unwrapped to scalar)
- **Multi-key object** (e.g. `{"temperature": 25, "humidity": 60}`) → `input = {temperature: 25, humidity: 60}` (use `input.temperature`)

```javascript
// Single-key: {"value": 212} → input = 212
return (input - 32) * 5 / 9

// Multi-key: {"temperature": 25, "humidity": 60}
return {feels_like: input.temperature * 1.1, humid: input.humidity}

// Classification (single-key)
if (input > 80) return "good"
if (input > 20) return "ok"
return "low"
```

### Extension Invocation

Call extension commands directly from transform code using `extensions.invoke()`:

```javascript
// Get weather data and combine with device input
const weather = extensions.invoke('weather', 'current', {city: 'Shanghai'});
return {outdoor_temp: weather.temperature, indoor_temp: input}
```

Extension calls are **pre-executed asynchronously** before your code runs — no async/await needed.

**Rules:**
- Must use `return` to output the result
- No imports, no external libraries — plain JS only
- No async/await — synchronous code only (extensions are handled automatically)
- Return can be: scalar, object (multiple metrics), or array

## Scope Levels

| Scope | Syntax | Applies To | When to Use |
|-------|--------|-----------|-------------|
| Global | `global` | All device data | Same formula for all devices (unit conversion, formatting) |
| Device Type | `device_type:<type_id>` | All devices of a specific type | Type-specific processing (e.g., all "temperature_sensor" devices) |
| Device | `device:<device_id>` | Only one specific device | Calibration or device-specific logic |

## CRITICAL: Discover Metrics Before Writing Code

**NEVER guess metric/field names.** Always discover actual names first:

### For Device Metrics (scope = device_type or device)

```bash
# Step 1: Find device ID and type
neomind device list

# Step 2: Check latest data to see actual metric names and structure
neomind device get <DEVICE_ID>
# Output example: {"temperature": 23.5, "humidity": 60, "pressure": 1013}
# → input fields: temperature, humidity, pressure

# Step 3: For single-key data like {"value": 42}, input is auto-unwrapped to 42
# For multi-key data like {"temperature": 23.5, "humidity": 60}, use input.temperature
```

### For Extension Metrics (extensions.invoke)

```bash
# Step 1: List available extensions
neomind extension list

# Step 2: Get extension details — shows available commands and their parameters
neomind extension get <EXTENSION_ID>
# Output includes: commands (name, params, return fields), metrics (field names)

# Step 3: Use the command name and field names from the output
# Example: extension "weather" has command "current" returning {temperature, humidity, description}
# → Code: extensions.invoke('weather', 'current', {city: 'Shanghai'}).temperature
```

### For Existing Transform Outputs

```bash
# Check what virtual metrics already exist
neomind transform metrics

# Check available data sources
neomind transform data-sources
```

**Workflow: Discover → Test → Create — then ACT, don't stop at discovery!**
1. Run discovery commands above to learn actual field names
2. `neomind transform test-code --code '...' --input '<actual_data>'` to verify
3. **`neomind transform create --name ... --code ... --scope ...`** to save — THIS IS THE GOAL. Don't stop at step 1 or 2. The user wants a transform CREATED, not just explored.

**⚠️ Common mistake: discovering field names then stopping without creating.** After `transform data-sources` or `device get`, the NEXT command must be `transform test-code` then `transform create`.

**Output format depends on context**:

- **Rules** use DataSourceId format: `transform:<transform_id>:<field>` (colon-separated) — `transform_id` is the transform's `id` from `transform list`, NOT `output_prefix`; `field` is the plain field name.
- **Dashboards** bind to a transform output using the transform's `id` + plain `field` (frontend: `transformId` + `metricId`, or `sourceId: "transform:{id}"`). NOT the `output_prefix`, and NOT `extensionMetric` — `extensionMetric` is for EXTENSION sources only (`command:field`).

## Command Reference

### Create Transform

```bash
neomind transform create --name '<name>' --scope <scope> --code '<JS>'
```

Required: `--name`, `--scope`, `--code`
Optional: `--output-prefix`, `--description`, `--enabled`

### Test Before Creating

```bash
# Test code with sample input
neomind transform test-code --code 'return (input - 32) * 5 / 9' --input '{"value": 212}'
```

**Always test before creating** to verify the code works correctly.

### List & Inspect

```bash
neomind transform list                  # List all transforms
neomind transform get <ID>              # Get transform details
neomind transform metrics               # List all virtual metrics produced by transforms
neomind transform data-sources          # List available data sources for transforms
```

### Update & Delete

```bash
neomind transform update <ID> --code '<NEW_JS>'    # Update code
neomind transform enable <ID>                       # Enable (unified form)
neomind transform disable <ID>                      # Disable (unified form)
neomind transform delete <ID>                       # Delete transform
```

## Workflow Examples

### Temperature Unit Conversion (F to C)

```bash
# Step 1: Test the code
neomind transform test-code --code 'return (input - 32) * 5 / 9' --input '{"value": 212}'

# Step 2: Create the transform
neomind transform create --name 'Fahrenheit to Celsius' \
  --scope global \
  --code 'return (input - 32) * 5 / 9'

# Step 3: Verify virtual metric exists
neomind transform metrics
```

### Battery Health Classification

```bash
neomind transform test-code --code 'if (input > 80) return "good"; if (input > 20) return "ok"; return "low"' --input '{"value": 15}'

neomind transform create --name 'Battery Health' \
  --scope global \
  --output-prefix battery \
  --code 'if (input > 80) return "good"; if (input > 20) return "ok"; return "low"'
```

### Device-Specific Transform

```bash
neomind transform create --name 'Sensor-001 Calibration' \
  --scope 'device:sensor-001' \
  --output-prefix calibrated \
  --code 'return input * 1.02 + 0.5'
```

### Percentage Formatter

```bash
neomind transform create --name 'Percentage Format' \
  --scope global \
  --code 'return Math.round(input * 100) / 100 + "%"'
```

### Status Mapper

```bash
neomind transform create --name 'Status Text' \
  --scope global \
  --code 'const map = {0: "offline", 1: "online", 2: "warning"}; return map[input] || "unknown"'
```

### Combine Device Data with Extension Data

```bash
# Uses extensions.invoke() to fetch external data and combine with device input
neomind transform test-code --code 'const w = extensions.invoke("weather", "current", {city: "Shanghai"}); return {outdoor: w.temperature, indoor: input}' --input '{"value": 22}'

neomind transform create --name 'Indoor vs Outdoor' \
  --scope global \
  --output-prefix temp_compare \
  --code 'const w = extensions.invoke("weather", "current", {city: "Shanghai"}); return {outdoor: w.temperature, indoor: input}'
```

## Using Transform Output in Dashboards

After creating a transform, its output appears as a virtual metric. Use it in dashboard components:

```bash
# Discover available virtual metrics
neomind transform metrics

# Add to dashboard (transform output uses extension-metric binding)
neomind dashboard add-components <DASHBOARD_ID> --components '[{
  "id": "t1",
  "type": "value-card",
  "title": "Battery Status",
  "position": {"x": 0, "y": 0, "w": 4, "h": 2},
  "data_source": {
    "type": "extension-metric",
    "extensionId": "transform",
    "extensionMetric": "battery.health"
  }
}]'
```

## Common Errors & Solutions

| Error | Cause | Solution |
|-------|-------|----------|
| "Transform code error" | JS syntax error | Test with `transform test-code --code '...' --input '...'` first |
| "Unexpected token return" | Missing semicolons or bad syntax | Use simple one-liner expressions |
| No virtual metric appears | Transform disabled or scope mismatch | Check `transform get <ID>` for enabled status and scope |
| "input is not defined" | Using wrong variable name | Must use `input` (not `inputs` or `value`) |
| Dashboard shows no data | Wrong DataSourceId binding | Check `transform metrics` for correct output names |
| Transform runs but output wrong | Logic error in code | Test with known input values via `transform test-code` |
| `return input * 2` returns NaN/0 | Using `input.value` on auto-unwrapped scalar | Single-key `{"value":X}` is auto-unwrapped — just use `input`, not `input.value` |
| `extensions.invoke is not defined` | Extension not installed | Check `neomind extension list` and verify extension ID |
| Multi-key object: `input * 2` = NaN | Object can't be multiplied | Use `input.temperature * 2` for specific fields |
