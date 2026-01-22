import { useState, useEffect } from "react"
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
import { ScrollArea } from "@/components/ui/scroll-area"
import { Badge } from "@/components/ui/badge"
import { Card } from "@/components/ui/card"
import { Brain, TrendingUp, Trash2, RefreshCw } from "lucide-react"
import { api } from "@/lib/api"
import type { AgentMemory } from "@/types"

interface AgentMemoryDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  agentId: string
  agentName: string
}

export function AgentMemoryDialog({
  open,
  onOpenChange,
  agentId,
  agentName,
}: AgentMemoryDialogProps) {
  const { t } = useTranslation(['common', 'agents'])
  const [memory, setMemory] = useState<AgentMemory | null>(null)
  const [loading, setLoading] = useState(false)

  useEffect(() => {
    if (open && agentId) {
      loadMemory()
    }
  }, [open, agentId])

  const loadMemory = async () => {
    setLoading(true)
    try {
      const data = await api.getAgentMemory(agentId)
      setMemory(data)
    } catch (error) {
      console.error('Failed to load agent memory:', error)
    } finally {
      setLoading(false)
    }
  }

  const handleClearMemory = async () => {
    if (!confirm(t('agents:clearMemoryConfirm'))) return

    setLoading(true)
    try {
      await api.clearAgentMemory(agentId)
      await loadMemory()
    } catch (error) {
      console.error('Failed to clear memory:', error)
    } finally {
      setLoading(false)
    }
  }

  const formatDate = (timestamp: string | number) => {
    if (typeof timestamp === 'string') {
      return new Date(timestamp).toLocaleString()
    }
    return new Date(timestamp * 1000).toLocaleString()
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-2xl max-h-[80vh]">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <Brain className="h-5 w-5 text-primary" />
            {t('agents:memory.title')} - {agentName}
          </DialogTitle>
          <DialogDescription>
            {t('agents:memory.description')}
          </DialogDescription>
        </DialogHeader>

        {loading ? (
          <div className="flex items-center justify-center py-8">
            <RefreshCw className="h-6 w-6 animate-spin text-muted-foreground" />
          </div>
        ) : memory ? (
          <ScrollArea className="max-h-[60vh] pr-4">
            <div className="space-y-6">
              {/* State Variables */}
              {Object.keys(memory.state_variables).length > 0 && (
                <div>
                  <h4 className="text-sm font-semibold mb-2 flex items-center gap-2">
                    <Brain className="h-4 w-4" />
                    {t('agents:memory.stateVariables')}
                  </h4>
                  <Card className="p-4">
                    <div className="grid grid-cols-2 gap-4">
                      {Object.entries(memory.state_variables).map(([key, value]) => (
                        <div key={key} className="space-y-1">
                          <div className="text-xs text-muted-foreground">{key}</div>
                          <div className="text-sm font-mono bg-muted px-2 py-1 rounded">
                            {String(value)}
                          </div>
                        </div>
                      ))}
                    </div>
                  </Card>
                </div>
              )}

              {/* Baselines */}
              {Object.keys(memory.baselines).length > 0 && (
                <div>
                  <h4 className="text-sm font-semibold mb-2 flex items-center gap-2">
                    <TrendingUp className="h-4 w-4" />
                    {t('agents:memory.baselines')}
                  </h4>
                  <Card className="p-4">
                    <div className="space-y-2">
                      {Object.entries(memory.baselines).map(([metric, value]) => (
                        <div key={metric} className="flex justify-between items-center">
                          <span className="text-sm">{metric}</span>
                          <Badge variant="secondary">{value.toFixed(2)}</Badge>
                        </div>
                      ))}
                    </div>
                  </Card>
                </div>
              )}

              {/* Learned Patterns */}
              {memory.learned_patterns.length > 0 && (
                <div>
                  <h4 className="text-sm font-semibold mb-2">{t('agents:memory.learnedPatterns')}</h4>
                  <Card className="p-4">
                    <div className="space-y-2">
                      {memory.learned_patterns.map((pattern, idx) => (
                        <div key={idx} className="text-sm p-2 bg-muted rounded">
                          {pattern}
                        </div>
                      ))}
                    </div>
                  </Card>
                </div>
              )}

              {/* Trend Data Summary */}
              {memory.trend_data.length > 0 && (
                <div>
                  <h4 className="text-sm font-semibold mb-2 flex items-center gap-2">
                    <TrendingUp className="h-4 w-4" />
                    {t('agents:memory.trendData')} ({memory.trend_data.length} {t('common:points')})
                  </h4>
                  <Card className="p-4">
                    <div className="text-sm text-muted-foreground">
                      {t('agents:memory.trendDataHint', { count: memory.trend_data.length })}
                    </div>
                  </Card>
                </div>
              )}

              {/* Updated At */}
              <div className="text-xs text-muted-foreground">
                {t('agents:memory.updatedAt')}: {formatDate(memory.updated_at)}
              </div>
            </div>
          </ScrollArea>
        ) : (
          <div className="text-center py-8 text-muted-foreground">
            {t('agents:memory.noMemory')}
          </div>
        )}

        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)}>
            {t('common:close')}
          </Button>
          <Button
            variant="destructive"
            onClick={handleClearMemory}
            disabled={loading || !memory || Object.keys(memory.state_variables).length === 0}
          >
            <Trash2 className="h-4 w-4 mr-2" />
            {t('agents:memory.clear')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
