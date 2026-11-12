import { Card, CardContent } from "@/components/ui/card"
import { Button } from "@/components/ui/button"
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"
import {
  MoreHorizontal,
  Trash,
  Code2,
  Terminal,
  Database,
  RefreshCw,
  EyeOff,
  Eye,
  Plug,
  CloudSun,
  Cpu,
  Factory,
  Wifi,
  Camera,
  
  Brain,
  MonitorPlay,
  ScanText,
  UserCheck,
  type LucideIcon,
} from "lucide-react"
import { useTranslation } from "react-i18next"
import { cn } from "@/lib/utils"
import { textNano, textMini } from "@/design-system/tokens/typography"
import type { Extension } from "@/types"

/// Pick a scannable category icon from the extension id/name keywords.
/// Backend sends no category/icon field, so heuristics keep the grid
/// distinguishable. Last-match wins; falls back to Code2.
function categoryIcon(id: string, name: string): LucideIcon {
  const haystack = `${id} ${name}`.toLowerCase()
  const pairs: [RegExp, LucideIcon][] = [
    [/vision|yolo|object.?detect|image|analy|ocr|face/, Eye],
    [/bridge|modbus|opc.?ua|bacnet|mqtt|lorawan|ha\b|home.?assist/, Plug],
    [/weather|forecast/, CloudSun],
    [/stream|player|rtsp|video|media/, MonitorPlay],
    [/onvif|camera|ip.?cam/, Camera],
    [/transcri|speech|voice|asr|tts/, Brain],
    [/scan|text|ocr/, ScanText],
    [/ident|recogn/, UserCheck],
    [/demo|core|runtime/, Cpu],
    [/industrial|scada|plc/, Factory],
    [/wifi|sensor|iot/, Wifi],
    [/push|export|telemetry|notif/, Database],
    [/tool|command|cli|skill/, Terminal],
  ]
  for (const [re, icon] of pairs) {
    if (re.test(haystack)) return icon
  }
  return Code2
}

interface ExtensionCardProps {
  extension: Extension
  onUninstall?: () => void
  onDetails?: () => void
  onReload?: () => void
}

