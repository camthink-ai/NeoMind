/**
 * EntityInspectorContent — shared data loader and body renderer for all 5
 * entity attachment kinds (device, rule, transform, agent, dashboard).
 *
 * Consumed by all 3 presentation hosts with zero duplication:
 * 1. EntityInspectorPane  — desktop /chat in-flow split side panel (no backdrop)
 * 2. EntityDetailDialog   — desktop overlay Sheet (PanelChatView)
 * 3. EntityDetailDialog   — mobile full-screen UnifiedFormDialog (< 768px)
 */

import { useCallback, useEffect, useRef, useState } from "react"
import { useNavigate } from "react-router-dom"
import { useTranslation } from "react-i18next"
import {
  AtSign,
  Box,
  ExternalLink,
  LayoutGrid,
  Loader2,
  Play,
  Sparkles,
  Workflow,
  Zap,
} from "lucide-react"
import type {
  AgentExecution,
  AgentExecutionDetail,
  AgentSpec,
  AiAgentDetail,
  Automation,
  ChatAttachment,
  ChatReference,
  DashboardResponse,
  Device,
  DeviceCurrentStateResponse,
  Rule,
  RuleExecutionResult,
  RuleExplanation,
} from "@/types"
import { api } from "@/lib/api"
import type { ExplainLoadStatus } from "@/components/automation/RuleWhyNotTriggeredPanel"
import { TransformTestDialog } from "@/components/automation/TransformTestDialog"
import { Button } from "@/components/ui/button"
import { SnapshotKV } from "./inspector/shared"
import { DeviceInspectorView } from "./inspector/DeviceInspectorView"
import { RuleInspectorView } from "./inspector/RuleInspectorView"
import { TransformInspectorView } from "./inspector/TransformInspectorView"
import { AgentInspectorView } from "./inspector/AgentInspectorView"
import { DashboardInspectorView } from "./inspector/DashboardInspectorView"

export function getEntityKindMeta(kind: ChatAttachment["kind"]) {
  switch (kind) {
    case "device":
      return {
        Icon: Box,
        color: "text-foreground",
        iconBg: "bg-muted-30 border border-border",
        labelZh: "设备",
        labelEn: "Device",
      }
    case "rule":
      return {
        Icon: Zap,
        color: "text-foreground",
        iconBg: "bg-muted-30 border border-border",
        labelZh: "规则",
        labelEn: "Rule",
      }
    case "transform":
      return {
        Icon: Workflow,
        color: "text-foreground",
        iconBg: "bg-muted-30 border border-border",
        labelZh: "数据转换",
        labelEn: "Transform",
      }
    case "agent":
      return {
        Icon: Sparkles,
        color: "text-foreground",
        iconBg: "bg-muted-30 border border-border",
        labelZh: "代理",
        labelEn: "Agent",
      }
    case "dashboard":
      return {
        Icon: LayoutGrid,
        color: "text-foreground",
        iconBg: "bg-muted-30 border border-border",
        labelZh: "仪表盘",
        labelEn: "Dashboard",
      }
    default:
      return {
        Icon: Box,
        color: "text-muted-foreground",
        iconBg: "bg-muted-30 border border-border",
        labelZh: "对象",
        labelEn: "Entity",
      }
  }
}

export interface UseEntityInspectorOptions {
  attachment: ChatAttachment | null
  active: boolean
  /** Keep the host open when mentioning (true for in-flow split pane). */
  keepOpenOnMention?: boolean
  onClose: () => void
  onAttachReference?: (ref: ChatReference) => void
}

