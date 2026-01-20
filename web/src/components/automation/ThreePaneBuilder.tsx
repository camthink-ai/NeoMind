/**
 * ThreePaneBuilder - Three-pane layout for AI-powered builders
 *
 * Left: AI Chat (supports multiple conversations, auto-clear on close)
 * Center: Visualization preview
 * Right: Form inputs
 *
 * Layout Structure:
 * ┌────────────────────────────────────────────────────────────────────────┐
 * │ Header: Title | Status | Close                                         │
 * ├──────────┬───────────────────────┬─────────────────────────────────────┤
 * │          │                       │                                     │
 * │ Left     │   Center              │   Right                             │
 * │ (25%)    │   (40%)               │   (35%)                             │
 * │          │                       │                                     │
 * │ AI Chat  │   Visualization       │   Form Inputs                       │
 * │ - Send   │   - Flowchart         │   - Basic Info                      │
 * │ - Clear  │   - Diagram           │   - Configuration                   │
 * │ - History│   - Preview           │   - Settings                        │
 * │          │                       │                                     │
 * ├──────────┴───────────────────────┴─────────────────────────────────────┤
 * │ Footer: Validation | Cancel | Save                                        │
 * └────────────────────────────────────────────────────────────────────────┘
 */

import { ReactNode, useState, useRef, useEffect } from 'react'
import { createPortal } from 'react-dom'
import { cn } from '@/lib/utils'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { ScrollArea } from '@/components/ui/scroll-area'
import { X, Send, Trash2, Sparkles, Activity, ChevronLeft } from 'lucide-react'

export interface ChatMessage {
  id: string
  role: 'user' | 'assistant'
  content: string
  timestamp: number
}

export interface ThreePaneBuilderProps {
  open: boolean
  onClose: () => void

  // Header
  title: string
  description?: string
  icon?: ReactNode
  headerActions?: ReactNode
  badge?: ReactNode

  // Left panel - AI Chat
  chatPanel?: {
    placeholder?: string
    onSendMessage?: (message: string) => void
    generating?: boolean
    messages?: ChatMessage[]
    clearOnClose?: boolean
  }

  // Center panel - Visualization
  centerPanel: {
    title?: string
    content: ReactNode
  }

  // Right panel - Form inputs
  rightPanel: {
    title: string
    content: ReactNode
  }

  // Footer
  isValid: boolean
  isSaving: boolean
  saveLabel?: string
  onSave: () => void | Promise<void>
  validationMessage?: string
  footerLeftActions?: ReactNode
}

