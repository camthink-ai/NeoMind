import { useState, useEffect } from "react"
import { useStore } from "@/store"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { CheckCircle } from "lucide-react"
import { toast } from "@/components/ui/use-toast"

export function LLMSettings() {
  const { llmSettings, updateLlmSettings, testLlm } = useStore()

  const [backend, setBackend] = useState(llmSettings?.backend || "ollama")
  // Set default endpoint for Ollama when initializing or when no endpoint is set
  const [endpoint, setEndpoint] = useState(
    llmSettings?.endpoint || (backend === "ollama" ? "http://localhost:11434" : "")
  )
  const [apiKey, setApiKey] = useState(llmSettings?.api_key || "")
  const [model, setModel] = useState(llmSettings?.model || "qwen3-vl:2b")
  const [isSaving, setIsSaving] = useState(false)
  const [isTesting, setIsTesting] = useState(false)

  useEffect(() => {
    if (llmSettings) {
      setBackend(llmSettings.backend)
      // Only update endpoint if it's not already set by user (avoid overwriting user input during typing)
      if (!endpoint) {
        const ep = llmSettings.endpoint || ""
        if (!ep && llmSettings.backend === "ollama") {
          setEndpoint("http://localhost:11434")
        } else if (!ep && llmSettings.backend === "openai") {
          setEndpoint("https://api.openai.com/v1")
        } else if (!ep && llmSettings.backend === "anthropic") {
          setEndpoint("https://api.anthropic.com/v1")
        } else if (!ep && llmSettings.backend === "google") {
          setEndpoint("https://generativelanguage.googleapis.com/v1")
        } else {
          setEndpoint(ep)
        }
      }
      setApiKey(llmSettings.api_key || "")
      if (!model || model === "qwen3-vl:2b") {
        setModel(llmSettings.model || (llmSettings.backend === "ollama" ? "qwen3-vl:2b" : ""))
      }
    }
  }, [llmSettings, backend, endpoint, model])

  const handleSave = async () => {
    setIsSaving(true)
    const success = await updateLlmSettings({
      backend,
      endpoint: endpoint || undefined,
      api_key: apiKey || undefined,
      model,
    })
    setIsSaving(false)

    if (success) {
      toast({ title: "设置已保存" })
    } else {
      toast({ title: "保存失败", variant: "destructive" })
    }
  }

  const handleTest = async () => {
    setIsTesting(true)
    const success = await testLlm()
    setIsTesting(false)

    if (success) {
      toast({ title: "连接成功" })
    } else {
      toast({ title: "连接失败", variant: "destructive" })
    }
  }

  const handleBackendChange = (value: string) => {
    setBackend(value)
    if (value === "ollama") {
      setEndpoint("http://localhost:11434")
      setModel("qwen3-vl:2b")
    } else if (value === "openai") {
      setEndpoint("https://api.openai.com/v1")
      setModel("gpt-4")
    } else if (value === "anthropic") {
      setEndpoint("https://api.anthropic.com/v1")
      setModel("claude-3-opus-20240229")
    } else if (value === "google") {
      setEndpoint("https://generativelanguage.googleapis.com/v1")
      setModel("gemini-pro")
    }
    setApiKey("")
  }

  return (
    <div className="max-w-md space-y-4 py-4">
      <div className="space-y-2">
        <Label htmlFor="backend">后端</Label>
        <Select value={backend} onValueChange={handleBackendChange}>
          <SelectTrigger id="backend">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="ollama">Ollama</SelectItem>
            <SelectItem value="openai">OpenAI</SelectItem>
            <SelectItem value="anthropic">Anthropic</SelectItem>
            <SelectItem value="google">Google</SelectItem>
          </SelectContent>
        </Select>
      </div>

      <div className="space-y-2">
        <Label htmlFor="endpoint">端点</Label>
        <Input
          id="endpoint"
          value={endpoint}
          onChange={(e) => setEndpoint(e.target.value)}
        />
      </div>

      {backend !== "ollama" && (
        <div className="space-y-2">
          <Label htmlFor="api-key">API 密钥</Label>
          <Input
            id="api-key"
            type="password"
            value={apiKey}
            onChange={(e) => setApiKey(e.target.value)}
          />
        </div>
      )}

      <div className="space-y-2">
        <Label htmlFor="model">模型</Label>
        <Input
          id="model"
          value={model}
          onChange={(e) => setModel(e.target.value)}
        />
      </div>

      <div className="pt-4 flex gap-2">
        <Button onClick={handleSave} disabled={isSaving} size="sm">
          {isSaving ? "保存中..." : "保存"}
        </Button>
        <Button variant="outline" onClick={handleTest} disabled={isTesting} size="sm">
          {isTesting ? "测试中..." : (
            <>
              <CheckCircle className="mr-2 h-4 w-4" />
              测试
            </>
          )}
        </Button>
      </div>
    </div>
  )
}
