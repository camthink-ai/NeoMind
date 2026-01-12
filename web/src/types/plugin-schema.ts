/**
 * Unified Plugin UI Schema
 *
 * This standard defines how plugins describe their UI configuration,
 * enabling the frontend to automatically render cards and forms.
 */

// ============================================================================
// Core Field Types
// ============================================================================

export type FieldType =
  | 'string'      // Text input
  | 'text'        // Multiline text area
  | 'number'      // Number input
  | 'boolean'     // Checkbox/switch
  | 'select'      // Dropdown selection
  | 'multiselect' // Multi-select dropdown
  | 'password'    // Password input (secret)
  | 'url'         // URL input with validation
  | 'email'       // Email input with validation
  | 'json'        // JSON editor
  | 'array'       // Array of items
  | 'object'      // Nested object
  | 'keyvalue'    // Key-value pairs

// ============================================================================
// Field Schema
// ============================================================================

export interface FieldSchema {
  // Basic properties
  name: string
  type: FieldType
  label: string
  description?: string

  // Value constraints
  default?: unknown
  required?: boolean
  placeholder?: string

  // Number constraints
  minimum?: number
  maximum?: number
  step?: number

  // String constraints
  minLength?: number
  maxLength?: number
  pattern?: string

  // Options for select/multiselect
  options?: Array<{
    value: string | number
    label: string
    description?: string
  }>

  // Array/Object types
  itemSchema?: FieldSchema        // For array items
  properties?: Record<string, FieldSchema>  // For object properties
  keySchema?: FieldSchema         // For keyvalue keys
  valueSchema?: FieldSchema       // For keyvalue values

  // UI hints
  secret?: boolean           // Mask the value (passwords, API keys)
  readonly?: boolean         // Display only
  hidden?: boolean           // Don't show in UI
  group?: string            // Group related fields
  order?: number            // Display order

  // Conditional visibility
  showWhen?: {
    field: string
    equals?: unknown
    notEquals?: unknown
    contains?: string
  }

  // Advanced
  dependsOn?: string[]       // Other fields this field depends on
  validate?: string          // Custom validation rule name
}

// ============================================================================
// Plugin Category & Type
// ============================================================================

export type PluginCategory = 'ai' | 'devices' | 'storage' | 'notify' | 'integration' | 'tools'

export type PluginType =
  | 'llm_backend'
  | 'device_adapter'
  | 'storage_backend'
  | 'alert_channel'
  | 'integration'
  | 'tool'

// ============================================================================
// Plugin UI Schema
// ============================================================================

export interface PluginUISchema {
  // Plugin identification
  id: string
  type: PluginType
  category: PluginCategory

  // Display info
  name: string
  description: string
  version: string
  icon?: string              // Icon name (lucide-react)

  // Capabilities
  canAddMultiple?: boolean   // Can create multiple instances
  builtin?: boolean          // Is a built-in plugin (cannot be removed)

  // Configuration fields
  fields: Record<string, FieldSchema>

  // Field groups (for organizing complex forms)
  groups?: Record<string, {
    label: string
    description?: string
    collapsible?: boolean
    defaultCollapsed?: boolean
  }>

  // Actions
  actions?: PluginAction[]

  // Status indicators
  statusFields?: {
    running: {
      icon?: string
      label: string
    }
    stopped: {
      icon?: string
      label: string
    }
    error: {
      icon?: string
      label: string
    }
  }

  // Display template for list view
  listTemplate?: {
    showStatus?: boolean
    showDeviceCount?: boolean
    showConfig?: boolean
    configDisplay?: (config: Record<string, unknown>) => string
  }
}

// ============================================================================
// Re-exports for convenience
// ============================================================================

// ============================================================================
// Plugin Actions
// ============================================================================

export interface PluginAction {
  id: string
  label: string
  icon?: string
  variant?: 'default' | 'destructive' | 'outline' | 'ghost'
  dangerous?: boolean        // Requires confirmation
  confirmMessage?: string    // Confirmation message
  apiEndpoint?: string       // REST API endpoint
  method?: 'GET' | 'POST' | 'PUT' | 'DELETE'
}

// ============================================================================
// Plugin Instance (runtime data)
// ============================================================================

export interface PluginInstance {
  id: string
  schema: PluginUISchema
  config: Record<string, unknown>
  state: 'running' | 'stopped' | 'error'
  enabled: boolean
  stats?: {
    startCount?: number
    uptime?: number
    lastError?: string
  }
  // Extra instance-specific data
  deviceCount?: number
  connected?: boolean
  lastSeen?: number
}

// ============================================================================
// Icon mapping (lucide-react)
// ============================================================================

export const PLUGIN_ICONS: Record<string, string> = {
  // LLM Backends
  ollama: 'Server',
  openai: 'Sparkles',
  anthropic: 'BrainCircuit',
  google: 'Gem',
  xai: 'Zap',

  // Device Adapters
  builtin_mqtt: 'Server',
  external_mqtt: 'Network',
  hass: 'Home',
  modbus: 'Wifi',
  zigbee: 'Radio',

  // Storage
  redb: 'Database',
  memory: 'Cpu',

  // Alert Channels
  email: 'Mail',
  webhook: 'Webhook',
  console: 'Terminal',

  // Integration
  mqtt_connector: 'Network',
}

// ============================================================================
// Default field schemas (reusable)
// ============================================================================

export const COMMON_FIELDS = {
  // Name field
  name: {
    name: 'name',
    type: 'string' as const,
    label: '显示名称',
    description: '用于在界面上显示的名称',
    required: true,
    placeholder: '例如：生产环境',
  } satisfies FieldSchema,

  // Enabled field
  enabled: {
    name: 'enabled',
    type: 'boolean' as const,
    label: '启用',
    description: '是否启用此插件',
    default: true,
  } satisfies FieldSchema,

  // Auto-start field
  autoStart: {
    name: 'auto_start',
    type: 'boolean' as const,
    label: '自动启动',
    description: '系统启动时自动启动此插件',
    default: false,
  } satisfies FieldSchema,

  // API Key field
  apiKey: {
    name: 'api_key',
    type: 'password' as const,
    label: 'API Key',
    description: '用于认证的 API 密钥',
    secret: true,
  } satisfies FieldSchema,

  // Host field
  host: {
    name: 'host',
    type: 'string' as const,
    label: '主机地址',
    description: '服务器主机地址或 IP',
    placeholder: '192.168.1.100',
    required: true,
  } satisfies FieldSchema,

  // Port field
  port: {
    name: 'port',
    type: 'number' as const,
    label: '端口',
    description: '服务器端口',
    default: 1883,
    minimum: 1,
    maximum: 65535,
  } satisfies FieldSchema,

  // URL field
  url: {
    name: 'url',
    type: 'url' as const,
    label: 'URL',
    description: '服务器的完整 URL',
    placeholder: 'https://example.com',
  } satisfies FieldSchema,

  // Username field
  username: {
    name: 'username',
    type: 'string' as const,
    label: '用户名',
    description: '认证用户名',
  } satisfies FieldSchema,

  // Password field
  password: {
    name: 'password',
    type: 'password' as const,
    label: '密码',
    description: '认证密码',
    secret: true,
  } satisfies FieldSchema,
}