export function ThreePaneBuilder({
  open,
  onClose,
  title,
  description,
  icon,
  headerActions,
  badge,
  chatPanel,
  centerPanel,
  rightPanel,
  isValid,
  isSaving,
  saveLabel,
  onSave,
  validationMessage,
  footerLeftActions,
}: ThreePaneBuilderProps) {
  // Chat state (managed internally for auto-clear)
  const [chatInput, setChatInput] = useState('')
  const [chatMessages, setChatMessages] = useState<ChatMessage[]>(chatPanel?.messages || [])
  const [isSending, setIsSending] = useState(false)
  const chatEndRef = useRef<HTMLDivElement>(null)

  // Auto-clear chat on close
  useEffect(() => {
    if (!open && chatPanel?.clearOnClose) {
      setChatMessages([])
      setChatInput('')
    }
  }, [open, chatPanel?.clearOnClose])

  // Auto-scroll to bottom of chat
  useEffect(() => {
    chatEndRef.current?.scrollIntoView({ behavior: 'smooth' })
  }, [chatMessages])

  const handleSendChat = async () => {
    const message = chatInput.trim()
    if (!message || isSending) return

    // Add user message
    const userMessage: ChatMessage = {
      id: `msg-${Date.now()}`,
      role: 'user',
      content: message,
      timestamp: Date.now(),
    }
    setChatMessages(prev => [...prev, userMessage])
    setChatInput('')
    setIsSending(true)

    // Call external handler if provided
    if (chatPanel?.onSendMessage) {
      await chatPanel.onSendMessage(message)
      // Simulate AI response (in real implementation, this would come from the AI)
      setTimeout(() => {
        const aiMessage: ChatMessage = {
          id: `msg-${Date.now()}`,
          role: 'assistant',
          content: '已收到您的指令，正在生成配置...',
          timestamp: Date.now(),
        }
        setChatMessages(prev => [...prev, aiMessage])
        setIsSending(false)
      }, 500)
    } else {
      setIsSending(false)
    }
  }

  const handleClearChat = () => {
    setChatMessages([])
  }

  const handleKeyPress = (e: React.KeyboardEvent) => {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault()
      handleSendChat()
    }
  }

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

      {/* Main Content - Three Panes */}
      <div className="flex flex-1 min-h-0 overflow-hidden">
        {/* Left Panel - AI Chat (25%) */}
        <div className="w-1/4 min-w-[250px] border-r bg-muted/5 flex flex-col">
          <div className="px-4 py-3 border-b bg-muted/30 font-medium text-sm flex items-center justify-between">
            <div className="flex items-center gap-2">
              <Sparkles className="h-4 w-4 text-purple-500" />
              <span>AI 助手</span>
            </div>
            {chatMessages.length > 0 && (
              <Button
                variant="ghost"
                size="icon"
                className="h-6 w-6"
                onClick={handleClearChat}
              >
                <Trash2 className="h-3 w-3" />
              </Button>
            )}
          </div>

          {/* Chat messages */}
          <ScrollArea className="flex-1">
            <div className="p-4 space-y-4">
              {chatMessages.length === 0 ? (
                <div className="text-center py-8 text-muted-foreground">
                  <Sparkles className="h-10 w-10 mx-auto mb-3 opacity-30" />
                  <p className="text-sm font-medium">开始对话</p>
                  <p className="text-xs mt-1">描述您想要的配置，AI 将自动生成</p>
                </div>
              ) : (
                <>
                  {chatMessages.map((msg) => (
                    <div
                      key={msg.id}
                      className={cn(
                        "flex flex-col gap-1 max-w-[85%]",
                        msg.role === 'user' ? 'ml-auto items-end' : 'mr-auto items-start'
                      )}
                    >
                      <div
                        className={cn(
                          "px-3 py-2 rounded-lg text-sm",
                          msg.role === 'user'
                            ? 'bg-purple-500 text-white rounded-br-sm'
                            : 'bg-muted text-foreground rounded-bl-sm'
                        )}
                      >
                        {msg.content}
                      </div>
                      <span className="text-xs text-muted-foreground px-1">
                        {new Date(msg.timestamp).toLocaleTimeString('zh-CN', {
                          hour: '2-digit',
                          minute: '2-digit'
                        })}
                      </span>
                    </div>
                  ))}
                  {(isSending || chatPanel?.generating) && (
                    <div className="flex flex-col gap-1 max-w-[85%] mr-auto items-start">
                      <div className="px-3 py-2 rounded-lg text-sm bg-muted text-foreground rounded-bl-sm">
                        <div className="flex items-center gap-2">
                          <Activity className="h-3 w-3 animate-spin" />
                          <span>思考中...</span>
                        </div>
                      </div>
                    </div>
                  )}
                  <div ref={chatEndRef} />
                </>
              )}
            </div>
          </ScrollArea>

          {/* Chat input */}
          <div className="p-3 border-t bg-background">
            <div className="flex gap-2">
              <Input
                value={chatInput}
                onChange={(e) => setChatInput(e.target.value)}
                onKeyPress={handleKeyPress}
                placeholder={chatPanel?.placeholder || '描述您的需求...'}
                className="flex-1 h-9"
                disabled={isSending || chatPanel?.generating}
              />
              <Button
                size="icon"
                onClick={handleSendChat}
                disabled={!chatInput.trim() || isSending || chatPanel?.generating}
                className="h-9 w-9 shrink-0"
              >
                {isSending || chatPanel?.generating ? (
                  <Activity className="h-4 w-4 animate-spin" />
                ) : (
                  <Send className="h-4 w-4" />
                )}
              </Button>
            </div>
          </div>
        </div>

        {/* Center Panel - Visualization (40%) */}
        <div className="w-2/5 min-w-[300px] border-r bg-background flex flex-col">
          {centerPanel.title && (
            <div className="px-4 py-3 border-b bg-muted/30 font-medium text-sm">
              {centerPanel.title}
            </div>
          )}
          <div className="flex-1 overflow-auto">
            {centerPanel.content}
          </div>
        </div>

        {/* Right Panel - Form (35%) */}
        <div className="flex-1 min-w-[280px] bg-muted/10 flex flex-col">
          <div className="px-4 py-3 border-b bg-muted/30 font-medium text-sm">
            {rightPanel.title}
          </div>
          <ScrollArea className="flex-1">
            <div className="p-4">{rightPanel.content}</div>
          </ScrollArea>
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
 * Collapsible section for the form panel
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
