import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { X, Loader2 } from 'lucide-react'
import { Dialog, DialogContent, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { fetchAPI } from '@/lib/api'

interface LLMBackendConfigDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  backend?: LlmBackendInstance | null
  mode: 'create' | 'edit'
}

interface LlmBackendInstance {
  id: string
  name: string
  backend_type: string
  config: Record<string, unknown>
  enabled: boolean
}

interface BackendTypeDefinition {
  id: string
  name: string
  description: string
  config_schema?: Record<string, unknown>
}

export function LLMBackendConfigDialog({
  open,
  onOpenChange,
  backend,
  mode,
}: LLMBackendConfigDialogProps) {
  const { t } = useTranslation()
  const [loading, setLoading] = useState(false)
  const [config, setConfig] = useState<Record<string, unknown>>({})
  const [name, setName] = useState('')

  useEffect(() => {
    if (backend && mode === 'edit') {
      setName(backend.name)
      setConfig(backend.config || {})
    } else {
      setName('')
      setConfig({})
    }
  }, [backend, mode])

  const handleSubmit = async () => {
    setLoading(true)
    try {
      if (mode === 'create') {
        // TODO: Implement create
      } else {
        await fetchAPI(`/api/llm-backends/${backend?.id}`, {
          method: 'PUT',
          body: JSON.stringify({ name, config }),
        })
      }
      onOpenChange(false)
    } catch (error) {
      console.error('Failed to save backend:', error)
    } finally {
      setLoading(false)
    }
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-2xl max-h-[80vh] overflow-y-auto">
        <DialogHeader>
          <DialogTitle>
            {mode === 'create' ? t('settings:llm.create') : t('settings:llm.edit')}
          </DialogTitle>
        </DialogHeader>

        <div className="space-y-4 py-4">
          <div>
            <Label htmlFor="name">{t('settings:llm.name')}</Label>
            <Input
              id="name"
              value={name}
              onChange={(e) => setName(e.target.value)}
              placeholder={t('settings:llm.namePlaceholder')}
            />
          </div>

          {/* Config fields would go here */}
          <div className="text-sm text-muted-foreground">
            Configuration fields will be dynamically generated based on backend type
          </div>
        </div>

        <div className="flex justify-end gap-2">
          <Button variant="outline" onClick={() => onOpenChange(false)}>
            {t('common:cancel')}
          </Button>
          <Button onClick={handleSubmit} disabled={loading}>
            {loading && <Loader2 className="mr-2 h-4 w-4 animate-spin" />}
            {mode === 'create' ? t('common:create') : t('common:save')}
          </Button>
        </div>
      </DialogContent>
    </Dialog>
  )
}
