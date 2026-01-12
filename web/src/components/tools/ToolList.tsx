import { useState, useEffect } from "react"
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card"
import { Button } from "@/components/ui/button"
import { Badge } from "@/components/ui/badge"
import { ScrollArea } from "@/components/ui/scroll-area"
import { LoadingState, EmptyState } from "@/components/shared"
import { Wrench, Play, TrendingUp, RefreshCw } from "lucide-react"
import type { Tool, ToolMetrics } from "@/types"
import { api } from "@/lib/api"

interface ToolListProps {
  onExecuteTool: (toolName: string) => void
}

export function ToolList({ onExecuteTool }: ToolListProps) {
  const [tools, setTools] = useState<Tool[]>([])
  const [metrics, setMetrics] = useState<ToolMetrics | null>(null)
  const [loading, setLoading] = useState(true)
  const [filterCategory, setFilterCategory] = useState<string | null>(null)

  const fetchTools = async () => {
    setLoading(true)
    try {
      const response = await api.listTools()
      setTools(response.tools)
    } catch (error) {
      console.error("Failed to fetch tools:", error)
    } finally {
      setLoading(false)
    }
  }

  const fetchMetrics = async () => {
    try {
      const response = await api.getToolMetrics()
      setMetrics(response.metrics)
    } catch (error) {
      console.error("Failed to fetch tool metrics:", error)
    }
  }

  useEffect(() => {
    fetchTools()
    fetchMetrics()
  }, [])

  // Get unique categories
  const categories = Array.from(new Set(tools.map((t) => t.category).filter(Boolean))) as string[]

  // Filter tools by category
  const filteredTools = filterCategory
    ? tools.filter((t) => t.category === filterCategory)
    : tools

  // Get call count for a tool
  const getToolCalls = (toolName: string) => {
    if (!metrics?.by_tool) return 0
    return metrics.by_tool[toolName]?.calls || 0
  }

  // Get avg time for a tool
  const getToolAvgTime = (toolName: string) => {
    if (!metrics?.by_tool) return 0
    return metrics.by_tool[toolName]?.avg_time_ms || 0
  }

  return (
    <div className="space-y-4">
      {/* Header with stats */}
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2">
          <Wrench className="h-5 w-5 text-muted-foreground" />
          <h3 className="font-semibold">可用工具</h3>
          <Badge variant="secondary">{tools.length}</Badge>
        </div>
        <Button variant="ghost" size="sm" onClick={() => { fetchTools(); fetchMetrics() }}>
          <RefreshCw className="h-4 w-4" />
        </Button>
      </div>

      {/* Metrics summary */}
      {metrics && (
        <div className="flex gap-4 text-sm text-muted-foreground">
          <span>总调用: {metrics.total_calls}</span>
          <span>成功: {metrics.successful_calls}</span>
          <span>失败: {metrics.failed_calls}</span>
          <span>平均耗时: {metrics.average_execution_time_ms.toFixed(0)}ms</span>
        </div>
      )}

      {/* Category filters */}
      {categories.length > 0 && (
        <div className="flex gap-2 flex-wrap">
          <Button
            variant={filterCategory === null ? "default" : "outline"}
            size="sm"
            onClick={() => setFilterCategory(null)}
          >
            全部
          </Button>
          {categories.map((cat) => (
            <Button
              key={cat}
              variant={filterCategory === cat ? "default" : "outline"}
              size="sm"
              onClick={() => setFilterCategory(cat)}
            >
              {cat}
            </Button>
          ))}
        </div>
      )}

      {/* Tool list */}
      {loading ? (
        <LoadingState text="加载工具中..." />
      ) : filteredTools.length === 0 ? (
        <EmptyState
          icon={<Wrench className="h-8 w-8 text-muted-foreground" />}
          title="暂无工具"
          description="系统中没有可用的工具"
        />
      ) : (
        <ScrollArea className="h-[400px]">
          <div className="space-y-2 pr-4">
            {filteredTools.map((tool) => (
              <Card key={tool.name} className="hover:bg-accent/50 transition-colors">
                <CardHeader className="pb-2">
                  <div className="flex items-start justify-between">
                    <div className="flex-1">
                      <CardTitle className="text-base flex items-center gap-2">
                        {tool.name}
                        {tool.category && (
                          <Badge variant="outline" className="text-xs">
                            {tool.category}
                          </Badge>
                        )}
                      </CardTitle>
                      {tool.description && (
                        <CardDescription className="text-xs mt-1">
                          {tool.description}
                        </CardDescription>
                      )}
                    </div>
                    <Button
                      variant="ghost"
                      size="icon"
                      onClick={() => onExecuteTool(tool.name)}
                    >
                      <Play className="h-4 w-4" />
                    </Button>
                  </div>
                </CardHeader>
                {(getToolCalls(tool.name) > 0 || getToolAvgTime(tool.name) > 0) && (
                  <CardContent className="pt-0">
                    <div className="flex items-center gap-3 text-xs text-muted-foreground">
                      <span className="flex items-center gap-1">
                        <TrendingUp className="h-3 w-3" />
                        调用: {getToolCalls(tool.name)}
                      </span>
                      {getToolAvgTime(tool.name) > 0 && (
                        <span>平均: {getToolAvgTime(tool.name).toFixed(0)}ms</span>
                      )}
                    </div>
                  </CardContent>
                )}
              </Card>
            ))}
          </div>
        </ScrollArea>
      )}
    </div>
  )
}
