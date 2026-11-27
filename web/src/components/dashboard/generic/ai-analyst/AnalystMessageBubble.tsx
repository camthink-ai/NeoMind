import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import {
  Camera,
  Sparkles,
  User,
  AlertCircle,
  AlertTriangle,
  Loader2,
  Clock,
  Cpu,
  Database,
} from 'lucide-react'
import { Dialog, DialogContent } from '@/components/ui/dialog'
import { ConclusionContent } from '@/pages/agents-components/ConclusionContent'
import type { AnalystMessage } from './types'
import { cn } from "@/lib/utils"
import { formatElapsedMs } from "@/lib/utils/format"
import { textNano } from "@/design-system/tokens/typography"

interface AnalystMessageBubbleProps {
  message: AnalystMessage
  streamingContent?: string
}

export function AnalystMessageBubble({ message, streamingContent }: AnalystMessageBubbleProps) {
  const { t } = useTranslation('dashboardComponents')
  const [fullscreenImage, setFullscreenImage] = useState<string | null>(null)
  // Streaming indicator is controlled by streamingMsgId (via AnalystTimeline),
  // NOT by message.isStreaming which is never cleared from the messages array.
  // streamingContent is undefined when this message is not the active streaming target.
  const isStreaming = streamingContent !== undefined
  const displayContent = streamingContent || message.content

  const formatTime = (ts: number) =>
    new Date(ts).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', second: '2-digit' })

  const hasImages = message.images && message.images.length > 0

  // Image-only entry (legacy standalone image messages)
  if (message.type === 'image') {
    return (
      <>
        <div className="flex items-start gap-2">
          <div className="w-6 h-6 rounded-lg border border-border bg-muted-30 text-foreground flex items-center justify-center shrink-0 mt-0.5">
            <Camera className="h-3.5 w-3.5" strokeWidth={1.75} />
          </div>
          <div className="flex-1 min-w-0">
            <div className="flex justify-between items-center tabular-nums">
              <span className={cn(textNano, "text-muted-foreground truncate")}>
                {message.dataSource || t('aiAnalyst.dataSourceFallback')}
              </span>
              <span className={cn(textNano, "text-muted-foreground")}>{formatTime(message.timestamp)}</span>
            </div>
            <div
              className="mt-1 rounded-xl overflow-hidden border border-border bg-muted-30 cursor-pointer max-w-full"
              onClick={() => setFullscreenImage(message.content)}
            >
              <img
                src={message.content}
                alt="Data source image"
                className="w-full h-auto max-h-[120px] object-cover"
              />
            </div>
          </div>
        </div>
        {fullscreenImage && (
          <Dialog open={!!fullscreenImage} onOpenChange={() => setFullscreenImage(null)}>
            <DialogContent className="max-w-4xl p-2 z-[110]">
              <img
                src={fullscreenImage}
                alt="Fullscreen"
                className="w-full h-auto max-h-[80vh] object-contain rounded-lg"
              />
            </DialogContent>
          </Dialog>
        )}
      </>
    )
  }

  // Data entry — unified bubble for images + text
  if (message.type === 'data') {
    const lines = message.content ? message.content.split('\n').filter(Boolean) : []
    return (
      <>
        <div className="flex items-start gap-2">
          <div className="w-6 h-6 rounded-lg border border-border bg-muted-30 text-foreground flex items-center justify-center shrink-0 mt-0.5">
            <Database className="h-3.5 w-3.5" strokeWidth={1.75} />
          </div>
          <div className="flex-1 min-w-0">
            <div className="flex justify-between items-center tabular-nums">
              <span className={cn(textNano, "text-muted-foreground truncate")}>
                {message.dataSource || t('aiAnalyst.dataSourceFallback')}
              </span>
              <span className={cn(textNano, "text-muted-foreground")}>{formatTime(message.timestamp)}</span>
            </div>
            <div className="mt-1 rounded-xl px-3 py-2 bg-muted-20 border border-glass-border w-full overflow-hidden">
              {/* Inline images */}
              {hasImages && (
                <div className="flex gap-1.5 mb-1.5 flex-wrap">
                  {message.images!.map((img, i) => (
                    <div
                      key={i}
                      className="rounded-md overflow-hidden border border-border cursor-pointer max-w-[120px]"
                      onClick={() => setFullscreenImage(img)}
                    >
                      <img
                        src={img}
                        alt={`Image ${i + 1}`}
                        className="w-full h-auto max-h-[80px] object-cover"
                      />
                    </div>
                  ))}
                </div>
              )}
              {/* Text data lines */}
              {lines.map((line, i) => (
                <p key={i} className="text-xs text-foreground font-mono leading-relaxed break-all tabular-nums">
                  {line.includes(':')
                    ? <>
                        <span className="text-muted-foreground">{line.split(':').slice(0, -1).join(':')}:</span>
                        <span>{line.split(':').slice(-1)[0]}</span>
                      </>
                    : line
                  }
                </p>
              ))}
            </div>
          </div>
        </div>
        {fullscreenImage && (
          <Dialog open={!!fullscreenImage} onOpenChange={() => setFullscreenImage(null)}>
            <DialogContent className="max-w-4xl p-2 z-[110]">
              <img
                src={fullscreenImage}
                alt="Fullscreen"
                className="w-full h-auto max-h-[80vh] object-contain rounded-lg"
              />
            </DialogContent>
          </Dialog>
        )}
      </>
    )
  }

  // AI response
  if (message.type === 'ai') {
    return (
      <div className="flex items-start gap-2">
        <div className="w-6 h-6 rounded-lg border border-border bg-muted-30 text-foreground flex items-center justify-center shrink-0 mt-0.5">
          <Sparkles className="h-3.5 w-3.5" strokeWidth={1.75} />
        </div>
        <div className="flex-1 min-w-0">
          <div className="flex justify-between items-center tabular-nums">
            <span className={cn(textNano, "text-muted-foreground flex items-center gap-1")}>
              <Cpu className="h-2.5 w-2.5" />
              {message.modelName || t('aiAnalyst.aiFallback')}
            </span>
            {isStreaming ? (
              <span className={cn(textNano, "text-info flex items-center gap-1")}>
                <Loader2 className="h-2.5 w-2.5 animate-spin" />
                {t('aiAnalyst.analyzing')}...
              </span>
            ) : (
              <span className={cn(textNano, "text-muted-foreground flex items-center gap-1")}>
                <Clock className="h-2.5 w-2.5" />
                {message.duration ? formatElapsedMs(message.duration) : formatTime(message.timestamp)}
              </span>
            )}
          </div>
          <div className="mt-1 rounded-xl px-3 py-2 bg-card border border-border w-full">
            {isStreaming && !displayContent ? (
              <div className="flex items-center gap-1.5">
                <Loader2 className="h-3.5 w-3.5 animate-spin text-muted-foreground" />
                <span className="text-xs text-muted-foreground">{t('aiAnalyst.waitingForResponse')}</span>
              </div>
            ) : (
              <div className="text-xs text-foreground leading-relaxed">
                <ConclusionContent content={displayContent} />
              </div>
            )}
          </div>
        </div>
      </div>
    )
  }

  // User follow-up
  if (message.type === 'user') {
    return (
      <div className="flex items-start gap-2 justify-end">
        <div className="flex-1 min-w-0 flex flex-col items-end">
          <div className="flex justify-between items-center w-full tabular-nums">
            <span className={cn(textNano, "text-muted-foreground")}>{formatTime(message.timestamp)}</span>
            <span className={cn(textNano, "text-muted-foreground flex items-center gap-1")}>
              <User className="h-2.5 w-2.5" />
              {t('aiAnalyst.user')}
            </span>
          </div>
          <div className="mt-1 rounded-xl px-3 py-2 bg-muted-30 border border-glass-border w-full">
            <p className="text-xs text-foreground leading-relaxed break-words">{message.content}</p>
          </div>
        </div>
      </div>
    )
  }

  // Error
  if (message.type === 'error') {
    return (
      <div className="flex items-start gap-2">
        <div className="w-6 h-6 rounded-md bg-error-light flex items-center justify-center shrink-0 mt-0.5">
          <AlertCircle className="h-4 w-4 text-error" />
        </div>
        <div className="flex-1 min-w-0">
          <div className="rounded-lg px-3 py-2 bg-error-light border border-error w-full">
            <p className="text-xs text-error">{message.content}</p>
          </div>
        </div>
      </div>
    )
  }

  // Partial / undetermined (vision) — not success, not hard failure
  if (message.type === 'warning') {
    return (
      <div className="flex items-start gap-2">
        <div className="w-6 h-6 rounded-md bg-warning-light flex items-center justify-center shrink-0 mt-0.5">
          <AlertTriangle className="h-4 w-4 text-warning" />
        </div>
        <div className="flex-1 min-w-0">
          <div className="rounded-lg px-3 py-2 bg-warning-light border border-warning w-full">
            <p className="text-xs text-warning">{message.content}</p>
          </div>
        </div>
      </div>
    )
  }

  return null
}