export function ExtensionCard({
  extension,
  onUninstall,
  onDetails,
  onReload,
}: ExtensionCardProps) {
  const { t } = useTranslation(["extensions", "common"])

  // Extension type has no category/icon field, so derive a scannable icon
  // from the id/name keywords (keeps a 20-extension grid distinguishable at
  // a glance instead of every card showing the same Code2 glyph).
  const CategoryIcon = categoryIcon(extension.id, extension.name)

  // toolsEnabled = master toggle for exposing commands to the agent.
  // The card only shows state; toggling lives in the details dialog to keep
  // the card height stable across on/off (no extra line, no alignment drift).
  const toolsEnabled = extension.enabled !== false

  const hasError = extension.state === "Error"
  // Crashed = stopped by crash-loop, not on purpose — error-tinted, with the
  // crash reason on hover (backend: consecutive_crashes/last_crash_reason).
  // Only when the backend says Crashed (stopped by crash-loop). A RUNNING
  // extension with crash history must not show as Crashed — the old OR on
  // consecutive_crashes tinted healthy restarted extensions red.
  const hasCrashed = extension.state === "Crashed"
  const hasWarning = extension.state === "Warning"
  const isFailed = extension.state === "Failed" || extension.state === "Stopped"

  const displayState = hasError
    ? t('error', { defaultValue: 'Error' })
    : hasCrashed
      ? t('crashed', { defaultValue: 'Crashed' })
      : hasWarning
        ? t('warning', { defaultValue: 'Warning' })
        : isFailed
          ? extension.state
          : t('active', { defaultValue: 'Active' })

  const crashTitle = hasCrashed
    ? extension.last_crash_reason
      ? `${t('crashed', { defaultValue: 'Crashed' })} ×${extension.consecutive_crashes}: ${extension.last_crash_reason}`
      : t('crashedTooltip', { defaultValue: 'Stopped after repeated crashes' })
    : undefined

  return (
    <Card
      role="button"
      tabIndex={0}
      onClick={() => onDetails?.()}
      onKeyDown={(e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); onDetails?.() } }}
      className={cn(
        "group h-full overflow-hidden flex flex-col cursor-pointer rounded-2xl border bg-card shadow-none active:scale-[0.99] md:rounded-xl md:shadow-sm",
        "md:transition-all md:duration-fast md:ease-out md:hover:border-[var(--border-hover)] md:hover:shadow-md",
        "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2",
      )}
    >
      <CardContent className="flex flex-1 flex-col px-4 py-3.5 md:p-4">
        {/* Header: status-tinted icon + name + state + actions.
            On mobile the icon tile is hidden, so the state pill next to the
            name is the only status indicator — keep both signals. */}
        <div className="flex min-w-0 items-start justify-between gap-2 md:mb-3">
          <div className="flex min-w-0 flex-1 items-center gap-3">
            {/* Icon with status-tinted background (mirrors AgentCard) */}
            <div className={cn(
              "relative hidden w-11 h-11 rounded-xl items-center justify-center shrink-0 transition-colors md:flex",
              hasError || hasCrashed ? "bg-error-light text-error" :
              hasWarning ? "bg-warning-light text-warning" :
              isFailed ? "bg-muted text-muted-foreground" :
              "bg-success-light text-success",
            )}>
            <CategoryIcon className="h-5 w-5" />
              {/* Status indicator dot */}
              <div className={cn(
                "absolute -top-0.5 -right-0.5 w-3 h-3 rounded-full border-2 border-background",
                hasError || hasCrashed ? "bg-error" : hasWarning ? "bg-warning" : isFailed ? "bg-muted-foreground" : "bg-success"
              )} />
            </div>
            <div className="min-w-0">
              <div className="flex items-center gap-2">
                <h3 className="font-semibold text-sm truncate" title={extension.name}>{extension.name}</h3>
                <span className={cn(
                  textNano, "shrink-0 rounded-full px-1.5 py-0.5",
                  hasError || hasCrashed ? "bg-error-light text-error" :
                  hasWarning ? "bg-warning-light text-warning" :
                  isFailed ? "bg-muted-30 text-muted-foreground" :
                  "bg-success-light text-success"
                )} title={crashTitle}>
                  {displayState}
                </span>
              </div>
              <div className="flex items-center gap-2 mt-0.5">
                <span className={cn(textMini, "text-muted-foreground font-mono truncate max-w-[160px]")} title={extension.id}>{extension.id}</span>
                {extension.version && (
                  <span className={cn(textNano, "text-muted-foreground shrink-0")}>v{extension.version}</span>
                )}
              </div>
            </div>
          </div>
          <DropdownMenu>
            <DropdownMenuTrigger asChild onClick={(e) => e.stopPropagation()}>
              <Button variant="ghost" size="icon-sm" className="shrink-0" aria-label={t('card.moreActions', { defaultValue: 'More actions' })}>
                <MoreHorizontal className="h-4 w-4" />
              </Button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end" className="w-40" onClick={(e) => e.stopPropagation()}>
              <DropdownMenuItem onClick={() => onReload?.()}>
                <RefreshCw className="mr-2 h-4 w-4" />
                {t('card.reload', { defaultValue: 'Reload' })}
              </DropdownMenuItem>
              <DropdownMenuItem onClick={() => onUninstall?.()} className="text-error">
                <Trash className="mr-2 h-4 w-4" />
                {t('card.uninstall', { defaultValue: "Uninstall" })}
              </DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>
        </div>

        {/* Description — the wrapper always reserves the two-line height
            (min-h-10 ≈ 2 lines of text-xs/leading-relaxed) so every card in
            the grid is equally tall whether the text is absent, one line, or
            clamped at two; same pattern as BuiltinModelWizard's model cards.
            The clamp needs display:-webkit-box, which a display utility on
            the same element would override (responsive variants sort later
            in the compiled CSS), so visibility lives on this wrapper and the
            inner p keeps the same clamp classes as AgentCard's description. */}
        <div className="mb-3 hidden min-h-10 md:block">
          {extension.description && (
            <p className="line-clamp-2 text-xs leading-relaxed text-muted-foreground">
              {extension.description}
            </p>
          )}
        </div>

        {/* Error summary — the card shows WHY it's red without opening
            details; min-h-10 keeps error cards uniform among themselves. */}
        {hasError && extension.last_error && (
          <div className="mb-3 hidden min-h-10 md:block">
            <p className="line-clamp-2 break-words text-xs leading-relaxed text-error">
              {extension.last_error}
            </p>
          </div>
        )}

        {/* Footer: capabilities + (optional) AI-tools-off badge + author.
            Single line, stable height whether tools are on or off. */}
        <div className="mt-auto hidden items-center gap-2 pt-2 md:flex">
          {extension.commands?.length > 0 && (
            <span className={cn(textMini, "text-muted-foreground flex items-center gap-1 shrink-0")}>
              <Terminal className="h-3 w-3" />
              {extension.commands.length} {t('commandsLabel', { defaultValue: 'commands' })}
            </span>
          )}
          {extension.metrics?.length > 0 && (
            <span className={cn(textMini, "text-muted-foreground flex items-center gap-1 shrink-0")}>
              <Database className="h-3 w-3" />
              {extension.metrics.length} {t('metricsLabel', { defaultValue: 'metrics' })}
            </span>
          )}
          {!toolsEnabled && (
            <span
              className={cn(
                textNano,
                "flex items-center gap-1 px-1.5 py-0.5 rounded-full shrink-0",
                "bg-warning-light text-warning",
              )}
              title={t('card.aiToolsOff', { defaultValue: 'Hidden from agent' })}
            >
              <EyeOff className="h-3 w-3" />
              {t('card.aiToolsOffShort', { defaultValue: 'AI off' })}
            </span>
          )}
          {extension.author && (
            <span className={cn(textNano, "text-muted-foreground truncate ml-auto")} title={extension.author}>
              {t('card.byAuthor', { author: extension.author })}
            </span>
          )}
        </div>
      </CardContent>
    </Card>
  )
}
