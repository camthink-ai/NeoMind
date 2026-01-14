import { useState, useEffect } from "react"
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
import { Textarea } from "@/components/ui/textarea"
import { Badge } from "@/components/ui/badge"
import { ScrollArea } from "@/components/ui/scroll-area"
import { Loader2, Play, AlertCircle, CheckCircle2 } from "lucide-react"
import type { ToolSchema, ToolExecutionResult } from "@/types"
import { api } from "@/lib/api"
import { useToast } from "@/hooks/use-toast"

interface ToolExecuteDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  toolName: string
}

export function ToolExecuteDialog({ open, onOpenChange, toolName }: ToolExecuteDialogProps) {
  const { toast } = useToast()
  const [schema, setSchema] = useState<ToolSchema | null>(null)
  const [loading, setLoading] = useState(false)
  const [executing, setExecuting] = useState(false)
  const [parameters, setParameters] = useState<Record<string, unknown>>({})
  const [result, setResult] = useState<ToolExecutionResult | null>(null)

  useEffect(() => {
    if (open && toolName) {
      fetchSchema()
      setParameters({})
      setResult(null)
    }
  }, [open, toolName])

  const fetchSchema = async () => {
    setLoading(true)
    try {
      const response = await api.getToolSchema(toolName)
      setSchema(response.schema)
      // Initialize parameters with empty values
      const initParams: Record<string, unknown> = {}
      if (response.schema.parameters?.properties) {
        Object.keys(response.schema.parameters.properties).forEach((key) => {
          initParams[key] = ""
        })
      }
      setParameters(initParams)
    } catch (error) {
      toast({
        title: "加载失败",
        description: `无法加载工具 ${toolName} 的参数定义`,
        variant: "destructive",
      })
    } finally {
      setLoading(false)
    }
  }

  const handleParameterChange = (key: string, value: unknown) => {
    setParameters((prev) => ({
      ...prev,
      [key]: value,
    }))
  }

  const handleExecute = async () => {
    setExecuting(true)
    setResult(null)
    try {
      const response = await api.executeTool(toolName, parameters)
      setResult(response.result)
      if (response.result.success) {
        toast({
          title: "执行成功",
          description: `工具 ${toolName} 执行完成`,
        })
      } else {
        toast({
          title: "执行失败",
          description: response.result.error || "工具执行返回失败状态",
          variant: "destructive",
        })
      }
    } catch (error) {
      setResult({
        success: false,
        result: null,
        error: error instanceof Error ? error.message : "未知错误",
        execution_time_ms: 0,
      })
      toast({
        title: "执行失败",
        description: "工具执行出错",
        variant: "destructive",
      })
    } finally {
      setExecuting(false)
    }
  }

  const isRequired = (key: string) => {
    return schema?.parameters?.required?.includes(key)
  }

  const getParameterDescription = (key: string) => {
    return schema?.parameters?.properties?.[key]?.description || ""
  }

  const getParameterValue = (key: string) => {
    const value = parameters[key]
    return value ?? ""
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-2xl max-h-[80vh] overflow-hidden flex flex-col">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <Play className="h-5 w-5" />
            执行工具: {toolName}
          </DialogTitle>
          {schema?.description && (
            <DialogDescription>{schema.description}</DialogDescription>
          )}
        </DialogHeader>

        {loading ? (
          <div className="flex-1 flex items-center justify-center">
            <Loader2 className="h-6 w-6 animate-spin text-muted-foreground" />
          </div>
        ) : (
          <ScrollArea className="flex-1 px-1">
            <div className="space-y-4 pr-4">
              {/* Parameters */}
              {schema && schema.parameters?.properties && (
                <div className="space-y-3">
                  <Label>参数</Label>
                  {Object.entries(schema.parameters.properties).map(([key, prop]) => (
                    <div key={key} className="space-y-1">
                      <div className="flex items-center gap-2">
                        <Label htmlFor={key} className="text-sm">
                          {key}
                        </Label>
                        <Badge variant="outline" className="text-xs">
                          {prop.type}
                        </Badge>
                        {isRequired(key) && (
                          <Badge variant="destructive" className="text-xs">
                            必填
                          </Badge>
                        )}
                      </div>
                      {getParameterDescription(key) && (
                        <p className="text-xs text-muted-foreground">
                          {getParameterDescription(key)}
                        </p>
                      )}
                      {prop.type === "string" &&
                      (key.includes("message") || key.includes("content") || key.includes("text")) ? (
                        <Textarea
                          id={key}
                          value={String(getParameterValue(key))}
                          onChange={(e) => handleParameterChange(key, e.target.value)}
                          placeholder={`请输入 ${key}`}
                          className="min-h-[80px]"
                        />
                      ) : prop.type === "boolean" ? (
                        <select
                          id={key}
                          value={String(getParameterValue(key))}
                          onChange={(e) => handleParameterChange(key, e.target.value === "true")}
                          className="flex h-10 w-full rounded-md border border-input bg-background px-3 py-2 text-sm"
                        >
                          <option value="false">false</option>
                          <option value="true">true</option>
                        </select>
                      ) : (
                        <Input
                          id={key}
                          type={prop.type === "number" ? "number" : "text"}
                          value={String(getParameterValue(key))}
                          onChange={(e) =>
                            handleParameterChange(
                              key,
                              prop.type === "number" ? Number(e.target.value) : e.target.value
                            )
                          }
                          placeholder={`请输入 ${key}`}
                        />
                      )}
                    </div>
                  ))}
                </div>
              )}

              {/* Result */}
              {result && (
                <div className="space-y-2">
                  <div className="flex items-center gap-2">
                    {result.success ? (
                      <CheckCircle2 className="h-5 w-5 text-success" />
                    ) : (
                      <AlertCircle className="h-5 w-5 text-destructive" />
                    )}
                    <Label>执行结果</Label>
                    <Badge variant={result.success ? "default" : "destructive"}>
                      {result.success ? "成功" : "失败"}
                    </Badge>
                    <span className="text-xs text-muted-foreground ml-auto">
                      耗时: {result.execution_time_ms}ms
                    </span>
                  </div>
                  {result.error && (
                    <div className="p-3 bg-destructive/10 rounded-md border border-destructive/20">
                      <p className="text-sm text-destructive">{result.error}</p>
                    </div>
                  )}
                  {result.result !== null && result.result !== undefined && (
                    <pre className="p-3 bg-muted rounded-md text-xs overflow-x-auto">
                      {JSON.stringify(result.result, null, 2)}
                    </pre>
                  )}
                </div>
              )}
            </div>
          </ScrollArea>
        )}

        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)} disabled={executing}>
            关闭
          </Button>
          <Button onClick={handleExecute} disabled={executing || loading}>
            {executing ? (
              <>
                <Loader2 className="mr-2 h-4 w-4 animate-spin" />
                执行中...
              </>
            ) : (
              <>
                <Play className="mr-2 h-4 w-4" />
                执行
              </>
            )}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
