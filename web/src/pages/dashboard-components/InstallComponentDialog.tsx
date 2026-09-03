/**
 * Install Component Dialog
 *
 * Manual community-component import with two entry points:
 * 1. Upload — drag & drop (or click/keyboard) a .zip from this machine;
 *    the backend extracts manifest.json + bundle.js.
 * 2. Server path — for packages already on the NeoMind box (scp/USB),
 *    inside its data directory; mirrors the extensions file_path API.
 */

import { useState, useRef, useCallback } from 'react'
import { useTranslation } from 'react-i18next'
import { PackagePlus, FileArchive, Upload, Server } from 'lucide-react'
import { UnifiedFormDialog } from '@/components/dialog/UnifiedFormDialog'
import { Input } from '@/components/ui/input'
import { cn } from '@/lib/utils'
import { useStore } from '@/store'
import { notifySuccess, notifyFromError } from '@/lib/notify'

interface InstallComponentDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
}

type ImportMode = 'upload' | 'server-path'

export function InstallComponentDialog({ open, onOpenChange }: InstallComponentDialogProps) {
  const { t } = useTranslation('dashboardComponents')
  const inputRef = useRef<HTMLInputElement>(null)
  const { installManualZip, installFromPath } = useStore()

  const [mode, setMode] = useState<ImportMode>('upload')
  const [zipFile, setZipFile] = useState<File | null>(null)
  const [isDragOver, setIsDragOver] = useState(false)
  const [serverPath, setServerPath] = useState('')
  const [isSubmitting, setIsSubmitting] = useState(false)
  const [submitError, setSubmitError] = useState<string | null>(null)

  const acceptFile = useCallback((file: File | undefined | null) => {
    if (!file) return
    if (!file.name.toLowerCase().endsWith('.zip')) {
      setSubmitError(t('componentLibrary.invalidZipType'))
      return
    }
    setZipFile(file)
    setSubmitError(null)
  }, [t])

  const handleClick = () => inputRef.current?.click()

  const handleInputChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    acceptFile(e.target.files?.[0])
    // allow picking the same file again after clearing
    e.target.value = ''
  }

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'Enter' || e.key === ' ') {
      e.preventDefault()
      handleClick()
    }
  }

  const handleDrop = (e: React.DragEvent) => {
    e.preventDefault()
    setIsDragOver(false)
    acceptFile(e.dataTransfer.files?.[0])
  }

  const handleInstall = async () => {
    setIsSubmitting(true)
    setSubmitError(null)

    try {
      if (mode === 'upload') {
        if (!zipFile) return
        await installManualZip(zipFile)
      } else {
        const trimmed = serverPath.trim()
        if (!trimmed) return
        await installFromPath(trimmed)
      }
      notifySuccess(t('installSuccess'))
      resetState()
      onOpenChange(false)
    } catch (error) {
      setSubmitError(error instanceof Error ? error.message : t('installError'))
      notifyFromError(error, t('installError'))
    } finally {
      setIsSubmitting(false)
    }
  }

  const resetState = () => {
    setMode('upload')
    setZipFile(null)
    setServerPath('')
    setSubmitError(null)
  }

  const handleOpenChange = (newOpen: boolean) => {
    if (!newOpen) resetState()
    onOpenChange(newOpen)
  }

  const canInstall = !isSubmitting && (
    mode === 'upload' ? !!zipFile : serverPath.trim().length > 0
  )

  const modes: Array<{ id: ImportMode; icon: typeof Upload; label: string }> = [
    { id: 'upload', icon: Upload, label: t('componentLibrary.importTypeUpload') },
    { id: 'server-path', icon: Server, label: t('componentLibrary.importTypeServerPath') },
  ]

  return (
    <UnifiedFormDialog
      open={open}
      onOpenChange={handleOpenChange}
      title={t('componentLibrary.importTitle')}
      icon={<PackagePlus className="w-full h-full" />}
      width="lg"
      className="z-[110]"
      isSubmitting={isSubmitting}
      submitError={submitError}
      onSubmit={handleInstall}
      submitLabel={t('installConfirm')}
      submitDisabled={!canInstall}
    >
      <div className="space-y-6">
        {/* Import type selector */}
        <div role="tablist" aria-label={t('componentLibrary.importTitle')} className="grid grid-cols-2 gap-2 p-1 bg-muted rounded-lg">
          {modes.map(({ id, icon: Icon, label }) => (
            <button
              key={id}
              type="button"
              role="tab"
              aria-selected={mode === id}
              onClick={() => { setMode(id); setSubmitError(null) }}
              className={cn(
                'flex items-center justify-center gap-2 h-9 rounded-md text-sm font-medium transition-colors',
                mode === id
                  ? 'bg-background text-foreground shadow-sm'
                  : 'text-muted-foreground hover:text-foreground',
              )}
            >
              <Icon className="h-4 w-4" />
              {label}
            </button>
          ))}
        </div>

        {mode === 'upload' ? (
          <>
            {/* ZIP drop zone — drag & drop, click, or keyboard */}
            <div
              role="button"
              tabIndex={0}
              aria-label={t('componentLibrary.selectFile') + ' (.zip)'}
              onClick={handleClick}
              onKeyDown={handleKeyDown}
              onDragOver={(e) => { e.preventDefault(); setIsDragOver(true) }}
              onDragLeave={() => setIsDragOver(false)}
              onDrop={handleDrop}
              className={cn(
                'border-2 border-dashed rounded-lg p-8 text-center cursor-pointer transition-colors outline-none',
                'focus-visible:ring-2 focus-visible:ring-ring',
                zipFile
                  ? 'border-success bg-success-light'
                  : isDragOver
                    ? 'border-primary bg-muted'
                    : 'border-border hover:border-muted-foreground hover:bg-muted-30',
              )}
            >
              <input
                ref={inputRef}
                type="file"
                accept=".zip"
                onChange={handleInputChange}
                className="hidden"
              />
              {zipFile ? (
                <div className="space-y-1">
                  <FileArchive className="w-10 h-10 text-success mx-auto" />
                  <p className="text-sm font-medium text-foreground">{zipFile.name}</p>
                  <p className="text-xs text-muted-foreground">
                    {(zipFile.size / 1024).toFixed(1)} KB · {t('componentLibrary.replaceFileHint')}
                  </p>
                </div>
              ) : (
                <div className="space-y-2">
                  <Upload className={cn('w-10 h-10 mx-auto', isDragOver ? 'text-primary' : 'text-muted-foreground')} />
                  <p className="text-sm text-muted-foreground">
                    {isDragOver ? t('componentLibrary.dropHere') : t('componentLibrary.dropOrSelect')}
                  </p>
                </div>
              )}
            </div>

            <div className="p-4 bg-muted-30 rounded-lg border border-border">
              <ul className="text-xs text-muted-foreground space-y-1 list-disc list-inside">
                <li>{t('componentLibrary.zipPackageDesc')}</li>
              </ul>
            </div>
          </>
        ) : (
          <>
            <div className="space-y-2">
              <Input
                value={serverPath}
                onChange={(e) => { setServerPath(e.target.value); setSubmitError(null) }}
                placeholder={t('componentLibrary.serverPathPlaceholder')}
                className="h-9 font-mono text-sm"
                spellCheck={false}
                autoComplete="off"
              />
              <p className="text-xs text-muted-foreground leading-relaxed">
                {t('componentLibrary.serverPathHint')}
              </p>
            </div>
          </>
        )}
      </div>
    </UnifiedFormDialog>
  )
}
