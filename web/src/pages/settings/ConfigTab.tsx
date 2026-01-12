import { useState, useRef } from "react"
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card"
import { Button } from "@/components/ui/button"
import { Label } from "@/components/ui/label"
import { Checkbox } from "@/components/ui/checkbox"
import { ScrollArea } from "@/components/ui/scroll-area"
import { Download, Upload, FileText, AlertCircle, Loader2 } from "lucide-react"
import { api } from "@/lib/api"
import { useToast } from "@/hooks/use-toast"

export function ConfigTab() {
  const { toast } = useToast()
  const [exporting, setExporting] = useState(false)
  const [importing, setImporting] = useState(false)
  const [validating, setValidating] = useState(false)
  const [exportedConfig, setExportedConfig] = useState<Record<string, unknown> | null>(null)
  const [importPreview, setImportPreview] = useState<Record<string, unknown> | null>(null)
  const [validationErrors, setValidationErrors] = useState<string[]>([])
  const [mergeMode, setMergeMode] = useState(false)
  const fileInputRef = useRef<HTMLInputElement>(null)

  const handleExport = async () => {
    setExporting(true)
    try {
      const response = await api.exportConfig()
      setExportedConfig(response.config)
      toast({
        title: "导出成功",
        description: `配置已导出，包含 ${Object.keys(response.config).length} 个模块`,
      })
    } catch (error) {
      toast({
        title: "导出失败",
        description: "无法导出配置",
        variant: "destructive",
      })
    } finally {
      setExporting(false)
    }
  }

  const handleDownloadFile = () => {
    if (!exportedConfig) return

    const blob = new Blob([JSON.stringify(exportedConfig, null, 2)], {
      type: "application/json",
    })
    const url = URL.createObjectURL(blob)
    const a = document.createElement("a")
    a.href = url
    a.download = `neotalk-config-${new Date().toISOString().slice(0, 10)}.json`
    document.body.appendChild(a)
    a.click()
    document.body.removeChild(a)
    URL.revokeObjectURL(url)
  }

  const handleFileSelect = (e: React.ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0]
    if (!file) return

    const reader = new FileReader()
    reader.onload = (event) => {
      try {
        const config = JSON.parse(event.target?.result as string)
        setImportPreview(config)
        setValidationErrors([])
      } catch (error) {
        toast({
          title: "文件解析失败",
          description: "请确保上传的是有效的 JSON 文件",
          variant: "destructive",
        })
      }
    }
    reader.readAsText(file)
  }

  const handleValidate = async () => {
    if (!importPreview) return

    setValidating(true)
    try {
      const response = await api.validateConfig(importPreview)
      if (response.valid) {
        toast({
          title: "验证通过",
          description: "配置文件格式正确",
        })
        setValidationErrors([])
      } else {
        setValidationErrors(response.errors || [])
        toast({
          title: "验证失败",
          description: `发现 ${response.errors?.length || 0} 个错误`,
          variant: "destructive",
        })
      }
    } catch (error) {
      toast({
        title: "验证失败",
        description: "无法验证配置文件",
        variant: "destructive",
      })
    } finally {
      setValidating(false)
    }
  }

  const handleImport = async () => {
    if (!importPreview) return

    setImporting(true)
    try {
      const response = await api.importConfig(importPreview, mergeMode)
      toast({
        title: "导入成功",
        description: `已导入 ${response.imported} 项配置`,
      })
      setImportPreview(null)
      setExportedConfig(null)
      if (fileInputRef.current) {
        fileInputRef.current.value = ""
      }
    } catch (error) {
      toast({
        title: "导入失败",
        description: "无法导入配置",
        variant: "destructive",
      })
    } finally {
      setImporting(false)
    }
  }

  return (
    <div className="p-6 space-y-6">
      {/* Export Section */}
      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2">
            <Download className="h-5 w-5" />
            导出配置
          </CardTitle>
          <CardDescription>
            将当前系统配置导出为 JSON 文件，可用于备份或迁移
          </CardDescription>
        </CardHeader>
        <CardContent className="space-y-4">
          <div className="flex gap-2">
            <Button onClick={handleExport} disabled={exporting}>
              {exporting ? (
                <>
                  <Loader2 className="mr-2 h-4 w-4 animate-spin" />
                  导出中...
                </>
              ) : (
                <>
                  <Download className="mr-2 h-4 w-4" />
                  导出配置
                </>
              )}
            </Button>
            {exportedConfig && (
              <Button variant="outline" onClick={handleDownloadFile}>
                <FileText className="mr-2 h-4 w-4" />
                下载文件
              </Button>
            )}
          </div>

          {exportedConfig && (
            <div className="mt-4">
              <Label className="mb-2 block">导出的配置模块:</Label>
              <div className="flex flex-wrap gap-2">
                {Object.keys(exportedConfig).map((key) => (
                  <span
                    key={key}
                    className="px-2 py-1 bg-muted rounded text-sm"
                  >
                    {key}
                  </span>
                ))}
              </div>
            </div>
          )}
        </CardContent>
      </Card>

      {/* Import Section */}
      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2">
            <Upload className="h-5 w-5" />
            导入配置
          </CardTitle>
          <CardDescription>
            从 JSON 文件导入配置，可选择合并或覆盖现有配置
          </CardDescription>
        </CardHeader>
        <CardContent className="space-y-4">
          <div>
            <input
              ref={fileInputRef}
              type="file"
              accept=".json"
              onChange={handleFileSelect}
              className="hidden"
              id="config-file-input"
            />
            <Button
              variant="outline"
              onClick={() => fileInputRef.current?.click()}
            >
              <Upload className="mr-2 h-4 w-4" />
              选择文件
            </Button>
            {importPreview && (
              <span className="ml-2 text-sm text-muted-foreground">
                已选择: {Object.keys(importPreview).length} 个模块
              </span>
            )}
          </div>

          {importPreview && (
            <>
              <div className="flex items-center gap-2">
                <Checkbox
                  id="merge-mode"
                  checked={mergeMode}
                  onCheckedChange={(checked) => setMergeMode(checked as boolean)}
                />
                <Label htmlFor="merge-mode" className="cursor-pointer">
                  合并模式（保留现有配置，只更新导入的项）
                </Label>
              </div>

              <div className="flex gap-2">
                <Button
                  variant="outline"
                  onClick={handleValidate}
                  disabled={validating}
                >
                  {validating ? (
                    <>
                      <Loader2 className="mr-2 h-4 w-4 animate-spin" />
                      验证中...
                    </>
                  ) : (
                    "验证配置"
                  )}
                </Button>
                <Button
                  onClick={handleImport}
                  disabled={importing || validationErrors.length > 0}
                >
                  {importing ? (
                    <>
                      <Loader2 className="mr-2 h-4 w-4 animate-spin" />
                      导入中...
                    </>
                  ) : (
                    <>
                      <Upload className="mr-2 h-4 w-4" />
                      导入配置
                    </>
                  )}
                </Button>
              </div>

              {/* Preview */}
              <div className="mt-4">
                <Label className="mb-2 block">导入的配置模块:</Label>
                <div className="flex flex-wrap gap-2">
                  {Object.keys(importPreview).map((key) => (
                    <span
                      key={key}
                      className="px-2 py-1 bg-muted rounded text-sm"
                    >
                      {key}
                    </span>
                  ))}
                </div>
              </div>

              {/* Validation Errors */}
              {validationErrors.length > 0 && (
                <div className="p-4 bg-destructive/10 rounded-md border border-destructive/20">
                  <div className="flex items-center gap-2 mb-2">
                    <AlertCircle className="h-4 w-4 text-destructive" />
                    <span className="font-medium text-destructive">验证错误:</span>
                  </div>
                  <ul className="list-disc list-inside text-sm text-destructive space-y-1">
                    {validationErrors.map((error, idx) => (
                      <li key={idx}>{error}</li>
                    ))}
                  </ul>
                </div>
              )}

              {/* JSON Preview */}
              <div className="mt-4">
                <Label className="mb-2 block">配置预览:</Label>
                <ScrollArea className="h-[300px] w-full rounded-md border p-4">
                  <pre className="text-xs">
                    {JSON.stringify(importPreview, null, 2)}
                  </pre>
                </ScrollArea>
              </div>
            </>
          )}
        </CardContent>
      </Card>

      {/* Info */}
      <Card className="bg-muted/50">
        <CardHeader>
          <CardTitle className="text-base">配置说明</CardTitle>
        </CardHeader>
        <CardContent className="text-sm text-muted-foreground space-y-2">
          <p>• 导出配置会保存当前系统所有设置，包括 LLM、MQTT、插件等配置</p>
          <p>• 导入配置时会验证配置文件的格式和必需字段</p>
          <p>• 覆盖模式会删除现有配置并使用导入的配置</p>
          <p>• 合并模式会保留现有配置，只更新导入文件中包含的项</p>
        </CardContent>
      </Card>
    </div>
  )
}
