import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card"
import { Badge } from "@/components/ui/badge"
import {
  Bot,
  Cpu,
  Database,
  Network,
  Shield,
  Zap,
  Code,
  Heart,
  Book,
  Workflow,
  Layers
} from "lucide-react"

export function AboutTab() {
  return (
    <div className="p-6 max-w-4xl mx-auto space-y-6">
      {/* Header */}
      <div className="text-center space-y-2">
        <div className="flex items-center justify-center gap-3">
          <div className="flex items-center justify-center w-16 h-16 rounded-2xl bg-gradient-to-br from-gray-900 to-gray-700 dark:from-white dark:to-gray-300 text-white dark:text-gray-900 shadow-lg">
            <Bot className="w-8 h-8" />
          </div>
          <h1 className="text-3xl font-bold bg-gradient-to-r from-gray-900 to-gray-600 dark:from-white dark:to-gray-400 bg-clip-text text-transparent">
            NeoTalk
          </h1>
        </div>
        <p className="text-muted-foreground">
          智能边缘 AI Agent 平台
        </p>
        <Badge variant="outline" className="mx-auto">v0.1.0</Badge>
      </div>

      {/* Overview */}
      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2">
            <Heart className="h-5 w-5 text-red-500" />
            项目简介
          </CardTitle>
        </CardHeader>
        <CardContent className="space-y-3 text-sm">
          <p>
            NeoTalk 是一个运行在边缘设备上的智能 AI Agent 平台，支持多模型 LLM、事件驱动架构、设备管理、规则引擎和工作流编排。
          </p>
          <p>
            通过插件化架构，NeoTalk 可以灵活扩展功能，支持 MQTT、Modbus、Home Assistant 等多种设备适配器，实现真正的智能化边缘计算。
          </p>
        </CardContent>
      </Card>

      {/* Tech Stack */}
      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2">
            <Layers className="h-5 w-5 text-blue-500" />
            技术栈
          </CardTitle>
        </CardHeader>
        <CardContent className="space-y-4">
          <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
            <div>
              <h4 className="font-medium mb-2 flex items-center gap-2">
                <Cpu className="h-4 w-4 text-orange-500" />
                后端
              </h4>
              <div className="text-sm text-muted-foreground space-y-1">
                <div>• Rust - 系统核心</div>
                <div>• Axum - Web 框架</div>
                <div>• Tokio - 异步运行时</div>
                <div>• redb - 嵌入式数据库</div>
              </div>
            </div>
            <div>
              <h4 className="font-medium mb-2 flex items-center gap-2">
                <Code className="h-4 w-4 text-blue-500" />
                前端
              </h4>
              <div className="text-sm text-muted-foreground space-y-1">
                <div>• React 18 - UI 框架</div>
                <div>• TypeScript - 类型安全</div>
                <div>• Vite - 构建工具</div>
                <div>• Tailwind CSS - 样式</div>
              </div>
            </div>
          </div>
        </CardContent>
      </Card>

      {/* Core Features */}
      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2">
            <Zap className="h-5 w-5 text-warning" />
            核心功能
          </CardTitle>
        </CardHeader>
        <CardContent>
          <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
            <FeatureItem
              icon={<Bot className="h-4 w-4" />}
              title="多模型 LLM 支持"
              description="Ollama、OpenAI、Anthropic、Google、xAI"
            />
            <FeatureItem
              icon={<Workflow className="h-4 w-4" />}
              title="事件驱动架构"
              description="EventBus 实时消息分发"
            />
            <FeatureItem
              icon={<Network className="h-4 w-4" />}
              title="设备管理"
              description="MQTT、Modbus、Home Assistant 集成"
            />
            <FeatureItem
              icon={<Database className="h-4 w-4" />}
              title="数据持久化"
              description="时序数据、向量搜索、决策存储"
            />
            <FeatureItem
              icon={<Shield className="h-4 w-4" />}
              title="规则引擎"
              description="Pest DSL 解析、实时评估"
            />
            <FeatureItem
              icon={<Layers className="h-4 w-4" />}
              title="插件系统"
              description="动态加载、Schema 驱动配置"
            />
          </div>
        </CardContent>
      </Card>

      {/* Project Links */}
      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2">
            <Book className="h-5 w-5 text-green-500" />
            项目信息
          </CardTitle>
        </CardHeader>
        <CardContent className="space-y-3 text-sm">
          <div className="flex items-center justify-between border-b pb-2">
            <span className="text-muted-foreground">版本</span>
            <Badge variant="secondary">v0.1.0</Badge>
          </div>
          <div className="flex items-center justify-between border-b pb-2">
            <span className="text-muted-foreground">许可证</span>
            <span>MIT</span>
          </div>
          <div className="flex items-center justify-between">
            <span className="text-muted-foreground">架构</span>
            <span>Workspace (Cargo)</span>
          </div>
        </CardContent>
      </Card>

      {/* Footer */}
      <div className="text-center text-sm text-muted-foreground">
        <p>© 2025 NeoTalk. Built with ❤️ for edge intelligence.</p>
      </div>
    </div>
  )
}

function FeatureItem({
  icon,
  title,
  description
}: {
  icon: React.ReactNode
  title: string
  description: string
}) {
  return (
    <div className="flex items-start gap-3 p-3 rounded-lg bg-muted/50">
      <div className="flex items-center justify-center w-8 h-8 rounded-md bg-background shrink-0">
        {icon}
      </div>
      <div>
        <div className="font-medium text-sm">{title}</div>
        <div className="text-xs text-muted-foreground">{description}</div>
      </div>
    </div>
  )
}
