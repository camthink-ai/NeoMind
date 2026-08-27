import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useErrorHandler } from '@/hooks/useErrorHandler'
import {
  Trash2,
  Plus,
  Loader2,
  AlertTriangle,
  ShieldCheck,
  Lock,
  Server,
  Download,
  Zap,
  FileText,
  ArrowLeft,
  CheckCircle2,
  Radar,
} from 'lucide-react'
import { UnifiedFormDialog } from '@/components/dialog/UnifiedFormDialog'
import { FormField } from '@/components/ui/field'
import { FormSection, FormSectionGroup } from '@/components/ui/form-section'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { PasswordInput } from '@/components/ui/password-input'
import { Switch } from '@/components/ui/switch'
import { Badge } from '@/components/ui/badge'
import { Textarea } from '@/components/ui/textarea'
import { api } from '@/lib/api'
import { useToast } from '@/hooks/use-toast'
import { confirm } from '@/hooks/use-confirm'

interface BrokerConfig {
  listen: string
  port: number
  max_connections: number
  auth_enabled: boolean
  credentials: { username: string; password: string }[]
  tls_enabled: boolean
  tls_cert_path: string | null
  tls_key_path: string | null
  tls_ca_path: string | null
  device_id_field?: string | null
}

type CertMode = 'auto' | 'manual' | null

interface Props {
  open: boolean
  onOpenChange: (open: boolean) => void
  onConfigSaved?: () => void
}

