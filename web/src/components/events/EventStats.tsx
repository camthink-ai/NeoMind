import { useState, useEffect } from "react"
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card"
import { ScrollArea } from "@/components/ui/scroll-area"
import { LoadingState } from "@/components/shared"
import { BarChart3, TrendingUp, AlertCircle } from "lucide-react"
import { api } from "@/lib/api"
import type { EventStats } from "@/types"

interface EventStatsProps {
  refreshInterval?: number
}

export function EventStats({ refreshInterval = 10000 }: EventStatsProps) {
  const [stats, setStats] = useState<EventStats | null>(null)
  const [loading, setLoading] = useState(true)

  const fetchStats = async () => {
    try {
      const response = await api.getEventStats()
      setStats(response.stats)
    } catch (error) {
      console.error("Failed to fetch event stats:", error)
    } finally {
      setLoading(false)
    }
  }

  useEffect(() => {
    fetchStats()
    const interval = setInterval(fetchStats, refreshInterval)
    return () => clearInterval(interval)
  }, [refreshInterval])

  if (loading) {
    return <LoadingState text="加载统计中..." />
  }

  if (!stats) {
    return (
      <Card>
        <CardContent className="pt-6">
          <p className="text-sm text-muted-foreground text-center">
            无法加载事件统计
          </p>
        </CardContent>
      </Card>
    )
  }

  return (
    <Card>
      <CardHeader className="pb-3">
        <CardTitle className="text-base flex items-center gap-2">
          <BarChart3 className="h-4 w-4" />
          事件统计
        </CardTitle>
      </CardHeader>
      <CardContent className="pt-0">
        <div className="space-y-4">
          {/* Summary Stats */}
          <div className="grid grid-cols-3 gap-4">
            <div className="text-center">
              <div className="text-2xl font-bold">{stats.total_events}</div>
              <div className="text-xs text-muted-foreground">总事件</div>
            </div>
            <div className="text-center">
              <div className="text-2xl font-bold text-green-600">{stats.processed}</div>
              <div className="text-xs text-muted-foreground">已处理</div>
            </div>
            <div className="text-center">
              <div className="text-2xl font-bold text-orange-600">{stats.pending}</div>
              <div className="text-xs text-muted-foreground">待处理</div>
            </div>
          </div>

          {/* By Type */}
          <div>
            <h4 className="text-sm font-medium mb-2 flex items-center gap-2">
              <TrendingUp className="h-4 w-4" />
              按类型
            </h4>
            <ScrollArea className="h-[150px]">
              <div className="space-y-2 pr-4">
                {Object.entries(stats.by_type).map(([type, count]) => {
                  const maxCount = Math.max(...Object.values(stats.by_type))
                  const percentage = (count / maxCount) * 100
                  return (
                    <div key={type} className="space-y-1">
                      <div className="flex items-center justify-between text-xs">
                        <span className="font-medium">{type}</span>
                        <span className="text-muted-foreground">{count}</span>
                      </div>
                      <div className="h-2 bg-muted rounded-full overflow-hidden">
                        <div
                          className="h-full bg-primary rounded-full"
                          style={{ width: `${percentage}%` }}
                        />
                      </div>
                    </div>
                  )
                })}
              </div>
            </ScrollArea>
          </div>

          {/* By Source */}
          {Object.keys(stats.by_source).length > 0 && (
            <div>
              <h4 className="text-sm font-medium mb-2 flex items-center gap-2">
                <AlertCircle className="h-4 w-4" />
                按来源
              </h4>
              <ScrollArea className="h-[120px]">
                <div className="space-y-1 pr-4">
                  {Object.entries(stats.by_source)
                    .sort(([, a], [, b]) => b - a)
                    .slice(0, 5)
                    .map(([source, count]) => (
                      <div
                        key={source}
                        className="flex items-center justify-between text-xs p-2 rounded hover:bg-accent"
                      >
                        <span className="truncate flex-1">{source}</span>
                        <span className="text-muted-foreground font-mono">
                          {count}
                        </span>
                      </div>
                    ))}
                </div>
              </ScrollArea>
            </div>
          )}
        </div>
      </CardContent>
    </Card>
  )
}
