/**
 * SplitPaneBuilder - Split layout for AI-powered builders
 *
 * Left: Form inputs
 * Right Top: Visualization preview
 * Right Bottom: Generated code
 *
 * Layout Structure:
 * ┌─────────────────────────────────────────────────────────────────┐
 * │ Header: Title | Status | Close                                  │
 * ├──────────────────────┬──────────────────────────────────────────┤
 │                      │                                          │
 │  Left Panel (45%)     │  Right Panel (55%)                        │
 │                      │  ┌────────────────────────────────────┐ │
 │  - Form inputs       │  │   Visualization Preview            │ │
 │  - Configuration     │  │   (Flowchart/Rule Diagram)          │ │
 │  - AI intent input   │  │                                    │ │
 │                      │  └────────────────────────────────────┘ │
 │                      │                                          │
 │                      │  ┌────────────────────────────────────┐ │
 │                      │  │   Generated Code                    │ │
 │                      │  │   (Syntax highlighted)               │ │
 │                      │  └────────────────────────────────────┘ │
 │                      │                                          │
 ├──────────────────────┴──────────────────────────────────────────┤
 │ Footer: Validation | Cancel | Save                              │
 └─────────────────────────────────────────────────────────────────┘
 */

import { ReactNode, useState } from 'react'
import { createPortal } from 'react-dom'
import { cn } from '@/lib/utils'
import { Button } from '@/components/ui/button'
import { ScrollArea } from '@/components/ui/scroll-area'
import { Tabs, TabsList, TabsTrigger, TabsContent } from '@/components/ui/tabs'
import { X, ChevronLeft, Eye, Code, Activity } from 'lucide-react'

export interface SplitPaneBuilderProps {
  open: boolean
  onClose: () => void

  // Header
  title: string
  description?: string
  icon?: ReactNode
  headerActions?: ReactNode
  badge?: ReactNode

  // Left panel - Form inputs
  leftPanel: {
    title: string
    content: ReactNode
  }

  // Right panel - Preview & Code
  rightPanel: {
    visualization?: ReactNode
    code?: string
    codeLanguage?: string
    loading?: boolean
    error?: string
  }

  // Footer
  isValid: boolean
  isSaving: boolean
  saveLabel?: string
  onSave: () => void | Promise<void>
  validationMessage?: string
  footerLeftActions?: ReactNode

  // Optional AI generation
  onGenerate?: () => void | Promise<void>
  generating?: boolean
  generateLabel?: string
}