export function EmbeddedBrokerConfigDialog({ open, onOpenChange, onConfigSaved }: Props) {
  const { t } = useTranslation('settings')
  const { toast } = useToast()
  const { handleError } = useErrorHandler()

  const [config, setConfig] = useState<BrokerConfig | null>(null)
  const [saving, setSaving] = useState(false)

  // Form state for general settings
  const [listen, setListen] = useState('0.0.0.0')
  const [port, setPort] = useState(1883)
  const [authEnabled, setAuthEnabled] = useState(false)
  const [tlsEnabled, setTlsEnabled] = useState(false)
  const [deviceIdField, setDeviceIdField] = useState('')

  // Form state for TLS certificates
  const [certPem, setCertPem] = useState('')
  const [keyPem, setKeyPem] = useState('')
  const [caPem, setCaPem] = useState('')

  // TLS cert mode and generation state
  const [certMode, setCertMode] = useState<CertMode>(null)
  const [generating, setGenerating] = useState(false)

  // Form state for adding credential
  const [showAddCredential, setShowAddCredential] = useState(false)
  const [newUsername, setNewUsername] = useState('')
  const [newPassword, setNewPassword] = useState('')
  const [addingCredential, setAddingCredential] = useState(false)

  // Track if config has unsaved changes that require restart
  const [hasUnsavedChanges, setHasUnsavedChanges] = useState(false)

  const certsExist = !!config?.tls_cert_path

  useEffect(() => {
    if (open) {
      loadConfig()
    }
  }, [open])

  const loadConfig = async (preserveUnsaved = false) => {
    try {
      const data = await api.getEmbeddedBrokerConfig()
      setConfig(data)
      if (!preserveUnsaved) {
        setListen(data.listen || '0.0.0.0')
        setPort(data.port || 1883)
        setAuthEnabled(data.auth_enabled ?? false)
        setTlsEnabled(data.tls_enabled ?? false)
        setDeviceIdField(data.device_id_field ?? '')
        setCertPem('')
        setKeyPem('')
        setCaPem('')
        setCertMode(null)
        setHasUnsavedChanges(false)
      }
    } catch (error) {
      handleError(error)
    }
  }

  const handleToggleAuth = (enabled: boolean) => {
    setAuthEnabled(enabled)
    setHasUnsavedChanges(true)
  }

  const handleToggleTls = (enabled: boolean) => {
    setTlsEnabled(enabled)
    setCertMode(null)
    setHasUnsavedChanges(true)
  }

  const handleGenerateCert = async () => {
    setGenerating(true)
    try {
      await api.generateMqttTlsCert()
      await loadConfig(true)
      setHasUnsavedChanges(true)
      toast({
        title: t('broker.certGenerated'),
        description: t('broker.certGeneratedSuccess'),
      })
    } catch (error) {
      handleError(error)
    } finally {
      setGenerating(false)
    }
  }

  const handleDownloadCaCert = async () => {
    try {
      await api.downloadMqttCaCert()
      toast({
        title: t('broker.caCertDownloaded'),
        description: t('broker.caCertDownloadSuccess'),
      })
    } catch (error) {
      handleError(error)
    }
  }

  const handleAddCredential = async () => {
    if (!newUsername.trim() || !newPassword.trim()) {
      toast({
        title: t('broker.invalidInput'),
        description: t('broker.usernamePasswordRequired'),
        variant: 'destructive',
      })
      return
    }

    setAddingCredential(true)
    try {
      await api.addMqttCredential(newUsername.trim(), newPassword)
      await loadConfig(true)
      setNewUsername('')
      setNewPassword('')
      setShowAddCredential(false)
      toast({
        title: t('broker.userAdded'),
        description: t('broker.userAddSuccess', { username: newUsername }),
      })
    } catch (error) {
      handleError(error)
    } finally {
      setAddingCredential(false)
    }
  }

  const handleDeleteCredential = async (username: string) => {
    // Credentials are security-sensitive and unrecoverable — require an
    // explicit destructive confirm instead of deleting on first click.
    const confirmed = await confirm({
      title: t('broker.deleteCredentialTitle', { username }),
      description: t('broker.deleteCredentialConfirm', { username }),
      confirmText: t('common:delete'),
      cancelText: t('common:cancel'),
      variant: "destructive",
    })
    if (!confirmed) return
    try {
      await api.deleteMqttCredential(username)
      await loadConfig(true)
      toast({
        title: t('broker.userDeleted'),
        description: t('broker.userDeleteSuccess', { username }),
      })
    } catch (error) {
      handleError(error)
    }
  }

  const handleSave = async () => {
    if (!config) return
    setSaving(true)
    try {
      const result = await api.updateEmbeddedBrokerConfig({
        listen,
        port,
        auth_enabled: authEnabled,
        tls_enabled: tlsEnabled,
        // 空串 = 清空(后端 trim 判空回 None);null 才是"不修改"
        device_id_field: deviceIdField.trim(),
      })

      // Only upload manual certs if in manual mode and fields are filled
      if (certMode === 'manual' && certPem && keyPem) {
        await api.uploadMqttTlsCert(certPem, keyPem, caPem || undefined)
      }

      await loadConfig()
      setHasUnsavedChanges(false)

      // Notify parent to refresh broker status
      onConfigSaved?.()

      if (result.restart_required) {
        onOpenChange(false)
        toast({
          title: t('broker.configSaved'),
          description: t('broker.restartRequired'),
        })
      } else {
        onOpenChange(false)
        toast({
          title: t('broker.configSaved'),
        })
      }
    } catch (error) {
      handleError(error)
    } finally {
      setSaving(false)
    }
  }

  return (
    <UnifiedFormDialog
      open={open}
      onOpenChange={onOpenChange}
      title={t('broker.settings')}
      description={t('broker.settingsDescription')}
      icon={<Server className="h-5 w-5 text-muted-foreground" />}
      width="lg"
      /* Settings dialog is a z-[100] full-screen layer — a default z-50
         form dialog opens UNDERNEATH it and is invisible. z-[110] is the
         spec for nested dialogs above full-screen layers. */
      className="z-[110]"
      loading={!config}
      isSubmitting={saving}
      onSubmit={handleSave}
      submitLabel={t('broker.save')}
      cancelLabel={t('broker.cancel')}
      submitDisabled={!config || (tlsEnabled && !certsExist && certMode !== 'auto' && !(certMode === 'manual' && !!certPem && !!keyPem))}
    >
      <FormSectionGroup>
        {/* Auto-Discovery (read-only status) */}
        <FormSection title={t('broker.autoDiscovery')} defaultExpanded>
          <div className="space-y-3">
            <div className="flex items-start gap-3 p-3 rounded-lg border bg-success-light">
              <Radar className="h-5 w-5 text-success shrink-0 mt-0.5" />
              <div className="flex-1 space-y-2">
                <div className="flex items-center gap-2 flex-wrap">
                  <span className="text-sm font-medium">
                    {t('broker.autoDiscovery')}
                  </span>
                  <Badge variant="default" className="bg-success text-primary-foreground">
                    <CheckCircle2 className="h-3 w-3 mr-1" />
                    {t('broker.autoDiscoveryEnabled')}
                  </Badge>
                </div>
                <p className="text-xs text-muted-foreground">
                  {t('broker.autoDiscoveryDescription')}
                </p>
                <div className="flex items-center gap-2 text-xs">
                  <span className="text-muted-foreground">
                    {t('broker.autoDiscoveryListening')}:
                  </span>
                  <code className="px-1.5 py-0.5 rounded bg-muted-30 font-mono">
                    {t('broker.autoDiscoveryAllTopics')}
                  </code>
                </div>
              </div>
            </div>
            <p className="text-xs text-muted-foreground flex items-start gap-1.5">
              <ArrowLeft className="h-3.5 w-3.5 shrink-0 mt-0.5 rotate-180" />
              {t('broker.autoDiscoveryHint')}
            </p>
          </div>
        </FormSection>

        {/* General Settings */}
        <FormSection title={t('broker.general')} defaultExpanded>
          <div className="space-y-4">
            <FormField label={t('broker.listen')} helpText="0.0.0.0">
              <Input
                value={listen}
                onChange={(e) => {
                  setListen(e.target.value)
                  setHasUnsavedChanges(true)
                }}
                placeholder="0.0.0.0"
              />
            </FormField>
            <FormField
              label={t('broker.port')}
              helpText={`${t('broker.maxConnections')}: ${config?.max_connections ?? '-'}`}
            >
              <Input
                type="number"
                min={1024}
                max={65535}
                value={port}
                onChange={(e) => {
                  setPort(Number(e.target.value))
                  setHasUnsavedChanges(true)
                }}
              />
            </FormField>
            <FormField
              label={t('broker.deviceIdField')}
              helpText={t('broker.deviceIdFieldHelp')}
            >
              <Textarea
                rows={2}
                value={deviceIdField}
                onChange={(e) => {
                  setDeviceIdField(e.target.value)
                  setHasUnsavedChanges(true)
                }}
                placeholder={'device_id\nsn\nmac'}
              />
            </FormField>
          </div>
        </FormSection>

        {/* Authentication */}
        <FormSection title={t('broker.auth')} defaultExpanded>
          <div className="space-y-4">
            <div className="flex items-center justify-between gap-4">
              <div className="flex-1">
                <div className="text-sm font-medium flex items-center gap-2">
                  <ShieldCheck className="h-4 w-4 text-muted-foreground" />
                  {t('broker.authEnabled')}
                </div>
                <p className="text-xs text-muted-foreground mt-0.5">
                  {t('broker.authDescription')}
                </p>
              </div>
              <Switch
                checked={authEnabled}
                onCheckedChange={handleToggleAuth}
              />
            </div>

            {authEnabled && config && (
              <div className="space-y-2">
                {config.credentials.length === 0 && (
                  <p className="text-xs text-muted-foreground py-2">
                    {t('broker.noCredentials')}
                  </p>
                )}
                {config.credentials.map((cred) => (
                  <div
                    key={cred.username}
                    className="flex items-center justify-between p-2 rounded border bg-muted-30"
                  >
                    <div className="flex items-center gap-2">
                      <span className="text-sm font-mono">{cred.username}</span>
                      <Badge variant="secondary" className="text-xs">
                        &bull;&bull;&bull;&bull;&bull;&bull;&bull;&bull;
                      </Badge>
                    </div>
                    <Button
                      variant="ghost"
                      size="icon"
                      className="h-8 w-8 text-error hover:text-error"
                      onClick={() => handleDeleteCredential(cred.username)}
                      aria-label={t('broker.deleteUser')}
                    >
                      <Trash2 className="h-4 w-4" />
                    </Button>
                  </div>
                ))}

                {showAddCredential ? (
                  <div className="space-y-2 p-3 rounded border bg-background">
                    <FormField label={t('broker.username')}>
                      <Input
                        placeholder={t('broker.username')}
                        value={newUsername}
                        onChange={(e) => setNewUsername(e.target.value)}
                        onKeyDown={(e) => {
                          if (e.key === 'Enter') handleAddCredential()
                        }}
                      />
                    </FormField>
                    <FormField label={t('broker.password')}>
                      <PasswordInput
                        placeholder={t('broker.password')}
                        value={newPassword}
                        onChange={(e) => setNewPassword(e.target.value)}
                        onKeyDown={(e) => {
                          if (e.key === 'Enter') handleAddCredential()
                        }}
                      />
                    </FormField>
                    <div className="flex gap-2">
                      <Button
                        size="sm"
                        onClick={handleAddCredential}
                        disabled={addingCredential}
                      >
                        {addingCredential && <Loader2 className="h-4 w-4 mr-2 animate-spin" />}
                        {t('broker.add')}
                      </Button>
                      <Button
                        size="sm"
                        variant="ghost"
                        onClick={() => {
                          setShowAddCredential(false)
                          setNewUsername('')
                          setNewPassword('')
                        }}
                      >
                        {t('broker.cancel')}
                      </Button>
                    </div>
                  </div>
                ) : (
                  <Button
                    variant="outline"
                    size="sm"
                    className="w-full"
                    onClick={() => setShowAddCredential(true)}
                  >
                    <Plus className="h-4 w-4 mr-2" />
                    {t('broker.addUser')}
                  </Button>
                )}
              </div>
            )}
          </div>
        </FormSection>

        {/* TLS */}
        <FormSection title={t('broker.tls')} defaultExpanded>
          <div className="space-y-4">
            <div className="flex items-center justify-between gap-4">
              <div className="flex-1">
                <div className="text-sm font-medium flex items-center gap-2">
                  <Lock className="h-4 w-4 text-muted-foreground" />
                  {t('broker.tlsEnabled')}
                </div>
                <p className="text-xs text-muted-foreground mt-0.5">
                  {t('broker.tlsDescription')}
                </p>
              </div>
              <Switch
                checked={tlsEnabled}
                onCheckedChange={handleToggleTls}
              />
            </div>

            {tlsEnabled && (
              <>
                {certsExist ? (
                  /* Certs already configured — show status + download */
                  <div className="space-y-3">
                    <div className="flex items-center gap-2 p-3 rounded border bg-success-light">
                      <CheckCircle2 className="h-4 w-4 text-success shrink-0" />
                      <div className="flex-1">
                        <p className="text-sm font-medium text-success">
                          {t('broker.certsConfigured')}
                        </p>
                        <p className="text-xs text-muted-foreground mt-0.5">
                          {config.tls_cert_path}
                        </p>
                      </div>
                    </div>
                    {config.tls_ca_path && (
                      <Button
                        variant="outline"
                        size="sm"
                        className="w-full"
                        onClick={handleDownloadCaCert}
                      >
                        <Download className="h-4 w-4 mr-2" />
                        {t('broker.downloadCaCert')}
                      </Button>
                    )}
                  </div>
                ) : certMode === null ? (
                  /* No certs, no mode selected — show choice cards */
                  <div className="space-y-3">
                    <p className="text-sm text-muted-foreground">
                      {t('broker.noCertsDescription')}
                    </p>
                    <div className="grid grid-cols-2 gap-3">
                      <button
                        type="button"
                        onClick={() => setCertMode('auto')}
                        className="flex flex-col items-start gap-1.5 p-4 rounded-lg border text-left transition-colors hover:border-primary hover:bg-primary-lightHover"
                      >
                        <Zap className="h-5 w-5 text-muted-foreground" />
                        <span className="text-sm font-medium">{t('broker.autoGenerate')}</span>
                        <span className="text-xs text-muted-foreground">
                          {t('broker.autoGenerateDescription')}
                        </span>
                      </button>
                      <button
                        type="button"
                        onClick={() => setCertMode('manual')}
                        className="flex flex-col items-start gap-1.5 p-4 rounded-lg border text-left transition-colors hover:border-primary hover:bg-primary-lightHover"
                      >
                        <FileText className="h-5 w-5 text-muted-foreground" />
                        <span className="text-sm font-medium">{t('broker.manualUpload')}</span>
                        <span className="text-xs text-muted-foreground">
                          {t('broker.manualUploadDescription')}
                        </span>
                      </button>
                    </div>
                  </div>
                ) : certMode === 'auto' ? (
                  /* Auto-generate selected */
                  <div className="space-y-3">
                    <p className="text-sm text-muted-foreground">
                      {t('broker.autoGenerateInfo')}
                    </p>
                    <Button
                      onClick={handleGenerateCert}
                      disabled={generating}
                      className="w-full"
                    >
                      {generating && <Loader2 className="h-4 w-4 mr-2 animate-spin" />}
                      <Zap className="h-4 w-4 mr-2" />
                      {t('broker.generateCert')}
                    </Button>
                    <Button
                      variant="ghost"
                      size="sm"
                      className="w-full"
                      onClick={() => setCertMode(null)}
                    >
                      <ArrowLeft className="h-4 w-4 mr-2" />
                      {t('broker.back')}
                    </Button>
                  </div>
                ) : (
                  /* Manual upload selected */
                  <div className="space-y-4">
                    <FormField label={t('broker.serverCert')}>
                      <Textarea
                        placeholder="-----BEGIN CERTIFICATE-----&#10;...&#10;-----END CERTIFICATE-----"
                        value={certPem}
                        onChange={(e) => {
                          setCertPem(e.target.value)
                          setHasUnsavedChanges(true)
                        }}
                        rows={4}
                        className="font-mono text-xs"
                      />
                    </FormField>
                    <FormField label={t('broker.privateKey')}>
                      <Textarea
                        placeholder="-----BEGIN PRIVATE KEY-----&#10;...&#10;-----END PRIVATE KEY-----"
                        value={keyPem}
                        onChange={(e) => {
                          setKeyPem(e.target.value)
                          setHasUnsavedChanges(true)
                        }}
                        rows={4}
                        className="font-mono text-xs"
                      />
                    </FormField>
                    <FormField label={`${t('broker.caCert')} (${t('broker.optional')})`}>
                      <Textarea
                        placeholder="-----BEGIN CERTIFICATE-----&#10;...&#10;-----END CERTIFICATE-----"
                        value={caPem}
                        onChange={(e) => {
                          setCaPem(e.target.value)
                          setHasUnsavedChanges(true)
                        }}
                        rows={4}
                        className="font-mono text-xs"
                      />
                    </FormField>
                    <Button
                      variant="ghost"
                      size="sm"
                      className="w-full"
                      onClick={() => setCertMode(null)}
                    >
                      <ArrowLeft className="h-4 w-4 mr-2" />
                      {t('broker.back')}
                    </Button>
                  </div>
                )}
              </>
            )}
          </div>
        </FormSection>
      </FormSectionGroup>

      {/* Restart warning */}
      {hasUnsavedChanges && (
        <div className="flex items-start gap-2 p-3 rounded bg-warning-light text-warning mt-4">
          <AlertTriangle className="h-4 w-4 shrink-0 mt-0.5" />
          <p className="text-sm">{t('broker.restartWarning')}</p>
        </div>
      )}
    </UnifiedFormDialog>
  )
}
