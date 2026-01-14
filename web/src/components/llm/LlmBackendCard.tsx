import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { CheckCircle, XCircle, Loader2, Trash2, Edit, Zap, MoreVertical } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardFooter, CardHeader, CardTitle } from '@/components/ui/card'
import { Badge } from '@/components/ui/badge'
import { Switch } from '@/components/ui/switch'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import { cn } from '@/lib/utils'
import type { LlmBackendInstance, BackendTestResult } from '@/types'

interface LlmBackendCardProps {
  backend: LlmBackendInstance
  testResult?: BackendTestResult
  onActivate: () => Promise<boolean>
  onEdit: () => void
  onDelete: () => Promise<boolean>
  onTest: () => Promise<BackendTestResult>
}

export function LlmBackendCard({
  backend,
  testResult,
  onActivate,
  onEdit,
  onDelete,
  onTest,
}: LlmBackendCardProps) {
  const { t } = useTranslation(['plugins', 'common'])
  const [activating, setActivating] = useState(false)
  const [deleting, setDeleting] = useState(false)
  const [testing, setTesting] = useState(false)
  const [currentTestResult, setCurrentTestResult] = useState<BackendTestResult | undefined>(testResult)

  const handleActivate = async () => {
    if (backend.is_active) return
    setActivating(true)
    const success = await onActivate()
    setActivating(false)
    if (!success) {
      // Could show toast here
    }
  }

  const handleDelete = async () => {
    if (!confirm(t('plugins:llm.deleteConfirm', { name: backend.name }))) return
    setDeleting(true)
    const success = await onDelete()
    setDeleting(false)
    if (!success) {
      // Could show toast here
    }
  }

  const handleTest = async () => {
    setTesting(true)
    const result = await onTest()
    setCurrentTestResult(result)
    setTesting(false)
  }

  const getBackendIcon = (type: string) => {
    switch (type) {
      case 'ollama':
        return '🦙'
      case 'openai':
        return '🤖'
      case 'anthropic':
        return '🧠'
      case 'google':
        return '🔍'
      case 'xai':
        return '❌'
      default:
        return '⚙️'
    }
  }

  const getBackendTypeLabel = (type: string) => {
    switch (type) {
      case 'ollama':
        return 'Ollama'
      case 'openai':
        return 'OpenAI'
      case 'anthropic':
        return 'Anthropic'
      case 'google':
        return 'Google'
      case 'xai':
        return 'xAI'
      default:
        return type
    }
  }

  const healthStatus = currentTestResult?.success
    ? backend.healthy !== false
    : backend.healthy

  return (
    <Card className={cn(
      'transition-all duration-200',
      backend.is_active && 'border-primary border-2 shadow-sm',
      !backend.is_active && 'hover:shadow-md'
    )}>
      <CardHeader className="pb-3">
        <div className="flex items-start justify-between">
          <div className="flex items-center gap-3">
            <span className="text-2xl">{getBackendIcon(backend.backend_type)}</span>
            <div>
              <CardTitle className="text-lg flex items-center gap-2">
                {backend.name}
                {backend.is_active && (
                  <Badge variant="default" className="text-xs">{t('plugins:llm.active')}</Badge>
                )}
              </CardTitle>
              <CardDescription className="flex items-center gap-2 mt-1">
                <span>{getBackendTypeLabel(backend.backend_type)}</span>
                <span>•</span>
                <span className="font-mono text-xs">{backend.model}</span>
              </CardDescription>
            </div>
          </div>

          {/* Health status indicator */}
          <div className="flex items-center gap-2">
            {healthStatus === true ? (
              <CheckCircle className="h-5 w-5 text-success" />
            ) : healthStatus === false ? (
              <XCircle className="h-5 w-5 text-error" />
            ) : null}

            <DropdownMenu>
              <DropdownMenuTrigger asChild>
                <Button variant="ghost" size="sm" className="h-8 w-8 p-0">
                  <MoreVertical className="h-4 w-4" />
                </Button>
              </DropdownMenuTrigger>
              <DropdownMenuContent align="end">
                <DropdownMenuItem onClick={onEdit}>
                  <Edit className="mr-2 h-4 w-4" />
                  {t('plugins:edit')}
                </DropdownMenuItem>
                <DropdownMenuItem onClick={handleTest} disabled={testing}>
                  <Zap className="mr-2 h-4 w-4" />
                  {testing ? t('plugins:llm.activating') : t('plugins:llm.testConnection')}
                </DropdownMenuItem>
                <DropdownMenuItem
                  onClick={handleDelete}
                  disabled={deleting || backend.is_active}
                  className="text-destructive focus:text-destructive"
                >
                  <Trash2 className="mr-2 h-4 w-4" />
                  {deleting ? t('plugins:deleting') : t('plugins:delete')}
                </DropdownMenuItem>
              </DropdownMenuContent>
            </DropdownMenu>
          </div>
        </div>
      </CardHeader>

      <CardContent className="pb-3">
        <div className="space-y-2 text-sm">
          {/* Endpoint */}
          {backend.endpoint && (
            <div className="flex items-center justify-between">
              <span className="text-muted-foreground">{t('plugins:llm.endpoint')}:</span>
              <span className="font-mono text-xs truncate max-w-[200px]">{backend.endpoint}</span>
            </div>
          )}

          {/* Capabilities */}
          <div className="flex gap-1 flex-wrap">
            {backend.capabilities.supports_streaming && (
              <Badge variant="outline" className="text-xs">{t('plugins:llm.streaming')}</Badge>
            )}
            {backend.capabilities.supports_thinking && (
              <Badge variant="outline" className="text-xs">{t('plugins:llm.thinking')}</Badge>
            )}
            {backend.capabilities.supports_multimodal && (
              <Badge variant="outline" className="text-xs">{t('plugins:llm.multimodal')}</Badge>
            )}
            <Badge variant="outline" className="text-xs">
              {t('plugins:llm.context')}: {backend.capabilities.max_context >= 1000
                ? `${Math.round(backend.capabilities.max_context / 1000)}k`
                : backend.capabilities.max_context}
            </Badge>
          </div>

          {/* Test result */}
          {currentTestResult && (
            <div className={cn(
              'text-xs p-2 rounded',
              currentTestResult.success ? 'badge-success' : 'badge-error'
            )}>
              {currentTestResult.success
                ? t('plugins:llm.connectionSuccess', { latency: currentTestResult.latency_ms?.toFixed(0) || '0' })
                : t('plugins:llm.connectionFailed', { error: currentTestResult.error })}
            </div>
          )}
        </div>
      </CardContent>

      <CardFooter className="pt-3 border-t">
        <div className="flex items-center justify-between w-full">
          <div className="flex items-center gap-2">
            <Switch
              checked={backend.is_active}
              onCheckedChange={handleActivate}
              disabled={activating || backend.is_active}
            />
            <span className="text-sm text-muted-foreground">
              {backend.is_active ? t('plugins:llm.currentActive') : t('plugins:llm.setAsActive')}
            </span>
          </div>

          {!backend.is_active && (
            <Button
              size="sm"
              variant="outline"
              onClick={handleActivate}
              disabled={activating}
            >
              {activating ? (
                <>
                  <Loader2 className="mr-2 h-4 w-4 animate-spin" />
                  {t('plugins:llm.activating')}
                </>
              ) : (
                t('plugins:llm.activate')
              )}
            </Button>
          )}
        </div>
      </CardFooter>
    </Card>
  )
}
