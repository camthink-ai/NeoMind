// Stub file for backward compatibility
// TODO: Refactor components using this to use proper configuration dialogs

import { Dialog, DialogContent, DialogHeader, DialogTitle } from "@/components/ui/dialog"
import type { LucideIcon } from "lucide-react"
import { ReactNode } from "react"

export interface PluginInstance {
  id: string
  name: string
  type?: string  // Made optional for backward compatibility
  enabled: boolean
  config: Record<string, any>
  // Additional properties used by components
  plugin_type?: string
  running?: boolean
  status?: Record<string, any>
  isBuiltin?: boolean
  capabilities?: string[] | any  // Allow any type for capabilities
  [key: string]: any  // Allow any additional properties
}

export interface UnifiedPluginType {
  type: string
  name: string
  description: string
  category?: string  // Made optional
  config_schema?: any
  icon?: LucideIcon | string | ReactNode  // Icon can be various types
  // Additional properties used by components
  id?: string
  can_add_multiple?: boolean
  default_endpoint?: string
  requires_api_key?: boolean
  supports_streaming?: boolean
  default_model?: string
  capabilities?: string[] | any
  color?: string
  builtin?: boolean
  field_order?: string[]
  [key: string]: any  // Allow any additional properties
}

export interface PluginConfigSchemaProperty {
  type: string
  description?: string
  enum?: string[]
  items?: any
  properties?: any
  required?: string[]
  default?: any
  minimum?: number
  maximum?: number
  format?: string
  secret?: boolean
  [key: string]: any
}

export interface PluginConfigSchema {
  type: 'object'
  properties: Record<string, PluginConfigSchemaProperty>
  required?: string[]
  ui_hints?: {
    field_order?: string[]
    [key: string]: any
  }
  [key: string]: any
}

export interface UniversalPluginConfigDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  plugin?: any
  pluginType?: UnifiedPluginType
  instances?: PluginInstance[]
  editingInstance?: PluginInstance | null
  config?: any
  onConfigChange?: (config: any) => void
  onSave?: () => void
  onSaveConfig?: (instanceId: string, config: any) => Promise<void>
  onCreateInstance?: (name: string, config: any) => Promise<void>
  onEditInstance?: (instanceId: string, name: string, config: any) => Promise<void>
  onDeleteInstance?: (instanceId: string) => Promise<void>
  onTestConnection?: (config: any) => Promise<void>
  setTestResults?: (results: any) => void
  testResults?: any
  isTesting?: boolean
  isSaving?: boolean
  canAddMultiple?: boolean
  [key: string]: any
}

export function UniversalPluginConfigDialog({ open, onOpenChange }: UniversalPluginConfigDialogProps) {
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Configuration Dialog</DialogTitle>
        </DialogHeader>
        <div className="text-sm text-muted-foreground">
          This is a stub component. The actual configuration dialog was removed during cleanup.
          Please use the Extension system instead.
        </div>
      </DialogContent>
    </Dialog>
  )
}
