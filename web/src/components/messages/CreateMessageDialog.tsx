// CreateMessageDialog Component
// Dialog for creating new messages/notifications
// Uses UnifiedFormDialog for consistent styling across mobile and desktop

import { useState, useCallback } from 'react'
import { useTranslation } from 'react-i18next'
import { MessageSquare } from 'lucide-react'
import { Input } from '@/components/ui/input'
import { Textarea } from '@/components/ui/textarea'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { FormField } from '@/components/ui/field'
import { FormSection, FormSectionGroup } from '@/components/ui/form-section'
import { UnifiedFormDialog } from '@/components/dialog/UnifiedFormDialog'
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

  // Validation state
  const [titleError, setTitleError] = useState<string | null>(null)
  const [messageError, setMessageError] = useState<string | null>(null)

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
      setTitleError(null)
      setMessageError(null)
      onOpenChange(false)
    },
    errorOperation: 'Create message',
  })

  const validateForm = useCallback(() => {
    let isValid = true

    if (!title.trim()) {
      setTitleError(t('messages.formTitle.required', { defaultValue: 'Title is required' }))
      isValid = false
    } else {
      setTitleError(null)
    }

    if (!message.trim()) {
      setMessageError(t('messages.content.required', { defaultValue: 'Message is required' }))
      isValid = false
    } else {
      setMessageError(null)
    }

    return isValid
  }, [title, message, t])

  const handleFormSubmit = useCallback(async () => {
    if (!validateForm()) return

    await wrapSubmit(async () => {
      const request: CreateMessageRequest = {
        category,
        severity,
        title: title.trim(),
        message: message.trim(),
        source: source || undefined,
        source_type: sourceType || undefined,
        tags: tags ? tags.split(',').map(t => t.trim()).filter(Boolean) : undefined,
      }
      await onCreate(request)
    })()
  }, [validateForm, wrapSubmit, onCreate, category, severity, title, message, source, sourceType, tags])

  return (
    <UnifiedFormDialog
      open={open}
      onOpenChange={onOpenChange}
      title={t('messages.createTitle')}
      icon={<MessageSquare className="h-5 w-5" />}
      width="md"
      onSubmit={handleFormSubmit}
      isSubmitting={isSubmitting}
      submitLabel={t('common:create')}
      loading={false}
    >
      <FormSectionGroup>
        {/* Basic Settings Section */}
        <FormSection
          title={t('messages.basicSettings', { defaultValue: 'Basic Settings' })}
          description={t('messages.basicSettingsDesc', { defaultValue: 'Configure category and severity' })}
        >
          <div className="space-y-4">
            {/* Category and Severity on same row */}
            <div className="grid grid-cols-1 sm:grid-cols-2 gap-4">
              <FormField label={t('messages.category.label')}>
                <Select value={category} onValueChange={(v) => setCategory(v as MessageCategory)}>
                  <SelectTrigger>
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="alert">{t('messages.category.alert')}</SelectItem>
                    <SelectItem value="system">{t('messages.category.system')}</SelectItem>
                    <SelectItem value="business">{t('messages.category.business')}</SelectItem>
                  </SelectContent>
                </Select>
              </FormField>

              <FormField label={t('messages.severity.label')}>
                <Select value={severity} onValueChange={(v) => setSeverity(v as MessageSeverity)}>
                  <SelectTrigger>
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="info">{t('messages.severity.info')}</SelectItem>
                    <SelectItem value="warning">{t('messages.severity.warning')}</SelectItem>
                    <SelectItem value="critical">{t('messages.severity.critical')}</SelectItem>
                    <SelectItem value="emergency">{t('messages.severity.emergency')}</SelectItem>
                  </SelectContent>
                </Select>
              </FormField>
            </div>
          </div>
        </FormSection>

        {/* Content Section */}
        <FormSection
          title={t('messages.contentSection', { defaultValue: 'Content' })}
          description={t('messages.contentSectionDesc', { defaultValue: 'Enter message title and body' })}
        >
          <div className="space-y-4">
            <FormField
              label={t('messages.formTitle.label')}
              required
              error={titleError || undefined}
            >
              <Input
                value={title}
                onChange={(e) => {
                  setTitle(e.target.value)
                  if (titleError) setTitleError(null)
                }}
                placeholder={t('messages.formTitle.placeholder')}
                autoFocus
              />
            </FormField>

            <FormField
              label={t('messages.content.label')}
              required
              error={messageError || undefined}
            >
              <Textarea
                value={message}
                onChange={(e) => {
                  setMessage(e.target.value)
                  if (messageError) setMessageError(null)
                }}
                placeholder={t('messages.content.placeholder')}
                rows={3}
              />
            </FormField>
          </div>
        </FormSection>

        {/* Source Section */}
        <FormSection
          title={t('messages.sourceSection', { defaultValue: 'Source (Optional)' })}
          description={t('messages.sourceSectionDesc', { defaultValue: 'Identify the source of this message' })}
          collapsible
          defaultExpanded={false}
        >
          <div className="grid grid-cols-1 sm:grid-cols-2 gap-4">
            <FormField
              label={t('messages.sourceLabel')}
            >
              <Input
                value={source}
                onChange={(e) => setSource(e.target.value)}
                placeholder={t('messages.sourcePlaceholder')}
              />
            </FormField>

            <FormField
              label={t('messages.sourceType.label')}
            >
              <Input
                value={sourceType}
                onChange={(e) => setSourceType(e.target.value)}
                placeholder={t('messages.sourceType.placeholder')}
              />
            </FormField>

            <FormField
              label={t('messages.tags.label')}
              helpText={t('messages.tags.hint')}
            >
              <Input
                value={tags}
                onChange={(e) => setTags(e.target.value)}
                placeholder="tag1, tag2, tag3"
              />
            </FormField>
          </div>
        </FormSection>
      </FormSectionGroup>
    </UnifiedFormDialog>
  )
}