export function useEntityInspector({
  attachment,
  active,
  keepOpenOnMention = false,
  onClose,
  onAttachReference,
}: UseEntityInspectorOptions) {
  const { i18n } = useTranslation(["chat", "common"])
  const navigate = useNavigate()
  const isZh = i18n.language?.toLowerCase().startsWith("zh")

  const [refreshNonce, setRefreshNonce] = useState(0)
  const [loading, setLoading] = useState(false)
  const [notFound, setNotFound] = useState(false)

  const [device, setDevice] = useState<Device | null>(null)
  const [deviceCurrent, setDeviceCurrent] = useState<DeviceCurrentStateResponse | null>(null)
  const [rule, setRule] = useState<Rule | null>(null)
  const [ruleExplain, setRuleExplain] = useState<RuleExplanation | null>(null)
  const [explainStatus, setExplainStatus] = useState<ExplainLoadStatus>("idle")
  const [ruleHistory, setRuleHistory] = useState<RuleExecutionResult[]>([])
  const [transform, setTransform] = useState<{
    automation: Automation
    definition: Record<string, unknown> | null
  } | null>(null)
  const [agent, setAgent] = useState<(AiAgentDetail & AgentSpec) | null>(null)
  const [latestExec, setLatestExec] = useState<AgentExecutionDetail | null>(null)
  const [agentExecutions, setAgentExecutions] = useState<AgentExecution[]>([])
  const [dashboard, setDashboard] = useState<DashboardResponse | null>(null)

  const [testDialogOpen, setTestDialogOpen] = useState(false)
  const [testDevices, setTestDevices] = useState<
    Array<{ id: string; name: string; device_type?: string }>
  >([])

  const entityId = attachment?.entityId
  const kind = attachment?.kind

  const refresh = useCallback(() => {
    setRefreshNonce((n) => n + 1)
  }, [])

  useEffect(() => {
    setTestDialogOpen(false)
    if (!active || !attachment || kind === "image") return
    if (!entityId) {
      setLoading(false)
      setNotFound(true)
      return
    }
    let cancelled = false

    setLoading(true)
    setNotFound(false)
    setDevice(null)
    setDeviceCurrent(null)
    setRule(null)
    setRuleExplain(null)
    setExplainStatus("idle")
    setRuleHistory([])
    setTransform(null)
    setAgent(null)
    setLatestExec(null)
    setAgentExecutions([])
    setDashboard(null)

    const load = async () => {
      try {
        if (kind === "device") {
          const [dev, cur] = await Promise.all([
            api.getDevice(entityId, { skipErrorToast: true }),
            api.getDeviceCurrent(entityId, { skipErrorToast: true }).catch(() => null),
          ])
          if (cancelled) return
          setDevice(dev)
          setDeviceCurrent(cur)
        } else if (kind === "rule") {
          const res = await api.getRule(entityId, { skipErrorToast: true })
          if (cancelled) return
          if (!res.rule) {
            setNotFound(true)
          } else {
            setRule(res.rule)
            setExplainStatus("loading")
            api
              .getRuleExplain(entityId, { skipErrorToast: true })
              .then((exp) => {
                if (cancelled) return
                setRuleExplain(exp)
                setExplainStatus("ready")
              })
              .catch(() => {
                if (cancelled) return
                setExplainStatus("error")
              })
            api
              .getRuleHistory(entityId, { skipErrorToast: true })
              .then((hist) => {
                if (cancelled) return
                setRuleHistory(hist?.executions ?? [])
              })
              .catch(() => {
                if (cancelled) return
                setRuleHistory([])
              })
          }
        } else if (kind === "transform") {
          const res = await api.getAutomation(entityId, { skipErrorToast: true })
          if (cancelled) return
          setTransform({
            automation: res.automation,
            definition:
              res.definition && typeof res.definition === "object"
                ? (res.definition as Record<string, unknown>)
                : null,
          })
        } else if (kind === "agent") {
          const [ag, exec, hist] = await Promise.all([
            api.getAgent(entityId, { skipErrorToast: true }),
            api.latestExecution(entityId, { skipErrorToast: true }).catch(() => null),
            api.getAgentExecutions(entityId, 5, { skipErrorToast: true }).catch(() => null),
          ])
          if (cancelled) return
          setAgent(ag)
          setLatestExec(exec)
          setAgentExecutions(hist?.executions ?? [])
        } else if (kind === "dashboard") {
          const dash = await api.getDashboard(entityId, { skipErrorToast: true })
          if (cancelled) return
          setDashboard(dash)
        }
      } catch {
        if (!cancelled) setNotFound(true)
      } finally {
        if (!cancelled) setLoading(false)
      }
    }

    void load()
    return () => {
      cancelled = true
    }
  }, [active, attachment, entityId, kind, refreshNonce])

  const activeEntityIdRef = useRef<string | undefined>(entityId)
  activeEntityIdRef.current = active ? entityId : undefined

  const handleOpenTransformTest = async () => {
    const expectedEntityId = entityId
    try {
      const res = await api.getDevices()
      if (activeEntityIdRef.current !== expectedEntityId) return
      setTestDevices(
        (res.devices ?? []).map((d) => ({
          id: d.id,
          name: d.name || d.id,
          device_type: d.device_type,
        }))
      )
    } catch {
      if (activeEntityIdRef.current !== expectedEntityId) return
      setTestDevices([])
    }
    if (activeEntityIdRef.current !== expectedEntityId) return
    setTestDialogOpen(true)
  }

  const handleAttach = () => {
    if (!onAttachReference || !entityId || !attachment || attachment.kind === "image") return
    onAttachReference({
      type: attachment.kind,
      id: entityId,
      label: attachment.title,
    })
    if (!keepOpenOnMention) {
      onClose()
    }
  }

  const handleNavigate = () => {
    if (
      !attachment?.deepLink ||
      !attachment.deepLink.startsWith("/") ||
      attachment.deepLink.startsWith("//")
    ) {
      return
    }
    onClose()
    navigate(attachment.deepLink)
  }

  const kindMeta = attachment ? getEntityKindMeta(attachment.kind) : null
  const kindLabel = kindMeta ? (isZh ? kindMeta.labelZh : kindMeta.labelEn) : ""

  const hasSafeDeepLink =
    !!attachment?.deepLink &&
    attachment.deepLink.startsWith("/") &&
    !attachment.deepLink.startsWith("//")

  const canAttach = !!(onAttachReference && entityId && !notFound)

  const footerButtons = (
    <>
      {canAttach && (
        <Button type="button" variant="outline" size="sm" onClick={handleAttach} className="gap-1.5">
          <AtSign className="h-3.5 w-3.5" />
          {isZh ? "引用到聊天" : "Mention"}
        </Button>
      )}
      {kind === "transform" && entityId && !notFound && (
        <Button
          type="button"
          variant="outline"
          size="sm"
          onClick={() => void handleOpenTransformTest()}
          className="gap-1.5"
        >
          <Play className="h-3.5 w-3.5" />
          {isZh ? "试运行" : "Test"}
        </Button>
      )}
      {hasSafeDeepLink && (
        <Button type="button" size="sm" onClick={handleNavigate} className="gap-1.5">
          {isZh ? `前往${kindLabel}页` : `Open ${kindLabel}s`}
          <ExternalLink className="h-3.5 w-3.5" />
        </Button>
      )}
    </>
  )

  const body = !attachment ? null : loading ? (
    <div className="flex items-center justify-center py-12 text-muted-foreground">
      <Loader2 className="h-5 w-5 animate-spin" />
    </div>
  ) : notFound ? (
    <div className="space-y-3">
      <div className="rounded-xl border border-dashed border-border bg-muted-20 px-4 py-3 text-xs leading-relaxed text-muted-foreground">
        {isZh
          ? "该对象当前已不存在或无权访问，以下为对话发生时的历史快照。"
          : "This entity is no longer available. Showing the snapshot captured during the conversation."}
      </div>
      <SnapshotKV snapshot={attachment.snapshot} />
    </div>
  ) : (
    <>
      {kind === "device" && device && (
        <DeviceInspectorView
          key={entityId}
          device={device}
          deviceCurrent={deviceCurrent}
          entityId={entityId}
          snapshotLastSeen={attachment.snapshot?.lastSeen}
          isZh={isZh}
          onAttach={canAttach ? handleAttach : undefined}
        />
      )}

      {kind === "rule" && rule && (
        <RuleInspectorView
          key={entityId}
          rule={rule}
          ruleExplain={ruleExplain}
          explainStatus={explainStatus}
          history={ruleHistory}
          entityId={entityId}
          isZh={isZh}
          onRefresh={refresh}
        />
      )}

      {kind === "transform" && transform && (
        <TransformInspectorView
          key={entityId}
          automation={transform.automation}
          definition={transform.definition}
          snapshotScope={attachment.snapshot?.scope}
          entityId={entityId}
          isZh={isZh}
        />
      )}

      {kind === "agent" && agent && (
        <AgentInspectorView
          key={entityId}
          agent={agent}
          latestExec={latestExec}
          executions={agentExecutions}
          fallbackStatusLabel={attachment.status?.label}
          entityId={entityId}
          isZh={isZh}
        />
      )}

      {kind === "dashboard" && dashboard && (
        <DashboardInspectorView
          key={entityId}
          dashboard={dashboard}
          entityId={entityId}
          isZh={isZh}
          onAttach={canAttach ? handleAttach : undefined}
        />
      )}
    </>
  )

  const testDialog =
    kind === "transform" && entityId && testDialogOpen ? (
      <TransformTestDialog
        open={testDialogOpen}
        onOpenChange={setTestDialogOpen}
        transformId={entityId}
        devices={testDevices}
      />
    ) : null

  return {
    loading,
    isZh,
    kindMeta,
    kindLabel,
    refresh,
    body,
    footerButtons,
    testDialog,
    testDialogOpen,
  }
}
