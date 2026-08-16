// ========== Rules Types ==========
// Must match backend models (crates/neomind-rules/src/models.rs)

export interface Rule {
  id: string
  name: string
  description?: string
  enabled: boolean
  tags?: string[]
  trigger_count: number
  last_triggered?: string
  created_at: string
  updated_at: string
  // Condition (optional for schedule/manual rules)
  condition?: RuleCondition
  actions: RuleAction[]
  // Trigger type
  trigger: RuleTrigger
  // Auto-generated preview (read-only)
  dsl_preview: string
  // Cooldown in ms (default 60000)
  cooldown?: number
  // Duration condition must hold before triggering (ms)
  for_duration?: number
  // Frontend-only source field for storing original UI state (not sent to backend)
  source?: {
    condition?: RuleCondition
    uiCondition?: unknown
    uiActions?: RuleAction[]
    forDuration?: number
    forUnit?: 'seconds' | 'minutes' | 'hours'
    tags?: string[]
    /** UI extras persisted by the rule builder (round-tripped, not part of
     *  the compiled rule). Typed so consumers stop asserting via `as any`. */
    triggerType?: string
    cronExpression?: string
    cron?: string
    cooldownValue?: number
    cooldownUnit?: string
  }
}

export type RuleTrigger =
  | { trigger_type: 'data_change'; sources: string[] }
  | { trigger_type: 'schedule'; cron: string }
  | { trigger_type: 'manual' }

// Rule condition - supports comparison, range, and logical (AND/OR/NOT) conditions
export interface RuleCondition {
  // Condition type discriminator (tagged union)
  condition_type: 'comparison' | 'range' | 'logical'

  // Comparison condition properties
  source?: string  // DataSourceId format: "device:sensor1:temperature" or "extension:weather:temp"
  operator?: string  // Comparison operators (>, <, >=, <=, ==, !=) or logical operators (and, or, not)
  threshold?: number | string
  threshold_value?: string  // String threshold for string operators (contains, starts_with, ends_with, regex, ==, !=)

  // Range condition properties
  min?: number
  max?: number

  // Logical condition properties (conditions array for and/or/not)
  conditions?: RuleCondition[]
}

export type RuleAction =
  | { type: 'notify'; message: string; severity: 'info' | 'warning' | 'critical' | 'emergency' }
  | { type: 'execute'; target: string; target_type: 'device' | 'extension'; command: string; params: Record<string, unknown> }
  | { type: 'trigger_agent'; agent_id: string; input?: string; data?: unknown }

export interface RuleExecutionResult {
  rule_id: string
  rule_name: string
  success: boolean
  actions_executed: string[]
  error: string | null
  duration_ms: number
  triggered_at: string
}