export function SplitPaneBuilder({
  open,
  onClose,
  title,
  description,
  icon,
  headerActions,
  badge,
  leftPanel,
  rightPanel,
  isValid,
  isSaving,
  saveLabel,
  onSave,
  validationMessage,
  footerLeftActions,
  onGenerate,
  generating,
  generateLabel,
}: SplitPaneBuilderProps) {
  const [rightTab, setRightTab] = useState<'visualization' | 'code'>('visualization')

  if (!open) return null

  const content = (
    <div className="fixed inset-0 z-[100] bg-background flex flex-col">
      {/* Header */}
      <div className="flex items-center justify-between px-6 py-4 border-b bg-background flex-shrink-0">
        <div className="flex items-center gap-4 flex-1 min-w-0">
          {icon && <div className="flex-shrink-0 text-muted-foreground">{icon}</div>}
          <div className="min-w-0 flex-1">
            <div className="flex items-center gap-2">
              <h1 className="text-lg font-semibold truncate">{title}</h1>
              {badge}
            </div>
            {description && (
              <p className="text-sm text-muted-foreground truncate">{description}</p>
            )}
          </div>
        </div>

        <div className="flex items-center gap-2">
          {headerActions}
          <Button variant="ghost" size="icon" onClick={onClose} className="flex-shrink-0">
            <X className="h-5 w-5" />
          </Button>
        </div>
      </div>

      {/* Main Content - Split Panes */}
      <div className="flex flex-1 min-h-0 overflow-hidden">
        {/* Left Panel - Form (45%) */}
        <div className="w-[45%] min-w-[400px] max-w-[600px] border-r bg-muted/10 flex flex-col">
          <div className="px-4 py-3 border-b bg-muted/30 font-medium text-sm flex items-center gap-2">
            <span>{leftPanel.title}</span>
          </div>
          <ScrollArea className="flex-1">
            <div className="p-4">{leftPanel.content}</div>
          </ScrollArea>
        </div>

        {/* Right Panel - Preview (55%) */}
        <div className="flex-1 flex flex-col bg-background">
          {/* Right panel header with tabs */}
          <div className="flex items-center justify-between px-4 py-2 border-b bg-muted/30 flex-shrink-0">
            <Tabs value={rightTab} onValueChange={(v) => setRightTab(v as 'visualization' | 'code')} className="w-auto">
              <TabsList className="h-8">
                <TabsTrigger value="visualization" className="gap-1.5 h-8 px-3">
                  <Eye className="h-3.5 w-3.5" />
                  <span>可视化</span>
                </TabsTrigger>
                <TabsTrigger value="code" className="gap-1.5 h-8 px-3">
                  <Code className="h-3.5 w-3.5" />
                  <span>代码</span>
                </TabsTrigger>
              </TabsList>
            </Tabs>

            {onGenerate && (
              <Button
                size="sm"
                onClick={onGenerate}
                disabled={generating}
                variant="default"
                className="h-8"
              >
                {generating ? (
                  <>
                    <Activity className="h-3.5 w-3.5 mr-1.5 animate-spin" />
                    生成中...
                  </>
                ) : (
                  generateLabel || 'AI 生成'
                )}
              </Button>
            )}
          </div>

          {/* Right panel content */}
          <div className="flex-1 min-h-0">
            {rightPanel.loading ? (
              <div className="h-full flex items-center justify-center">
                <Activity className="h-8 w-8 animate-spin text-muted-foreground" />
              </div>
            ) : rightPanel.error ? (
              <div className="h-full flex items-center justify-center p-6">
                <div className="text-center max-w-md">
                  <p className="text-destructive">{rightPanel.error}</p>
                </div>
              </div>
            ) : (
              <Tabs value={rightTab} className="h-full flex flex-col">
                {/* Visualization Tab */}
                <TabsContent value="visualization" className="flex-1 m-0 p-0 overflow-auto">
                  {rightPanel.visualization ?? (
                    <div className="h-full flex items-center justify-center text-muted-foreground p-6">
                      <p className="text-sm">暂无可视化预览，请先配置规则或点击生成</p>
                    </div>
                  )}
                </TabsContent>

                {/* Code Tab */}
                <TabsContent value="code" className="flex-1 m-0 p-0 overflow-auto">
                  {rightPanel.code ? (
                    <pre className="h-full p-4 text-sm overflow-auto">
                      <code>{rightPanel.code}</code>
                    </pre>
                  ) : (
                    <div className="h-full flex items-center justify-center text-muted-foreground p-6">
                      <p className="text-sm">暂无代码，请先配置规则或点击 AI 生成</p>
                    </div>
                  )}
                </TabsContent>
              </Tabs>
            )}
          </div>
        </div>
      </div>

      {/* Footer */}
      <div className="flex items-center justify-between px-6 py-4 border-t bg-background flex-shrink-0">
        <div className="flex items-center gap-4 flex-1">
          {footerLeftActions}

          {/* Validation Status */}
          <div className="flex items-center gap-2 text-sm">
            {validationMessage && (
              <span className={cn('flex items-center gap-1', isValid ? 'text-muted-foreground' : 'text-destructive')}>
                {validationMessage}
              </span>
            )}
          </div>
        </div>

        {/* Action Buttons */}
        <div className="flex items-center gap-2">
          <Button variant="outline" onClick={onClose} disabled={isSaving}>
            取消
          </Button>
          <Button onClick={onSave} disabled={!isValid || isSaving}>
            {isSaving ? (
              <>
                <Activity className="h-4 w-4 mr-2 animate-spin" />
                保存中...
              </>
            ) : (
              saveLabel || '保存'
            )}
          </Button>
        </div>
      </div>
    </div>
  )

  return createPortal(content, document.body)
}

/**
 * Collapsible section for the left panel
 */
export interface FormSectionProps {
  title: string
  description?: string
  children: ReactNode
  defaultExpanded?: boolean
  className?: string
}

export function FormSection({ title, description, children, defaultExpanded = true, className }: FormSectionProps) {
  const [expanded, setExpanded] = useState(defaultExpanded)

  return (
    <div className={cn('border rounded-lg overflow-hidden mb-4', className)}>
      <button
        onClick={() => setExpanded(!expanded)}
        className="w-full flex items-center justify-between px-4 py-3 bg-muted/30 hover:bg-muted/40 transition-colors"
      >
        <div>
          <h3 className="font-medium text-sm">{title}</h3>
          {description && <p className="text-xs text-muted-foreground mt-0.5">{description}</p>}
        </div>
        {expanded ? (
          <ChevronLeft className="h-4 w-4 text-muted-foreground rotate-[-90deg]" />
        ) : (
          <ChevronLeft className="h-4 w-4 text-muted-foreground" />
        )}
      </button>
      {expanded && <div className="p-4 space-y-4">{children}</div>}
    </div>
  )
}

/**
 * Visualization placeholder when no visualization is available
 */
export function VisualizationPlaceholder({
  icon,
  title,
  description,
  action,
}: {
  icon?: ReactNode
  title: string
  description?: string
  action?: { label: string; onClick: () => void }
}) {
  return (
    <div className="h-full flex flex-col items-center justify-center p-8 text-center">
      {icon ?? <Code className="h-12 w-12 text-muted-foreground/40 mb-4" />}
      <h3 className="font-medium text-muted-foreground mb-2">{title}</h3>
      {description && <p className="text-sm text-muted-foreground/60 mb-4 max-w-sm">{description}</p>}
      {action && (
        <Button size="sm" variant="outline" onClick={action.onClick}>
          {action.label}
        </Button>
      )}
    </div>
  )
}
