// Prompt templates + schedule constants — split from AgentEditorFullScreen.tsx

import { Zap, Activity, BarChart3, Bell } from 'lucide-react'


// Shared className for inline link-styled buttons (not IconButton — these are
// text links rendered as <button> for accessibility).
export const inlineLinkBtn = 'text-xs text-primary hover:underline'

// ============================================================================
// Constants
// ============================================================================

export const INTERVALS = [5, 10, 15, 30, 60]
export const HOURS = Array.from({ length: 24 }, (_, i) => i)

export const PROMPT_TEMPLATES = [
  { id: 'empty', label: 'Custom', icon: null, description: 'Write your own prompt', template: '' },
  {
    id: 'monitor',
    label: 'Monitor',
    icon: Activity,
    description: 'Monitor data and detect anomalies',
    template: `Monitor the following metrics and alert when anomalies are detected:

Monitoring Targets:
- Data Range: Check if values exceed thresholds (e.g., temperature > 30°C)
- Anomaly Detection: Look for sudden changes or abnormal patterns
- Alert Method: Send notification when conditions are met

Please analyze the current data and compare with historical baselines to identify:
1. Values outside normal range
2. Sudden spikes or drops
3. Data gaps or missing readings

When an anomaly is detected, send an alert with:
- What metric is affected
- Current value vs expected range
- Severity level (info/warning/critical)`
  },
  {
    id: 'control',
    label: 'Control',
    icon: Zap,
    description: 'Automatically control devices',
    template: `Automatically control devices based on the following conditions:

Trigger Conditions:
- Check current sensor readings
- Compare against threshold values
- Verify device states before taking action

Control Actions:
- Device ID: [target device]
- Command: [turn_on / turn_off / adjust]
- Parameters: [any required settings]

Please:
1. First verify the current condition by checking sensor data
2. Only execute commands when the condition is clearly met
3. Confirm the action was successful
4. Avoid rapid repeated switching (add a cooldown between same actions)`
  },
  {
    id: 'analysis',
    label: 'Analysis',
    icon: BarChart3,
    description: 'Analyze trends and generate reports',
    template: `Analyze the following data and generate a comprehensive report:

Analysis Scope:
- Time Range: Use available historical data
- Metrics: All selected metrics
- Comparison: Compare with previous periods if available

Report Contents:
1. **Data Overview**: Summary of current values and status
2. **Trend Analysis**: Increasing, decreasing, or stable patterns
3. **Anomalies**: Any unusual readings or deviations
4. **Correlations**: Relationships between different metrics
5. **Recommendations**: Actionable insights based on the data

Please provide specific numbers and percentages when describing trends and changes.`
  },
  {
    id: 'alert',
    label: 'Alert',
    icon: Bell,
    description: 'Send notifications based on conditions',
    template: `Monitor the selected metrics and send alerts when specific conditions occur:

Alert Conditions:
- Threshold exceeded: When metric goes above/below a value
- Rate of change: When value changes too quickly
- Status change: When device state changes

Alert Content:
- Which metric/device triggered the alert
- Current value and threshold
- Time of occurrence
- Suggested actions if applicable

Please avoid duplicate alerts - only alert when:
1. This is a new incident (not previously reported)
2. The condition has significantly worsened
3. A sufficient cooldown period has passed since the last alert`
  },
]
