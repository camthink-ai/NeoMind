// CreateMessageDialog Component
// Dialog for creating new messages/notifications

import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  DialogContentBody,
} from '@/components/ui/dialog'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Textarea } from '@/components/ui/textarea'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import type { CreateMessageRequest, MessageSeverity, MessageCategory } from '@/types'
import { useFormSubmit } from '@/hooks/useErrorHandler'

interface CreateMessageDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  onCreate: (req: CreateMessageRequest) => Promise<void>
}

export function CreateMessageDialog({ open, onOpenChange, onCreate }: CreateMessageDialogProps) {
  const { t } = useTranslation()

  const [category, setCategory] = useState<MessageCategory>('alert')
  const [severity, setSeverity] = useState<MessageSeverity>('info')
  const [title, setTitle] = useState('')
  const [message, setMessage] = useState('')
  const [source, setSource] = useState('')
  const [sourceType, setSourceType] = useState('')
  const [tags, setTags] = useState('')

  const { isSubmitting, handleSubmit: wrapSubmit } = useFormSubmit({
    onSuccess: () => {
      // Reset form
      setTitle('')
      setMessage('')
      setSource('')
      setSourceType('')
      setTags('')
      setSeverity('info')
      setCategory('alert')
      onOpenChange(false)
    },
    errorOperation: 'Create message',
  })

  const handleSubmit = () => {
    if (!title.trim() || !message.trim()) return

    wrapSubmit(async () => {
      await onCreate({
        category,
        severity,
        title: title.trim(),
        message: message.trim(),
        source: source || undefined,
        source_type: sourceType || undefined,
        tags: tags ? tags.split(',').map(t => t.trim()).filter(Boolean) : undefined,
      })
    })()
  }

  const isValid = title.trim() && message.trim()

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-[500px] flex flex-col">
        <DialogHeader>
          <DialogTitle>{t('messages.createTitle')}</DialogTitle>
          <DialogDescription>
            {t('messages.createDescription')}
          </DialogDescription>
        </DialogHeader>

        <DialogContentBody className="flex flex-col gap-4 py-4 overflow-y-auto">
          {/* Category and Severity - side by side on desktop only */}
          <div className="grid grid-cols-1 sm:grid-cols-2 gap-4">
            <div className="space-y-2">
              <Label htmlFor="category">{t('messages.category.label')}</Label>
              <Select value={category} onValueChange={(v) => setCategory(v as MessageCategory)}>
                <SelectTrigger id="category">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="alert">{t('messages.category.alert')}</SelectItem>
                  <SelectItem value="system">{t('messages.category.system')}</SelectItem>
                  <SelectItem value="business">{t('messages.category.business')}</SelectItem>
                </SelectContent>
              </Select>
            </div>

            <div className="space-y-2">
              <Label htmlFor="severity">{t('messages.severity.label')}</Label>
              <Select value={severity} onValueChange={(v) => setSeverity(v as MessageSeverity)}>
                <SelectTrigger id="severity">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="info">{t('messages.severity.info')}</SelectItem>
                  <SelectItem value="warning">{t('messages.severity.warning')}</SelectItem>
                  <SelectItem value="critical">{t('messages.severity.critical')}</SelectItem>
                  <SelectItem value="emergency">{t('messages.severity.emergency')}</SelectItem>
                </SelectContent>
              </Select>
            </div>
          </div>

          {/* Title */}
          <div className="space-y-2">
            <Label htmlFor="title">{t('messages.formTitle.label')} *</Label>
            <Input
              id="title"
              value={title}
              onChange={(e) => setTitle(e.target.value)}
              placeholder={t('messages.formTitle.placeholder')}
            />
          </div>

          {/* Message */}
          <div className="space-y-2">
            <Label htmlFor="message-content">{t('messages.content.label')} *</Label>
            <Textarea
              id="message-content"
              value={message}
              onChange={(e) => setMessage(e.target.value)}
              placeholder={t('messages.content.placeholder')}
              rows={3}
            />
          </div>

          {/* Source and Source Type - side by side on desktop only */}
          <div className="grid grid-cols-1 sm:grid-cols-2 gap-4">
            <div className="space-y-2">
              <Label htmlFor="source">{t('messages.sourceLabel')}</Label>
              <Input
                id="source"
                value={source}
                onChange={(e) => setSource(e.target.value)}
                placeholder={t('messages.sourcePlaceholder')}
              />
            </div>

            <div className="space-y-2">
              <Label htmlFor="source-type">{t('messages.sourceType.label')}</Label>
              <Input
                id="source-type"
                value={sourceType}
                onChange={(e) => setSourceType(e.target.value)}
                placeholder={t('messages.sourceType.placeholder')}
              />
            </div>
          </div>

          {/* Tags */}
          <div className="space-y-2">
            <Label htmlFor="tags">{t('messages.tags.label')}</Label>
            <Input
              id="tags"
              value={tags}
              onChange={(e) => setTags(e.target.value)}
              placeholder="tag1, tag2, tag3"
            />
            <p className="text-xs text-muted-foreground">
              {t('messages.tags.hint')}
            </p>
          </div>
        </DialogContentBody>

        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)} disabled={isSubmitting}>
            {t('common:cancel')}
          </Button>
          <Button onClick={handleSubmit} disabled={!isValid || isSubmitting}>
            {isSubmitting ? t('common:creating') : t('common:create')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
