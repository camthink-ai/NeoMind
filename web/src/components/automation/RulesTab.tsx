import { useState, useEffect } from 'react'
import { useTranslation } from 'react-i18next'
import { Card } from '@/components/ui/card'
import { Button } from '@/components/ui/button'
import { Badge } from '@/components/ui/badge'
import { Switch } from '@/components/ui/switch'
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Textarea } from '@/components/ui/textarea'
import { Label } from '@/components/ui/label'
import { Input } from '@/components/ui/input'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs'
import { Plus, Trash2, Edit, Zap, Clock, FileDown, FileUp, Wand2, Loader2 } from 'lucide-react'
import { ActionBar, EmptyStateInline } from '@/components/shared'
import { api } from '@/lib/api'
import type { Rule } from '@/types'

interface RuleTemplate {
  id: string
  name: string
  category: string
  description: string
  parameters: Array<{
    name: string
    label: string
    default?: string
    required: boolean
  }>
}

interface RulesTabProps {
  onRefresh?: () => void
}

type CreateMode = 'manual' | 'template' | 'generate'

export function RulesTab({ onRefresh }: RulesTabProps) {
  const { t } = useTranslation(['automation', 'common'])
  const [rules, setRules] = useState<Rule[]>([])
  const [loading, setLoading] = useState(true)
  const [createDialogOpen, setCreateDialogOpen] = useState(false)
  const [editRule, setEditRule] = useState<Rule | null>(null)
  const [newRuleName, setNewRuleName] = useState('')
  const [newRuleDSL, setNewRuleDSL] = useState('')
  const [createMode, setCreateMode] = useState<CreateMode>('manual')

  // Template state
  const [templates, setTemplates] = useState<RuleTemplate[]>([])
  const [selectedTemplate, setSelectedTemplate] = useState<RuleTemplate | null>(null)
  const [templateParams, setTemplateParams] = useState<Record<string, string>>({})
  const [templatePreview, setTemplatePreview] = useState('')

  // Generate state
  const [description, setDescription] = useState('')
  const [generating, setGenerating] = useState(false)
  const [generatedResult, setGeneratedResult] = useState<{ dsl: string; explanation: string; warnings: string[] } | null>(null)

  // Resources state
  const [resources, setResources] = useState<{ devices: Array<{ id: string; name: string, type: string }>; metrics: string[]; alert_channels: string[] }>({
    devices: [],
    metrics: [],
    alert_channels: []
  })

  const [validation, setValidation] = useState<{ valid: boolean; errors?: string[] } | null>(null)
  const [validating, setValidating] = useState(false)

  const fetchRules = async () => {
    setLoading(true)
    try {
      const result = await api.listRules()
      setRules(result.rules || [])
    } catch (error) {
      console.error('Failed to fetch rules:', error)
    } finally {
      setLoading(false)
    }
  }

  const fetchResources = async () => {
    try {
      const result = await api.getRuleResources()
      setResources(result)
    } catch (error) {
      console.error('Failed to fetch resources:', error)
    }
  }

  useEffect(() => {
    fetchRules()
    fetchResources()
  }, [])

  const handleToggleRule = async (rule: Rule) => {
    try {
      if (rule.enabled) {
        await api.disableRule(rule.id)
      } else {
        await api.enableRule(rule.id)
      }
      await fetchRules()
      onRefresh?.()
    } catch (error) {
      console.error('Failed to toggle rule:', error)
    }
  }

  const handleDeleteRule = async (id: string) => {
    if (!confirm(t('automation:deleteConfirm'))) return
    try {
      await api.deleteRule(id)
      await fetchRules()
      onRefresh?.()
    } catch (error) {
      console.error('Failed to delete rule:', error)
    }
  }

  const handleValidate = async () => {
    setValidating(true)
    try {
      const result = await api.validateRuleDSL(newRuleDSL)
      setValidation(result)
    } catch (error) {
      setValidation({ valid: false, errors: [t('automation:dslInvalid')] })
    } finally {
      setValidating(false)
    }
  }

  const handleCreateRule = async () => {
    if (!newRuleName.trim() || !newRuleDSL.trim()) return
    try {
      await api.createRule({
        name: newRuleName,
        dsl: newRuleDSL,
        enabled: true,
        trigger_count: 0,
      })
      handleCloseDialog()
      await fetchRules()
      onRefresh?.()
    } catch (error) {
      console.error('Failed to create rule:', error)
    }
  }

  const handleEditRule = async () => {
    if (!editRule) return
    try {
      await api.updateRule(editRule.id, {
        name: editRule.name,
        dsl: editRule.dsl,
      })
      setEditRule(null)
      await fetchRules()
      onRefresh?.()
    } catch (error) {
      console.error('Failed to update rule:', error)
    }
  }

  const handleExportRules = async () => {
    try {
      const result = await api.exportRules()
      const dataStr = JSON.stringify(result, null, 2)
      const dataBlob = new Blob([dataStr], { type: 'application/json' })
      const url = URL.createObjectURL(dataBlob)
      const link = document.createElement('a')
      link.href = url
      link.download = `neotalk-rules-${new Date().toISOString().split('T')[0]}.json`
      link.click()
      URL.revokeObjectURL(url)
    } catch (error) {
      console.error('Failed to export rules:', error)
    }
  }

  const handleImportRules = async () => {
    const input = document.createElement('input')
    input.type = 'file'
    input.accept = 'application/json'
    input.onchange = async (e) => {
      const file = (e.target as HTMLInputElement).files?.[0]
      if (!file) return
      try {
        const text = await file.text()
        const data = JSON.parse(text)
        const result = await api.importRules(data.rules || data)
        alert(`${t('automation:imported')}: ${result.imported}, ${t('automation:skipped')}: ${result.skipped}`)
        await fetchRules()
        onRefresh?.()
      } catch (error) {
        console.error('Failed to import rules:', error)
        alert(t('automation:importFailed'))
      }
    }
    input.click()
  }

  const handleGenerate = async () => {
    if (!description.trim()) return
    setGenerating(true)
    try {
      const result = await api.generateRule(description, { devices: resources.devices })
      setGeneratedResult({
        dsl: result.dsl,
        explanation: result.explanation,
        warnings: result.warnings || [],
      })
      setNewRuleDSL(result.dsl)
      setNewRuleName(t('automation:generatedRule'))
    } catch (error) {
      console.error('Failed to generate rule:', error)
    } finally {
      setGenerating(false)
    }
  }

  const handleLoadTemplates = async () => {
    try {
      const result = await api.getRuleTemplates()
      setTemplates(result)
    } catch (error) {
      console.error('Failed to load templates:', error)
    }
  }

  const handleSelectTemplate = async (template: RuleTemplate) => {
    setSelectedTemplate(template)
    setTemplateParams({})
    setTemplatePreview('')
  }

  const handleFillTemplate = async () => {
    if (!selectedTemplate) return
    try {
      const result = await api.fillRuleTemplate(selectedTemplate.id, templateParams)
      setTemplatePreview(result.dsl)
    } catch (error) {
      console.error('Failed to fill template:', error)
    }
  }

  const handleUseTemplate = () => {
    if (templatePreview) {
      setNewRuleDSL(templatePreview)
      setNewRuleName(selectedTemplate?.name || t('automation:newRule'))
      setCreateMode('manual')
    }
  }

  const handleCloseDialog = () => {
    setCreateDialogOpen(false)
    setCreateMode('manual')
    setNewRuleName('')
    setNewRuleDSL('')
    setValidation(null)
    setSelectedTemplate(null)
    setTemplateParams({})
    setTemplatePreview('')
    setDescription('')
    setGeneratedResult(null)
  }

  const formatTimestamp = (timestamp: string | number | undefined) => {
    if (!timestamp) return '-'
    if (typeof timestamp === 'string') {
      return new Date(timestamp).toLocaleString('zh-CN', {
        month: '2-digit',
        day: '2-digit',
        hour: '2-digit',
        minute: '2-digit',
      })
    }
    const date = new Date(timestamp * 1000)
    const now = new Date()
    const diff = now.getTime() - date.getTime()

    if (diff < 60000) return t('automation:justNow')
    if (diff < 3600000) return `${Math.floor(diff / 60000)} ${t('automation:minutesAgo')}`
    if (diff < 86400000) return `${Math.floor(diff / 3600000)} ${t('automation:hoursAgo')}`

    return date.toLocaleString('zh-CN', {
      month: '2-digit',
      day: '2-digit',
      hour: '2-digit',
      minute: '2-digit',
    })
  }

  return (
    <>
      {/* Header with actions */}
      <ActionBar
        title={t('automation:rulesTitle')}
        titleIcon={<Zap className="h-5 w-5" />}
        description={t('automation:rulesDesc')}
        actions={[
          {
            label: t('automation:rulesAdd'),
            icon: <Plus className="h-4 w-4" />,
            onClick: () => setCreateDialogOpen(true),
          },
          {
            label: t('automation:export'),
            icon: <FileDown className="h-4 w-4" />,
            onClick: handleExportRules,
          },
          {
            label: t('automation:import'),
            icon: <FileUp className="h-4 w-4" />,
            onClick: handleImportRules,
          },
        ]}
        onRefresh={onRefresh}
      />

      <Card>
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead>{t('automation:ruleName')}</TableHead>
              <TableHead align="center">{t('automation:enabled')}</TableHead>
              <TableHead align="center">{t('automation:todayTriggered')}</TableHead>
              <TableHead>{t('automation:lastExecution')}</TableHead>
              <TableHead align="right">{t('automation:actions')}</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {loading ? (
              <EmptyStateInline title={t('automation:loading')} colSpan={5} />
            ) : rules.length === 0 ? (
              <EmptyStateInline title={t('automation:noRules')} colSpan={5} />
            ) : (
              rules.map((rule) => (
                <TableRow key={rule.id}>
                  <TableCell>
                    <div>
                      <div className="font-medium">{rule.name}</div>
                      <div className="text-xs text-muted-foreground font-mono truncate max-w-md">
                        {rule.dsl}
                      </div>
                    </div>
                  </TableCell>
                  <TableCell align="center">
                    <div className="flex items-center justify-center gap-2">
                      <Switch
                        checked={rule.enabled}
                        onCheckedChange={() => handleToggleRule(rule)}
                      />
                      <Badge variant={rule.enabled ? 'default' : 'secondary'}>
                        {rule.enabled ? t('automation:enabled') : t('automation:disabled')}
                      </Badge>
                    </div>
                  </TableCell>
                  <TableCell align="center">
                    <div className="flex items-center justify-center gap-1">
                      <Zap className="h-4 w-4 text-warning" />
                      {rule.trigger_count || 0}
                    </div>
                  </TableCell>
                  <TableCell className="text-sm text-muted-foreground">
                    {rule.last_triggered ? (
                      <div className="flex items-center gap-1">
                        <Clock className="h-4 w-4" />
                        {formatTimestamp(rule.last_triggered)}
                      </div>
                    ) : (
                      <span className="text-xs text-muted-foreground">{t('automation:noHistory')}</span>
                    )}
                  </TableCell>
                  <TableCell align="right">
                    <div className="flex items-center justify-end gap-1">
                      <Button variant="ghost" size="icon" className="h-8 w-8" onClick={() => setEditRule(rule)}>
                        <Edit className="h-4 w-4" />
                      </Button>
                      <Button variant="ghost" size="icon" className="h-8 w-8" onClick={() => handleDeleteRule(rule.id)}>
                        <Trash2 className="h-4 w-4 text-destructive" />
                      </Button>
                    </div>
                  </TableCell>
                </TableRow>
              ))
            )}
          </TableBody>
        </Table>
      </Card>

      {/* Create Rule Dialog with Tabs */}
      <Dialog open={createDialogOpen} onOpenChange={handleCloseDialog}>
        <DialogContent className="max-w-3xl max-h-[90vh] overflow-y-auto">
          <DialogHeader>
            <DialogTitle>{t('automation:createRuleTitle')}</DialogTitle>
            <DialogDescription>
              {t('automation:createRuleDesc')}
            </DialogDescription>
          </DialogHeader>

          <Tabs value={createMode} onValueChange={(v) => setCreateMode(v as CreateMode)} className="mt-4">
            <TabsList className="grid w-full grid-cols-3">
              <TabsTrigger value="manual">{t('automation:manualCreate')}</TabsTrigger>
              <TabsTrigger value="template">{t('automation:fromTemplate')}</TabsTrigger>
              <TabsTrigger value="generate">{t('automation:aiGenerate')}</TabsTrigger>
            </TabsList>

            {/* Manual Create */}
            <TabsContent value="manual" className="space-y-4 mt-4">
              <div>
                <Label htmlFor="rule-name">{t('automation:ruleName')}</Label>
                <Input
                  id="rule-name"
                  value={newRuleName}
                  onChange={(e) => setNewRuleName(e.target.value)}
                  placeholder={t('automation:ruleNamePlaceholder')}
                />
              </div>
              <div>
                <Label htmlFor="rule-dsl">{t('automation:ruleDSL')}</Label>
                <Textarea
                  id="rule-dsl"
                  value={newRuleDSL}
                  onChange={(e) => setNewRuleDSL(e.target.value)}
                  placeholder={t('automation:ruleDSLPlaceholder')}
                  className="min-h-[120px] font-mono text-sm"
                />
              </div>
              {validation && (
                <div className={validation.valid ? "text-green-600 text-sm" : "text-red-600 text-sm"}>
                  {validation.valid ? t('automation:dslValid') : `✗ ${t('automation:dslInvalid')}: ${validation.errors?.join(', ')}`}
                </div>
              )}
            </TabsContent>

            {/* Template Create */}
            <TabsContent value="template" className="space-y-4 mt-4">
              {!selectedTemplate ? (
                <div>
                  <Label>{t('automation:selectTemplate')}</Label>
                  <div className="grid grid-cols-1 md:grid-cols-2 gap-3 mt-2">
                    {templates.length === 0 ? (
                      <div className="col-span-2 text-center py-4 text-muted-foreground text-sm">
                        {t('automation:noTemplates')} <Button variant="link" className="p-0 h-auto" onClick={handleLoadTemplates}>{t('automation:loadTemplates')}</Button>
                      </div>
                    ) : (
                      templates.map((template) => (
                        <Card
                          key={template.id}
                          className="p-3 cursor-pointer hover:bg-accent/50 transition-colors"
                          onClick={() => handleSelectTemplate(template)}
                        >
                          <div className="font-medium">{template.name}</div>
                          <div className="text-xs text-muted-foreground mt-1">{template.description}</div>
                          <Badge variant="outline" className="mt-2 text-xs">{template.category}</Badge>
                        </Card>
                      ))
                    )}
                    {templates.length === 0 && (
                      <div className="col-span-2">
                        <Button variant="outline" className="w-full" onClick={handleLoadTemplates}>
                          {t('automation:loadTemplates')}
                        </Button>
                      </div>
                    )}
                  </div>
                </div>
              ) : (
                <div className="space-y-4">
                  <div className="flex items-center justify-between">
                    <div>
                      <h3 className="font-medium">{selectedTemplate.name}</h3>
                      <p className="text-sm text-muted-foreground">{selectedTemplate.description}</p>
                    </div>
                    <Button variant="ghost" size="sm" onClick={() => setSelectedTemplate(null)}>
                      {t('automation:back')}
                    </Button>
                  </div>
                  {selectedTemplate.parameters.map((param) => (
                    <div key={param.name}>
                      <Label>
                        {param.label}
                        {param.required && <span className="text-destructive ml-1">*</span>}
                      </Label>
                      <Input
                        value={templateParams[param.name] || param.default || ''}
                        onChange={(e) => setTemplateParams({ ...templateParams, [param.name]: e.target.value })}
                        placeholder={param.default || t('automation:enterValue')}
                      />
                    </div>
                  ))}
                  <div className="flex gap-2">
                    <Button onClick={handleFillTemplate} variant="outline">
                      {t('automation:preview')}
                    </Button>
                    {templatePreview && (
                      <Button onClick={handleUseTemplate}>
                        {t('automation:useTemplate')}
                      </Button>
                    )}
                  </div>
                  {templatePreview && (
                    <div>
                      <Label>{t('automation:previewDSL')}</Label>
                      <pre className="bg-muted p-3 rounded-md text-xs font-mono overflow-x-auto">
                        {templatePreview}
                      </pre>
                    </div>
                  )}
                </div>
              )}
            </TabsContent>

            {/* AI Generate */}
            <TabsContent value="generate" className="space-y-4 mt-4">
              <div>
                <Label htmlFor="description">{t('automation:ruleDescription')}</Label>
                <Textarea
                  id="description"
                  value={description}
                  onChange={(e) => setDescription(e.target.value)}
                  placeholder={t('automation:descriptionPlaceholder')}
                  className="min-h-[80px]"
                />
              </div>
              <Button onClick={handleGenerate} disabled={!description.trim() || generating}>
                {generating ? (
                  <>
                    <Loader2 className="h-4 w-4 mr-2 animate-spin" />
                    {t('automation:generating')}
                  </>
                ) : (
                  <>
                    <Wand2 className="h-4 w-4 mr-2" />
                    {t('automation:generateRule')}
                  </>
                )}
              </Button>
              {generatedResult && (
                <div className="space-y-3">
                  <div>
                    <Label>{t('automation:generatedDSL')}</Label>
                    <Textarea
                      value={generatedResult.dsl}
                      onChange={(e) => setNewRuleDSL(e.target.value)}
                      className="min-h-[120px] font-mono text-sm"
                    />
                  </div>
                  <div className="text-sm text-muted-foreground">
                    <strong>{t('automation:explanation')}:</strong> {generatedResult.explanation}
                  </div>
                  {generatedResult.warnings.length > 0 && (
                    <div className="text-sm text-yellow-600">
                      <strong>{t('automation:warnings')}:</strong> {generatedResult.warnings.join(', ')}
                    </div>
                  )}
                  <Button onClick={() => { setNewRuleDSL(generatedResult.dsl); setCreateMode('manual') }}>
                    {t('automation:editGenerated')}
                  </Button>
                </div>
              )}
            </TabsContent>
          </Tabs>

          <DialogFooter>
            <Button variant="outline" onClick={handleCloseDialog}>
              {t('automation:cancel')}
            </Button>
            {createMode === 'manual' && (
              <>
                <Button variant="outline" onClick={handleValidate} disabled={validating || !newRuleDSL.trim()}>
                  {validating ? t('automation:validating') : t('automation:validateDSL')}
                </Button>
                <Button onClick={handleCreateRule} disabled={!newRuleName.trim() || !newRuleDSL.trim()}>
                  {t('automation:saveRule')}
                </Button>
              </>
            )}
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* Edit Rule Dialog */}
      <Dialog open={!!editRule} onOpenChange={(open) => !open && setEditRule(null)}>
        <DialogContent className="max-w-2xl">
          <DialogHeader>
            <DialogTitle>{t('automation:editRuleTitle')}</DialogTitle>
            <DialogDescription>
              {t('automation:editRuleDesc')}
            </DialogDescription>
          </DialogHeader>
          {editRule && (
            <div className="space-y-4">
              <div>
                <Label htmlFor="edit-rule-name">{t('automation:ruleName')}</Label>
                <Input
                  id="edit-rule-name"
                  value={editRule.name}
                  onChange={(e) => setEditRule({ ...editRule, name: e.target.value })}
                />
              </div>
              <div>
                <Label htmlFor="edit-rule-dsl">{t('automation:ruleDSL')}</Label>
                <Textarea
                  id="edit-rule-dsl"
                  value={editRule.dsl}
                  onChange={(e) => setEditRule({ ...editRule, dsl: e.target.value })}
                  className="min-h-[120px] font-mono text-sm"
                />
              </div>
            </div>
          )}
          <DialogFooter>
            <Button variant="outline" onClick={() => setEditRule(null)}>
              {t('automation:cancel')}
            </Button>
            <Button onClick={handleEditRule}>
              {t('automation:saveChanges')}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </>
  )
}
