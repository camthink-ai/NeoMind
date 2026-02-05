import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card"
import { Button } from "@/components/ui/button"
import { Badge } from "@/components/ui/badge"
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"
import {
  MoreHorizontal,
  Play,
  Square,
  Trash2,
  Settings,
  FileCode,
  Cpu,
  Shield,
  Wrench,
  Package,
} from "lucide-react"
import { useTranslation } from "react-i18next"
import type { Extension } from "@/types"

interface ExtensionCardProps {
  extension: Extension
  onStart?: () => void
  onStop?: () => void
  onDelete?: () => void
  onConfigure?: () => void
}

// Extension type icons
const EXTENSION_ICONS: Record<string, React.ElementType> = {
  llm_provider: Cpu,
  device_protocol: Shield,
  alert_channel_type: Package,
  tool: Wrench,
  generic: FileCode,
}

// Extension type colors
const EXTENSION_COLORS: Record<string, string> = {
  llm_provider: "text-purple-500",
  device_protocol: "text-blue-500",
  alert_channel_type: "text-orange-500",
  tool: "text-green-500",
  generic: "text-gray-500",
}

export function ExtensionCard({ extension, onStart, onStop, onDelete, onConfigure }: ExtensionCardProps) {
  const { t } = useTranslation(["extensions"])
  const isRunning = extension.state === "Running"
  const Icon = EXTENSION_ICONS[extension.extension_type] || FileCode
  const colorClass = EXTENSION_COLORS[extension.extension_type] || "text-gray-500"
  const typeNameKey = `card.types.${extension.extension_type}` || "card.types.unknown"
  const stateNameKey = `card.states.${extension.state}` || extension.state

  // State badge color
  const getStateColor = (state: string) => {
    switch (state) {
      case "Running": return "default"
      case "Stopped": return "secondary"
      case "Error": return "destructive"
      case "Initialized": return "outline"
      default: return "secondary"
    }
  }

  return (
    <Card className="group hover:shadow-md transition-shadow">
      <CardHeader className="pb-3">
        <div className="flex items-start justify-between">
          <div className="flex items-center gap-3">
            <div className={`p-2 rounded-lg bg-muted ${colorClass}`}>
              <Icon className="h-5 w-5" />
            </div>
            <div>
              <CardTitle className="text-lg font-semibold">{extension.name}</CardTitle>
              <CardDescription className="flex items-center gap-2 mt-1">
                <span>{extension.id}</span>
                {extension.version && (
                  <Badge variant="outline" className="text-xs">v{extension.version}</Badge>
                )}
              </CardDescription>
            </div>
          </div>
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <Button variant="ghost" size="icon" className="h-8 w-8">
                <MoreHorizontal className="h-4 w-4" />
              </Button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end">
              {isRunning ? (
                <DropdownMenuItem onClick={() => onStop?.()}>
                  <Square className="mr-2 h-4 w-4" />
                  {t('card.stop')}
                </DropdownMenuItem>
              ) : (
                <DropdownMenuItem onClick={() => onStart?.()}>
                  <Play className="mr-2 h-4 w-4" />
                  {t('card.start')}
                </DropdownMenuItem>
              )}
              <DropdownMenuItem onClick={() => onConfigure?.()}>
                <Settings className="mr-2 h-4 w-4" />
                {t('card.configure')}
              </DropdownMenuItem>
              <DropdownMenuItem onClick={() => onDelete?.()} className="text-destructive">
                <Trash2 className="mr-2 h-4 w-4" />
                {t('card.unregister')}
              </DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>
        </div>
      </CardHeader>
      <CardContent className="space-y-4">
        {/* Extension Type Badge */}
        <div className="flex items-center gap-2">
          <Badge variant="secondary" className="text-xs">
            {t(typeNameKey)}
          </Badge>
          <Badge variant={getStateColor(extension.state)} className="text-xs">
            {t(stateNameKey)}
          </Badge>
        </div>

        {/* Description */}
        {extension.description && (
          <p className="text-sm text-muted-foreground line-clamp-2">
            {extension.description}
          </p>
        )}

        {/* Author */}
        {extension.author && (
          <p className="text-xs text-muted-foreground">
            {t('card.byAuthor', { author: extension.author })}
          </p>
        )}

        {/* File path */}
        {extension.file_path && (
          <p className="text-xs text-muted-foreground font-mono" title={extension.file_path}>
            {extension.file_path.split("/").pop()}
          </p>
        )}

        {/* Action buttons */}
        <div className="flex items-center justify-between pt-2 border-t">
          <span className="text-xs text-muted-foreground">
            {isRunning ? t('card.running') : t('card.stopped')}
          </span>
          <div className="flex gap-2">
            {isRunning ? (
              <Button size="sm" variant="outline" onClick={() => onStop?.()}>
                <Square className="mr-1 h-3 w-3" />
                {t('card.stop')}
              </Button>
            ) : (
              <Button size="sm" variant="default" onClick={() => onStart?.()}>
                <Play className="mr-1 h-3 w-3" />
                {t('card.start')}
              </Button>
            )}
          </div>
        </div>
      </CardContent>
    </Card>
  )
}
