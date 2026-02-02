import { useState, useCallback } from "react"
import { useTranslation } from "react-i18next"
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import { Button } from "@/components/ui/button"
import { Label } from "@/components/ui/label"
import { Progress } from "@/components/ui/progress"
import { Upload, FileCode, X, CheckCircle, AlertCircle } from "lucide-react"
import { useToast } from "@/hooks/use-toast"

export interface PluginUploadDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  onUploadComplete?: (pluginId: string) => void
}

type UploadState = "idle" | "uploading" | "success" | "error"

interface UploadedFile {
  file: File
  state: UploadState
  progress: number
  pluginId?: string
  error?: string
}

export function PluginUploadDialog({
  open,
  onOpenChange,
  onUploadComplete,
}: PluginUploadDialogProps) {
  const { t } = useTranslation(["common", "plugins"])
  const { toast } = useToast()

  const [uploadedFiles, setUploadedFiles] = useState<UploadedFile[]>([])
  const [isUploading, setIsUploading] = useState(false)

  // Reset state when dialog opens/closes
  const handleOpenChange = useCallback(
    (newOpen: boolean) => {
      if (!newOpen) {
        setUploadedFiles([])
      }
      onOpenChange(newOpen)
    },
    [onOpenChange]
  )

  // Handle file selection
  const handleFileChange = useCallback(
    (e: React.ChangeEvent<HTMLInputElement>) => {
      const files = Array.from(e.target.files || [])

      // Filter for valid plugin file extensions
      const validExtensions = [".so", ".dylib", ".dll", ".wasm"]
      const validFiles = files.filter((file) => {
        const name = file.name.toLowerCase()
        return validExtensions.some((ext) => name.endsWith(ext))
      })

      if (validFiles.length === 0) {
        toast({
          title: t("plugins:invalidPluginFile"),
          description: t("plugins:supportedFormats"),
          variant: "destructive",
        })
        return
      }

      // Add files to the list
      const newUploads: UploadedFile[] = validFiles.map((file) => ({
        file,
        state: "idle",
        progress: 0,
      }))

      setUploadedFiles((prev) => [...prev, ...newUploads])
    },

    [toast, t]
  )

  // Remove a file from the list
  const handleRemoveFile = useCallback((index: number) => {
    setUploadedFiles((prev) => prev.filter((_, i) => i !== index))
  }, [])

  // Upload a single file
  const uploadFile = async (uploadedFile: UploadedFile): Promise<void> => {
    const { file } = uploadedFile

    // Update state to uploading
    setUploadedFiles((prev) =>
      prev.map((f) =>
        f.file === file
          ? { ...f, state: "uploading", progress: 0 }
          : f
      )
    )

    const formData = new FormData()
    formData.append("file", file)

    try {
      // Simulate progress
      const progressInterval = setInterval(() => {
        setUploadedFiles((prev) =>
          prev.map((f) =>
            f.file === file && f.state === "uploading"
              ? { ...f, progress: Math.min(f.progress + 10, 90) }
              : f
          )
        )
      }, 100)

      // Upload to the API
      // Use correct API base for Tauri environment
      const apiBase = (window as any).__TAURI__ ? 'http://localhost:3000/api' : '/api'
      const response = await fetch(`${apiBase}/plugins/upload`, {
        method: "POST",
        body: formData,
      })

      clearInterval(progressInterval)

      if (!response.ok) {
        const error = await response.json()
        throw new Error(error.message || "Upload failed")
      }

      const result = await response.json()

      // Update to success
      setUploadedFiles((prev) =>
        prev.map((f) =>
          f.file === file
            ? { ...f, state: "success", progress: 100, pluginId: result.plugin_id }
            : f
        )
      )

      toast({
        title: t("plugins:uploadSuccess", { name: file.name }),
      })

      if (onUploadComplete && result.plugin_id) {
        onUploadComplete(result.plugin_id)
      }
    } catch (error) {
      const errorMessage = error instanceof Error ? error.message : String(error)

      setUploadedFiles((prev) =>
        prev.map((f) =>
          f.file === file
            ? { ...f, state: "error", error: errorMessage }
            : f
        )
      )

      toast({
        title: t("plugins:uploadFailed", { name: file.name }),
        description: errorMessage,
        variant: "destructive",
      })
    }
  }

  // Upload all pending files
  const handleUpload = async () => {
    const pendingFiles = uploadedFiles.filter((f) => f.state === "idle")

    if (pendingFiles.length === 0) return

    setIsUploading(true)

    // Upload files sequentially
    for (const file of pendingFiles) {
      await uploadFile(file)
    }

    setIsUploading(false)
  }

  // Check if all uploads are complete
  const allComplete = uploadedFiles.length > 0 &&
    uploadedFiles.every((f) => f.state === "success" || f.state === "error")

  // Get file icon based on extension
  const getFileIcon = (filename: string) => {
    const ext = filename.toLowerCase().split(".").pop()
    return ext || "file"
  }

  // Get state icon
  const getStateIcon = (state: UploadState) => {
    switch (state) {
      case "success":
        return <CheckCircle className="h-5 w-5 text-success" />
      case "error":
        return <AlertCircle className="h-5 w-5 text-destructive" />
      case "uploading":
        return (
          <div className="h-5 w-5 border-2 border-primary border-t-transparent rounded-full animate-spin" />
        )
      default:
        return <FileCode className="h-5 w-5 text-muted-foreground" />
    }
  }

  return (
    <Dialog open={open} onOpenChange={handleOpenChange}>
      <DialogContent className="max-w-lg">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <Upload className="h-5 w-5" />
            {t("plugins:uploadPlugin")}
          </DialogTitle>
          <DialogDescription>
            {t("plugins:uploadDesc")}
          </DialogDescription>
        </DialogHeader>

        <div className="space-y-4 py-4">
          {/* File Drop Zone */}
          <div className="border-2 border-dashed border-border rounded-lg p-8 text-center hover:border-primary/50 transition-colors">
            <input
              type="file"
              id="plugin-file"
              className="hidden"
              accept=".so,.dylib,.dll,.wasm"
              multiple
              onChange={handleFileChange}
              disabled={isUploading}
            />
            <Label
              htmlFor="plugin-file"
              className="cursor-pointer flex flex-col items-center gap-2"
            >
              <Upload className="h-10 w-10 text-muted-foreground" />
              <span className="text-sm font-medium">
                {t("plugins:clickToUpload")}
              </span>
              <span className="text-xs text-muted-foreground">
                {t("plugins:supportedFormats")}
              </span>
            </Label>
          </div>

          {/* File List */}
          {uploadedFiles.length > 0 && (
            <div className="space-y-2">
              <Label className="text-sm text-muted-foreground">
                {t("plugins:selectedFiles")} ({uploadedFiles.length})
              </Label>
              {uploadedFiles.map((uploadedFile, index) => (
                <div
                  key={`${uploadedFile.file.name}-${index}`}
                  className="flex items-center gap-3 p-3 bg-muted/50 rounded-lg"
                >
                  <div className="flex-shrink-0">
                    {getStateIcon(uploadedFile.state)}
                  </div>
                  <div className="flex-1 min-w-0">
                    <p className="text-sm font-medium truncate">
                      {uploadedFile.file.name}
                    </p>
                    <p className="text-xs text-muted-foreground">
                      {(uploadedFile.file.size / 1024).toFixed(1)} KB • {getFileIcon(uploadedFile.file.name)}
                    </p>
                    {uploadedFile.state === "uploading" && (
                      <Progress value={uploadedFile.progress} className="h-1 mt-2" />
                    )}
                    {uploadedFile.state === "error" && (
                      <p className="text-xs text-destructive mt-1">
                        {uploadedFile.error}
                      </p>
                    )}
                  </div>
                  {uploadedFile.state === "idle" && (
                    <Button
                      variant="ghost"
                      size="sm"
                      className="flex-shrink-0"
                      onClick={() => handleRemoveFile(index)}
                    >
                      <X className="h-4 w-4" />
                    </Button>
                  )}
                </div>
              ))}
            </div>
          )}

          {/* Info text */}
          <div className="bg-muted/50 rounded-lg p-3 text-xs text-muted-foreground">
            <p className="font-medium mb-1">{t("plugins:uploadNoteTitle")}</p>
            <ul className="list-disc list-inside space-y-1 ml-2">
              <li>{t("plugins:uploadNote1")}</li>
              <li>{t("plugins:uploadNote2")}</li>
              <li>{t("plugins:uploadNote3")}</li>
            </ul>
          </div>
        </div>

        <DialogFooter>
          <Button
            variant="outline"
            onClick={() => handleOpenChange(false)}
            disabled={isUploading}
          >
            {allComplete ? t("common:close") : t("common:cancel")}
          </Button>
          <Button
            onClick={handleUpload}
            disabled={uploadedFiles.length === 0 || isUploading}
          >
            {isUploading ? t("plugins:uploading") : t("plugins:upload")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
