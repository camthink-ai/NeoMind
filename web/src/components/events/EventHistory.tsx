import { useState, useEffect } from "react"
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card"
import { Button } from "@/components/ui/button"
import { Badge } from "@/components/ui/badge"
import { ScrollArea } from "@/components/ui/scroll-area"
import { LoadingState, EmptyState } from "@/components/shared"
import { Activity, RefreshCw, Clock, Tag } from "lucide-react"
import { api } from "@/lib/api"
import type { Event as NeoTalkEvent } from "@/types"
import { formatTimestamp } from "@/lib/utils/format"

interface EventHistoryProps {
  limit?: number
  eventType?: string
  source?: string
}

export function EventHistory({ limit = 50, eventType, source }: EventHistoryProps) {
  const [events, setEvents] = useState<NeoTalkEvent[]>([])
  const [loading, setLoading] = useState(true)

  const fetchEvents = async () => {
    setLoading(true)
    try {
      const response = await api.getEventHistory({
        event_type: eventType,
        source,
        limit,
      })
      setEvents(response.events)
    } catch (error) {
      console.error("Failed to fetch events:", error)
    } finally {
      setLoading(false)
    }
  }

  useEffect(() => {
    fetchEvents()
  }, [eventType, source, limit])

  const getEventTypeColor = (type: string) => {
    const colors: Record<string, string> = {
      device: "bg-blue-500/10 text-blue-500 border-blue-500/20",
      alert: "bg-red-500/10 text-red-500 border-red-500/20",
      command: "bg-green-500/10 text-green-500 border-green-500/20",
      decision: "bg-purple-500/10 text-purple-500 border-purple-500/20",
      workflow: "bg-orange-500/10 text-orange-500 border-orange-500/20",
      system: "bg-gray-500/10 text-gray-500 border-gray-500/20",
    }
    return colors[type] || "bg-gray-500/10 text-gray-500 border-gray-500/20"
  }

  return (
    <Card>
      <CardHeader className="pb-3">
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-2">
            <Activity className="h-5 w-5 text-muted-foreground" />
            <CardTitle className="text-base">事件历史</CardTitle>
            <Badge variant="secondary">{events.length}</Badge>
          </div>
          <Button
            variant="ghost"
            size="icon"
            onClick={fetchEvents}
          >
            <RefreshCw className="h-4 w-4" />
          </Button>
        </div>
        <CardDescription className="text-xs">
          系统事件的实时记录
        </CardDescription>
      </CardHeader>
      <CardContent className="pt-0">
        {loading ? (
          <LoadingState text="加载事件中..." />
        ) : events.length === 0 ? (
          <EmptyState
            icon={<Activity className="h-8 w-8 text-muted-foreground" />}
            title="暂无事件"
            description="系统事件会在此显示"
          />
        ) : (
          <ScrollArea className="h-[300px]">
            <div className="space-y-2 pr-4">
              {events.map((event) => (
                <div
                  key={event.id}
                  className="flex items-start gap-3 p-3 rounded-lg border bg-card/50 hover:bg-accent/50 transition-colors"
                >
                  <Activity className="h-4 w-4 text-muted-foreground mt-0.5" />
                  <div className="flex-1 min-w-0">
                    <div className="flex items-center gap-2 mb-1">
                      <span className="font-medium text-sm truncate">
                        {event.event_type}
                      </span>
                      <Badge
                        variant="outline"
                        className={`text-xs ${getEventTypeColor(event.event_type)}`}
                      >
                        {event.event_type}
                      </Badge>
                      {event.source && (
                        <span className="text-xs text-muted-foreground flex items-center gap-1">
                          <Tag className="h-3 w-3" />
                          {event.source}
                        </span>
                      )}
                    </div>
                    {event.data && Object.keys(event.data).length > 0 && (
                      <pre className="text-xs text-muted-foreground bg-muted rounded p-2 mt-1 overflow-x-auto">
                        {JSON.stringify(event.data, null, 2)}
                      </pre>
                    )}
                    <div className="flex items-center gap-1 mt-1 text-xs text-muted-foreground">
                      <Clock className="h-3 w-3" />
                      {formatTimestamp(event.timestamp)}
                    </div>
                  </div>
                </div>
              ))}
            </div>
          </ScrollArea>
        )}
      </CardContent>
    </Card>
  )
}
