import { useState, useRef } from "react"
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import { Button } from "@/components/ui/button"
import { useToast } from "@/hooks/use-toast"
import { useStore } from "@/store"
import { Loader2, Package, File } from "lucide-react"
import { useTranslation } from "react-i18next"
import { Progress } from "@/components/ui/progress"

interface ExtensionUploadDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  onUploadComplete?: (extensionId: string) => void
}

interface UploadProgress {
  filename: string
  loaded: number
  total: number
  status: 'idle' | 'uploading' | 'processing' | 'error'
  message?: string
  extensionId?: string
}

export function ExtensionUploadDialog({
  open,
  onOpenChange,
  onUploadComplete,
}: ExtensionUploadDialogProps) {
  const { t } = useTranslation(["extensions", "common"])
  const { toast } = useToast()
  const fetchExtensions = useStore((state) => state.fetchExtensions)
  const isAuthenticated = useStore((state) => state.isAuthenticated)

  const [uploading, setUploading] = useState(false)
  const [progress, setProgress] = useState<UploadProgress | null>(null)
  const fileInputRef = useRef<HTMLInputElement>(null)

  const handleFileSelect = () => {
    fileInputRef.current?.click()
  }

  const handleFileInputChange = async (e: React.ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0]
    if (!file) return

    // Check authentication status before upload
    if (!isAuthenticated) {
      toast({
        title: t('extensions:authRequired'),
        description: t('extensions:authRequiredDescription'),
        variant: 'destructive',
      })
      return
    }

    // Check file extension
    if (!file.name.endsWith('.nep') && !file.name.endsWith('.zip')) {
      toast({
        title: t('extensions:invalidFile'),
        description: t('extensions:invalidFileDescription'),
        variant: 'destructive',
      })
      return
    }

    // Set progress and start upload
    setProgress({
      filename: file.name,
      loaded: 0,
      total: file.size,
      status: 'uploading',
    })
    setUploading(true)

    let interval: ReturnType<typeof setInterval> | null = null
    let timeoutId: ReturnType<typeof setTimeout> | null = null
    let isCompleted = false

    // Helper function to handle errors consistently
    const handleError = (error: unknown) => {
      if (isCompleted) return
      isCompleted = true

      if (timeoutId) clearTimeout(timeoutId)
      if (interval) clearInterval(interval)

      // Handle specific error types
      let errorMessage = 'Upload failed'

      if (error instanceof Error) {
        if (error.name === 'AbortError' || error.message.includes('timeout')) {
          errorMessage = 'Upload timed out. The server may have crashed or is not responding.'
        } else if (error.message.includes('Failed to fetch') || error.message.includes('NetworkError')) {
          errorMessage = 'Network error. The connection was lost during upload.'
        } else if (error.message.includes('EPIPE')) {
          errorMessage = 'Connection closed unexpectedly. The server may have crashed.'
        } else {
          errorMessage = error.message
        }
      }

      setProgress({
        filename: file.name,
        loaded: 0,
        total: file.size,
        status: 'error',
        message: errorMessage,
      })
      setUploading(false)

      toast({
        title: t('extensions:installError'),
        description: errorMessage,
        variant: 'destructive',
      })
    }

    try {
      // Dynamically import api to avoid circular dependencies
      const { api } = await import('@/lib/api')

      // Simulate progress
      interval = setInterval(() => {
        setProgress(prev => {
          if (!prev || prev.status === 'error') {
            if (interval) clearInterval(interval)
            return prev
          }
          return {
            ...prev,
            loaded: Math.min(prev.loaded + prev.total / 10, prev.total),
          }
        })
      }, 100)

      // Create abort controller for timeout
      const controller = new AbortController()

      // Set timeout for upload (2 minutes for large files)
      const UPLOAD_TIMEOUT = 120000
      timeoutId = setTimeout(() => {
        if (!isCompleted) {
          controller.abort()
          handleError(new Error('Upload timeout'))
        }
      }, UPLOAD_TIMEOUT)

      // Wrap API call in Promise.race to handle cases where fetch hangs
      const uploadPromise = api.uploadExtensionFile(file, controller.signal)

      // Create a promise that rejects when aborted
      const abortPromise = new Promise<never>((_, reject) => {
        controller.signal.addEventListener('abort', () => {
          reject(new Error('Upload timeout'))
        })
      })

      // Race between upload and abort
      const result = await Promise.race([uploadPromise, abortPromise])

      if (timeoutId) clearTimeout(timeoutId)
      if (interval) clearInterval(interval)

      if (isCompleted) return
      isCompleted = true

      setProgress(prev => prev ? { ...prev, status: 'processing' } : null)

      let extensionId = ''
      if (result.extension_id) {
        extensionId = result.extension_id
      }

      toast({
        title: t('extensions:installSuccess'),
        description: result.name || file.name.replace('.nep', ''),
      })

      await fetchExtensions()

      // Sync extension components to dashboard registry
      try {
        const { syncExtensionComponents } = await import('@/hooks/useExtensionComponents')
        await syncExtensionComponents()
      } catch (e) {
        console.warn('Failed to sync extension components:', e)
      }

      // Close dialog immediately on success
      onOpenChange(false)
      resetForm()

      setUploading(false)
      onUploadComplete?.(extensionId || file.name)
    } catch (error) {
      handleError(error)
    }
  }

  const resetForm = () => {
    setProgress(null)
    if (fileInputRef.current) {
      fileInputRef.current.value = ''
    }
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-[500px] flex flex-col">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <Package className="h-5 w-5" />
            {t("extensions:uploadExtension")}
          </DialogTitle>
          <DialogDescription>
            {t("extensions:dragDropDescription")}
          </DialogDescription>
        </DialogHeader>

        <div className="space-y-4 py-4 flex-1 overflow-y-auto -mx-6 px-6">
          {/* File Upload */}
          <div
            className={`border-2 border-dashed rounded-lg p-6 text-center transition-colors ${
              progress?.status === 'error'
                ? 'border-destructive/50 bg-destructive/5 cursor-pointer hover:border-destructive/70'
                : 'cursor-pointer hover:border-primary/50'
            }`}
            onClick={() => {
              // Allow clicking to retry when there's an error
              if (progress?.status === 'error') {
                resetForm()
              }
              handleFileSelect()
            }}
          >
            <input
              ref={fileInputRef}
              type="file"
              accept=".nep,.zip"
              className="hidden"
              onChange={handleFileInputChange}
              disabled={uploading}
            />
            {progress && (uploading || progress.status === 'error') ? (
              <div className="space-y-3">
                {progress.status === 'error' ? (
                  // Error state - show error with retry option
                  <>
                    <div className="flex items-center justify-center gap-2 text-destructive">
                      <svg className="h-5 w-5" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                        <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M12 8v4m0 4h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z" />
                      </svg>
                      <span className="text-sm font-medium">{t('extensions:installFailed')}</span>
                    </div>
                    <p className="text-sm text-muted-foreground">{progress.filename}</p>
                    <div className="text-destructive/80 text-xs bg-destructive/10 rounded p-2">
                      {progress.message || t('extensions:installFailed')}
                    </div>
                    <p className="text-xs text-muted-foreground">
                      {t('extensions:clickToRetry')}
                    </p>
                  </>
                ) : (
                  // Uploading/Processing state
                  <>
                    <div className="flex items-center justify-center gap-2">
                      <Loader2 className="h-4 w-4 animate-spin" />
                      <span className="text-sm text-muted-foreground">
                        {progress.status === 'processing'
                          ? t('extensions:processing')
                          : t('extensions:uploading')}
                      </span>
                    </div>
                    <Progress value={(progress.loaded / progress.total) * 100} />
                    <p className="text-sm text-muted-foreground">{progress.filename}</p>
                  </>
                )}
              </div>
            ) : (
              <div className="space-y-2">
                <Package className="h-12 w-12 mx-auto text-muted-foreground" />
                <p className="text-sm font-medium">{t('extensions:dragDrop')}</p>
                <p className="text-xs text-muted-foreground">
                  {t('extensions:dragDropDescription')}
                </p>
              </div>
            )}
          </div>
          <p className="text-xs text-muted-foreground text-center">
            {t('extensions:supportedFormats')}: .nep, .zip
          </p>
        </div>

        <DialogFooter>
          <Button
            variant="outline"
            onClick={() => {
              onOpenChange(false)
              resetForm()
            }}
            disabled={uploading}
          >
            {t("common:cancel")}
          </Button>
          <Button onClick={handleFileSelect} disabled={uploading}>
            {uploading ? (
              <>
                <Loader2 className="mr-2 h-4 w-4 animate-spin" />
                {t('extensions:installing')}
              </>
            ) : (
              <>
                <Package className="mr-2 h-4 w-4" />
                {t('extensions:uploadAndInstall')}
              </>
            )}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}