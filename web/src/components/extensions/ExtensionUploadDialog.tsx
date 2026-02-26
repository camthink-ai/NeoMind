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
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import { Switch } from "@/components/ui/switch"
import { useToast } from "@/hooks/use-toast"
import { useStore } from "@/store"
import { Upload, Loader2, FolderOpen, Package, File, X } from "lucide-react"
import { useTranslation } from "react-i18next"
import { api } from "@/lib/api"
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
  status: 'idle' | 'uploading' | 'processing' | 'success' | 'error'
  message?: string
}

export function ExtensionUploadDialog({
  open,
  onOpenChange,
  onUploadComplete,
}: ExtensionUploadDialogProps) {
  const { t } = useTranslation(["extensions", "common"])
  const { toast } = useToast()
  const registerExtension = useStore((state) => state.registerExtension)
  const fetchExtensions = useStore((state) => state.fetchExtensions)

  const [filePath, setFilePath] = useState("")
  const [autoStart, setAutoStart] = useState(false)
  const [uploading, setUploading] = useState(false)
  const [uploadMode, setUploadMode] = useState<'path' | 'file'>('path')
  const [progress, setProgress] = useState<UploadProgress | null>(null)
  const fileInputRef = useRef<HTMLInputElement>(null)

  const handleFileSelect = () => {
    fileInputRef.current?.click()
  }

  const handleFileInputChange = async (e: React.ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0]
    if (!file) return

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

    // Declare interval outside try block so it's accessible in catch
    let interval: ReturnType<typeof setInterval> | null = null

    try {
      // Simulate progress
      interval = setInterval(() => {
        setProgress(prev => {
          if (!prev || prev.status === 'success' || prev.status === 'error') {
            if (interval) clearInterval(interval)
            return prev
          }
          return {
            ...prev,
            loaded: Math.min(prev.loaded + prev.total / 10, prev.total),
          }
        })
      }, 100)

      const isTauri = typeof window !== 'undefined' && !!(window as any).__TAURI__
      if (isTauri) {
        // Use direct file upload
        await api.uploadExtensionFile(file)
        setProgress(prev => prev ? { ...prev, status: 'processing' } : null)
      } else {
        // For web, need to use path method
        const arrayBuffer = await file.arrayBuffer()
        const tempDir = '/tmp'
        const fileName = `${Date.now()}-${file.name}`

        // In web, we can't save to filesystem directly
        // Show message for web users
        toast({
          title: t('extensions:webUploadNotSupported'),
          description: t('extensions:webUploadNotSupportedDesc'),
          variant: 'destructive',
        })
        if (interval) clearInterval(interval)
        setUploading(false)
        setProgress(null)
        return
      }

      if (interval) clearInterval(interval)

      setProgress({
        filename: file.name,
        loaded: file.size,
        total: file.size,
        status: 'success',
      })

      toast({
        title: t('extensions:installSuccess'),
        description: file.name.replace('.nep', ''),
      })

      await fetchExtensions()

      setTimeout(() => {
        onOpenChange(false)
        resetForm()
      }, 1500)

      onUploadComplete?.(file.name)
    } catch (error) {
      if (interval) clearInterval(interval)
      setProgress({
        filename: file.name,
        loaded: 0,
        total: file.size,
        status: 'error',
        message: error instanceof Error ? error.message : 'Upload failed',
      })
      toast({
        title: t('extensions:installError'),
        description: error instanceof Error ? error.message : 'Unknown error',
        variant: 'destructive',
      })
    } finally {
      setUploading(false)
    }
  }

  const handlePathInputChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0]
    if (file) {
      const filePath = (file as any).path || file.name
      setFilePath(filePath)
    }
  }

  const handleSubmit = async () => {
    if (uploadMode === 'file') {
      handleFileSelect()
      return
    }

    if (!filePath.trim()) {
      toast({
        title: t("extensionFile"),
        description: t("extensionPathLabel"),
        variant: "destructive",
      })
      return
    }

    setUploading(true)
    try {
      await registerExtension({
        file_path: filePath,
        auto_start: autoStart,
      })

      toast({
        title: t("registerSuccess"),
      })
      onUploadComplete?.(filePath)
      onOpenChange(false)
      resetForm()
    } catch (error) {
      toast({
        title: t("registerFailed"),
        description: error instanceof Error ? error.message : "Unknown error",
        variant: "destructive",
      })
    } finally {
      setUploading(false)
    }
  }

  const resetForm = () => {
    setFilePath("")
    setAutoStart(false)
    setProgress(null)
    if (fileInputRef.current) {
      fileInputRef.current.value = ''
    }
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-[500px]">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <Upload className="h-5 w-5" />
            {t("registerExtension")}
          </DialogTitle>
          <DialogDescription>
            {t("registerExtensionDesc")}
          </DialogDescription>
        </DialogHeader>

        {/* Upload Mode Toggle */}
        {(typeof window !== 'undefined' && !!(window as any).__TAURI__) && (
          <div className="flex gap-2 mb-4 p-1 bg-muted rounded-lg">
            <Button
              type="button"
              variant={uploadMode === 'path' ? 'default' : 'ghost'}
              size="sm"
              onClick={() => setUploadMode('path')}
            >
              <FolderOpen className="h-4 w-4 mr-1" />
              {t('extensions:pathMode')}
            </Button>
            <Button
              type="button"
              variant={uploadMode === 'file' ? 'default' : 'ghost'}
              size="sm"
              onClick={() => setUploadMode('file')}
            >
              <Package className="h-4 w-4 mr-1" />
              {t('extensions:fileMode')}
            </Button>
          </div>
        )}

        <div className="space-y-4 pt-4">
          {uploadMode === 'path' ? (
            <>
              {/* File Path Input */}
              <div className="space-y-2">
                <Label htmlFor="file-path">{t("extensionFile")}</Label>
                <div className="flex items-center gap-2">
                  <Input
                    id="file-path"
                    placeholder={t("extensionPathPlaceholder")}
                    value={filePath}
                    onChange={(e) => setFilePath(e.target.value)}
                    disabled={uploading}
                    className="font-mono text-sm"
                  />
                  <Button
                    type="button"
                    variant="outline"
                    size="icon"
                    onClick={handleFileSelect}
                    disabled={uploading}
                  >
                    <FolderOpen className="h-4 w-4" />
                  </Button>
                  <input
                    ref={fileInputRef}
                    type="file"
                    accept=".so,.dylib,.dll,.wasm"
                    className="hidden"
                    onChange={handlePathInputChange}
                  />
                </div>
                <p className="text-xs text-muted-foreground">
                  {t("extensionPathHint")}
                </p>
              </div>
            </>
          ) : (
            <>
              {/* File Upload */}
              <div
                className="border-2 border-dashed rounded-lg p-6 text-center cursor-pointer hover:border-primary/50 transition-colors"
                onClick={handleFileSelect}
              >
                <input
                  ref={fileInputRef}
                  type="file"
                  accept=".nep,.zip"
                  className="hidden"
                  onChange={handleFileInputChange}
                  disabled={uploading}
                />
                {uploading && progress ? (
                  <div className="space-y-3">
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
                    {progress.status === 'success' && (
                      <div className="flex items-center justify-center gap-2 text-green-600">
                        <File className="h-4 w-4" />
                        <span>{t('extensions:installComplete')}</span>
                      </div>
                    )}
                    {progress.status === 'error' && (
                      <div className="text-red-600 text-sm">
                        {progress.message || t('extensions:installFailed')}
                      </div>
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
            </>
          )}

          {/* Auto Start Switch */}
          <div className="flex items-center justify-between">
            <div className="space-y-0.5">
              <Label htmlFor="auto-start" className="cursor-pointer">
                {t("autoStart")}
              </Label>
              <p className="text-xs text-muted-foreground">
                {t("autoStartDesc")}
              </p>
            </div>
            <Switch
              id="auto-start"
              checked={autoStart}
              onCheckedChange={setAutoStart}
              disabled={uploading}
            />
          </div>
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
          <Button onClick={handleSubmit} disabled={uploading || (uploadMode === 'path' && !filePath.trim())}>
            {uploading ? (
              <>
                <Loader2 className="mr-2 h-4 w-4 animate-spin" />
                {t('extensions:installing')}
              </>
            ) : (
              <>
                <Upload className="mr-2 h-4 w-4" />
                {uploadMode === 'file'
                  ? t('extensions:uploadAndInstall')
                  : t('extensions:registerExtension')}
              </>
            )}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

