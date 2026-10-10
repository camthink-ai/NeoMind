import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import {
  Camera,
  Bot,
  User,
  AlertCircle,
  Loader2,
  Clock,
  Cpu,
  Database,
} from 'lucide-react'
import { Dialog, DialogContent } from '@/components/ui/dialog'
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
          <div className="w-6 h-6 rounded-md bg-info-light flex items-center justify-center shrink-0 mt-0.5">
            <Camera className="h-4 w-4 text-info" />
          </div>
          <div className="flex-1 min-w-0">
            <div className="flex justify-between items-center">
              <span className={cn(textNano, "text-muted-foreground truncate")}>
                {message.dataSource || t('aiAnalyst.dataSourceFallback')}
              </span>
              <span className={cn(textNano, "text-muted-foreground")}>{formatTime(message.timestamp)}</span>
            </div>
            <div
              className="mt-1 rounded-lg overflow-hidden border border-border bg-muted-30 cursor-pointer max-w-full"
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
          <div className="w-6 h-6 rounded-md bg-warning-light flex items-center justify-center shrink-0 mt-0.5">
            <Database className="h-4 w-4 text-warning" />
          </div>
          <div className="flex-1 min-w-0">
            <div className="flex justify-between items-center">
              <span className={cn(textNano, "text-muted-foreground truncate")}>
                {message.dataSource || t('aiAnalyst.dataSourceFallback')}
              </span>
              <span className={cn(textNano, "text-muted-foreground")}>{formatTime(message.timestamp)}</span>
            </div>
            <div className="mt-1 rounded-lg px-3 py-1.5 bg-warning-light border border-warning w-full overflow-hidden">
              {/* Inline images */}
              {hasImages && (
                <div className="flex gap-1 mb-1 flex-wrap">
                  {message.images!.map((img, i) => (
                    <div
                      key={i}
                      className="rounded overflow-hidden border border-border cursor-pointer max-w-[120px]"
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
                <p key={i} className="text-xs text-foreground font-mono leading-relaxed break-all">
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
        <div className="w-6 h-6 rounded-md bg-accent-purple-light flex items-center justify-center shrink-0 mt-0.5">
          <Bot className="h-4 w-4 text-accent-purple" />
        </div>
        <div className="flex-1 min-w-0">
          <div className="flex justify-between items-center">
            <span className={cn(textNano, "text-accent-purple flex items-center gap-1")}>
              <Cpu className="h-2.5 w-2.5" />
              {message.modelName || t('aiAnalyst.aiFallback')}
            </span>
            {isStreaming ? (
              <span className={cn(textNano, "text-accent-purple flex items-center gap-1")}>
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
          <div className="mt-1 rounded-lg px-3 py-2 bg-accent-purple-light border border-accent-purple-light w-full">
            {isStreaming && !displayContent ? (
              <div className="flex items-center gap-1.5">
                <Loader2 className="h-4 w-4 animate-spin text-accent-purple" />
                <span className="text-xs text-muted-foreground">{t('aiAnalyst.waitingForResponse')}</span>
              </div>
            ) : (
              <p className="text-xs text-foreground leading-relaxed whitespace-pre-wrap">
                {displayContent}
              </p>
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
          <div className="flex justify-between items-center w-full">
            <span className={cn(textNano, "text-muted-foreground")}>{formatTime(message.timestamp)}</span>
            <span className={cn(textNano, "text-accent-purple flex items-center gap-1 opacity-70")}>
              <User className="h-2.5 w-2.5" />
              {t('aiAnalyst.user')}
            </span>
          </div>
          <div className="mt-1 rounded-lg px-3 py-2 bg-accent-purple-light border border-accent-purple-light w-full">
            <p className="text-xs text-foreground leading-relaxed">{message.content}</p>
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

  return null
}
