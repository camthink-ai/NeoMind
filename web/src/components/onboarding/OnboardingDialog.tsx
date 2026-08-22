/**
 * OnboardingDialog — Full-screen getting-started wizard
 *
 * Two-step guide:
 *   1. Core setup (configure LLM, connect devices) — with completion status + CLI helpers
 *   2. Ready (clickable prompt cards that hand off to chat via ?q= URL param)
 *
 * Freely browsable; clicking Finish or Skip marks the guide as seen.
 */

import { useState, useEffect, useMemo } from "react"
import { createPortal } from "react-dom"
import { useTranslation } from "react-i18next"
import { useNavigate } from "react-router-dom"
import { useStore } from "@/store"
import { BuiltinModelWizard } from "@/components/llm/BuiltinModelWizard"
import { useThemeColor } from "@/hooks/useThemeColor"
import type { SettingsSection } from "@/store/types"
import {
  Rocket, Sparkles, Cpu, Check, X, ChevronLeft, ChevronRight,
  LayoutDashboard, Zap, Puzzle, MessageSquareText,
  Terminal, Copy, BookOpen, ExternalLink, Download,
} from "lucide-react"
import { Button } from "@/components/ui/button"
import { Select, SelectTrigger, SelectValue, SelectContent, SelectItem } from "@/components/ui/select"
import { cn } from "@/lib/utils"
import { notifySuccess, notifyError } from "@/lib/notify"
import { useServerUrl } from "@/lib/server-url"
import type { OnboardingStatus } from "@/hooks/useOnboarding"

interface OnboardingDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  status: OnboardingStatus | null
  onDismiss: () => void
}

const STEPS = ["setup", "ready"] as const

// Progress stages mirror the actual journey (model → devices → done), not
// the internal dialog steps — the user thinks in setup items, not wizard
// pages. Labels reuse the setup item titles / common:done.
const PROGRESS_STAGES = [
  { key: "llm" as const, icon: <Sparkles className="w-4 h-4" />, label: "onboarding.setup.llm.title" },
  { key: "device" as const, icon: <Cpu className="w-4 h-4" />, label: "onboarding.setup.device.title" },
  { key: "ready" as const, icon: <Rocket className="w-4 h-4" />, label: "done" },
]
type StepKey = (typeof STEPS)[number]

export function OnboardingDialog({ open, onOpenChange, status, onDismiss }: OnboardingDialogProps) {
  const { t } = useTranslation("common")
  const navigate = useNavigate()
  const openSettings = useStore((s) => s.openSettings)
  const [step, setStep] = useState<StepKey>("setup")

  const stepIndex = STEPS.indexOf(step)
  const isFirst = stepIndex === 0
  const isLast = stepIndex === STEPS.length - 1

  // Sync the PWA status-bar/safe-area color to the onboarding surface while
  // open (bg-bg-90 → near-opaque background), so the notch strip matches the
  // dialog body (see useThemeColor).
  useThemeColor("bg-90", open)

  // Reset to first step each time the dialog opens
  useEffect(() => {
    if (open) setStep("setup")
  }, [open])

  // Lock body scroll + Escape to close
  useEffect(() => {
    if (!open) return
    const prev = document.body.style.overflow
    document.body.style.overflow = "hidden"
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onOpenChange(false)
    }
    window.addEventListener("keydown", onKey)
    return () => {
      document.body.style.overflow = prev
      window.removeEventListener("keydown", onKey)
    }
  }, [open, onOpenChange])

  if (!open || !status) return null

  const handleAction = (path: string) => {
    onOpenChange(false)
    // Settings is now a full-screen dialog, not a route — open it on the tab.
    if (path.startsWith("/settings")) {
      const tab = path.includes("?tab=")
        ? (path.split("?tab=")[1] as SettingsSection)
        : undefined
      openSettings(tab)
    } else {
      navigate(path)
    }
  }

  const handleFinish = () => {
    onDismiss()
    onOpenChange(false)
  }

  const handlePromptNavigate = (prompt: string) => {
    onDismiss()
    onOpenChange(false)
    navigate(`/chat?q=${encodeURIComponent(prompt)}`)
  }

  const handleStartChat = () => {
    onDismiss()
    onOpenChange(false)
    navigate("/chat")
  }

  const root = typeof document !== "undefined"
    ? document.getElementById("dialog-root") || document.body
    : null
  if (!root) return null

  return createPortal(
    <div className="fixed inset-0 z-[100] flex flex-col bg-bg-90 backdrop-blur-xl" style={{ paddingTop: "calc(env(safe-area-inset-top, 0px) + var(--titlebar-inset, 0px))" }}>
      {/* Close button */}
      <button
        onClick={() => onOpenChange(false)}
        className="absolute top-4 right-4 z-10 w-9 h-9 rounded-lg flex items-center justify-center text-muted-foreground hover:bg-muted-30 transition-colors"
        aria-label={t("onboarding.dismiss")}
      >
        <X className="w-5 h-5" />
      </button>

      {/* Progress indicator — three real stages with state colors and
          connecting lines that fill as stages complete. */}
      <div className="shrink-0 pt-8 pb-3 px-6">
        <div className="max-w-5xl mx-auto flex items-center justify-center">
          {(() => {
            // Active stage: within setup, the first incomplete item; on the
            // ready step, the Done stage.
            const firstIncomplete = !status.steps.llm.completed
              ? "llm"
              : !status.steps.device.completed
                ? "device"
                : "ready"
            return PROGRESS_STAGES.map((stage, i) => {
              const completed =
                stage.key === "llm"
                  ? !!status.steps.llm.completed
                  : stage.key === "device"
                    ? !!status.steps.device.completed
                    : false
              const active = step === "ready" ? stage.key === "ready" : stage.key === firstIncomplete
              return (
                <button
                  key={stage.key}
                  onClick={() => setStep(stage.key === "ready" ? "ready" : "setup")}
                  className="flex items-center"
                  aria-label={t(stage.label)}
                >
                  <span className={cn(
                    "flex items-center justify-center w-8 h-8 rounded-full transition-all duration-300",
                    completed && "bg-success text-primary-foreground",
                    !completed && active && "bg-primary text-primary-foreground ring-4 ring-primary-light",
                    !completed && !active && "bg-muted-30 text-muted-foreground",
                  )}>
                    {completed ? <Check className="w-4 h-4" /> : stage.icon}
                  </span>
                  <span className={cn(
                    "ml-2 text-xs font-medium hidden sm:inline transition-colors",
                    active ? "text-foreground" : completed ? "text-success" : "text-muted-foreground",
                  )}>
                    {t(stage.label)}
                  </span>
                  {i < PROGRESS_STAGES.length - 1 && (
                    <span className={cn(
                      "w-6 sm:w-12 h-0.5 rounded-full mx-3 transition-colors duration-500",
                      completed ? "bg-success" : "bg-border",
                    )} />
                  )}
                </button>
              )
            })
          })()}
        </div>
      </div>

      {/* Scrollable content */}
      <div className="flex-1 overflow-y-auto">
        <div className="max-w-5xl mx-auto px-6 sm:px-10 py-6 sm:py-8">
          {step === "setup" && <SetupStep open={open} status={status} onAction={handleAction} />}
          {step === "ready" && <ReadyStep status={status} onPromptNavigate={handlePromptNavigate} onStartChat={handleStartChat} />}
        </div>
      </div>

      {/* Footer navigation */}
      <div className="shrink-0 border-t border-border bg-bg-95">
        <div className="max-w-5xl mx-auto px-6 py-3 flex items-center justify-between">
          <Button variant="ghost" size="sm" onClick={handleFinish} className="text-muted-foreground">
            {t("onboarding.dismiss")}
          </Button>
          <div className="flex items-center gap-2">
            {!isFirst && (
              <Button variant="outline" size="sm" onClick={() => setStep(STEPS[stepIndex - 1])}>
                <ChevronLeft className="w-4 h-4 mr-1" />
                {t("onboarding.nav.prev")}
              </Button>
            )}
            {isLast ? (
              <Button size="sm" onClick={handleFinish}>
                {t("onboarding.nav.finish")}
                <Check className="w-4 h-4 ml-1.5" />
              </Button>
            ) : (
              <Button size="sm" onClick={() => setStep(STEPS[stepIndex + 1])}>
                {t("onboarding.nav.next")}
                <ChevronRight className="w-4 h-4 ml-1.5" />
              </Button>
            )}
          </div>
        </div>
      </div>
    </div>,
    root,
  )
}

// ── LLM CLI quick-setup helper ──

interface LlmProvider {
  id: string
  label: string
  type: string
  endpoint: string
  model: string
  needsKey: boolean
}

// backend_type passes straight through to the API (cli-ops/src/llm.rs),
// so each provider's native type works. Endpoints from the user-guide README.
const LLM_PROVIDERS: LlmProvider[] = [
  { id: "ollama", label: "Ollama", type: "ollama", endpoint: "http://localhost:11434", model: "qwen3.5:4b", needsKey: false },
  // Endpoint must NOT include /v1 — the llamacpp backend appends its own path.
  { id: "llamacpp", label: "llama.cpp", type: "llamacpp", endpoint: "http://127.0.0.1:8080", model: "qwen3.5-4b-q4_k_m", needsKey: false },
  { id: "openai", label: "OpenAI", type: "openai", endpoint: "https://api.openai.com/v1", model: "gpt-4.1-mini", needsKey: true },
  { id: "anthropic", label: "Anthropic", type: "anthropic", endpoint: "https://api.anthropic.com", model: "claude-sonnet-4-5", needsKey: true },
  { id: "deepseek", label: "DeepSeek", type: "deepseek", endpoint: "https://api.deepseek.com", model: "deepseek-chat", needsKey: true },
  { id: "glm", label: "GLM", type: "glm", endpoint: "https://open.bigmodel.cn/api/paas/v4", model: "glm-4.5-flash", needsKey: true },
  { id: "qwen", label: "Qwen", type: "qwen", endpoint: "https://dashscope.aliyuncs.com/compatible-mode/v1", model: "qwen-plus", needsKey: true },
  { id: "xai", label: "xAI Grok", type: "xai", endpoint: "https://api.x.ai", model: "grok-3-mini", needsKey: true },
]

function buildLlmCommand(p: LlmProvider): string {
  const lines: string[] = []
  if (p.id === "ollama") lines.push(`ollama pull ${p.model}`)
  if (p.id === "llamacpp") lines.push(`llama-server -m ${p.model}.gguf -c 32768 --port 8080`)
  const parts = [
    "neomind llm create",
    `--name ${p.id}`,
    `--type ${p.type}`,
    `--endpoint ${p.endpoint}`,
    `--model ${p.model}`,
  ]
  if (p.needsKey) parts.push("--api-key YOUR_API_KEY")
  lines.push(parts.join(" \\\n  "))
  return lines.join("\n")
}

// Verify + set-default, run after `create` returns a backend ID.
const FOLLOWUP_COMMANDS = "neomind llm test <ID>\nneomind llm activate <ID>"

function LlmCliHelper() {
  const { t } = useTranslation("common")
  const [providerId, setProviderId] = useState("ollama")
  const provider = LLM_PROVIDERS.find((p) => p.id === providerId) ?? LLM_PROVIDERS[0]
  const command = useMemo(() => buildLlmCommand(provider), [provider])

  const handleCopy = async () => {
    try {
      await navigator.clipboard.writeText(command)
      notifySuccess(t("onboarding.cli.copied"))
    } catch {
      notifyError(t("onboarding.cli.copyFailed"))
    }
  }

  return (
    <div className="space-y-3">
      <div className="flex items-center gap-2 flex-wrap">
        <Terminal className="w-4 h-4 text-muted-foreground" />
        <span className="text-xs text-muted-foreground">{t("onboarding.cli.provider")}</span>
        <Select value={providerId} onValueChange={setProviderId}>
          <SelectTrigger className="h-8 w-auto min-w-[140px] text-xs">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {LLM_PROVIDERS.map((p) => (
              <SelectItem key={p.id} value={p.id}>{p.label}</SelectItem>
            ))}
          </SelectContent>
        </Select>
        {provider.needsKey && (
          <span className="text-xs text-muted-foreground">{t("onboarding.cli.keyHint")}</span>
        )}
      </div>
      <pre className="text-xs font-mono bg-background border border-border rounded-lg p-3 overflow-x-auto text-foreground whitespace-pre leading-relaxed">
        {command}
      </pre>
      <Button size="sm" variant="outline" onClick={handleCopy} className="gap-1.5">
        <Copy className="w-3.5 h-3.5" />
        {t("onboarding.cli.copy")}
      </Button>
      <div className="rounded-lg bg-muted-30 p-3">
        <p className="text-xs text-muted-foreground mb-1.5 leading-relaxed">
          {t("onboarding.cli.followup")}
        </p>
        <pre className="text-xs font-mono text-muted-foreground whitespace-pre-wrap break-all leading-relaxed">
          {FOLLOWUP_COMMANDS}
        </pre>
      </div>
    </div>
  )
}

// ── Device CLI quick-start helper ──
// POSTing telemetry to the webhook endpoint auto-disovers unregistered devices
// (webhook.rs:343 emits DeviceDiscovered for unknown device IDs). This gives a
// pure-curl closed loop: publish → draft created → approve → device registered.

// After the webhook creates a draft, these commands view and approve it.
const DEVICE_FOLLOWUP_COMMANDS = [
  "neomind device drafts list",
  'neomind device drafts approve demo-001 --name "Demo Sensor" --type sensor',
].join("\n")

function DeviceQuickStart() {
  const { t } = useTranslation("common")
  const serverUrl = useServerUrl()

  // Build curl command dynamically using canonical server URL
  const DEVICE_CURL_COMMAND = useMemo(() => [
    `curl -X POST ${serverUrl}/api/devices/demo-001/webhook \\`,
    '  -H "Content-Type: application/json" \\',
    `  -d '{"data": {"temperature": 25.5, "humidity": 60}}'`,
  ].join("\n"), [serverUrl])

  const handleCopy = async () => {
    try {
      await navigator.clipboard.writeText(DEVICE_CURL_COMMAND)
      notifySuccess(t("onboarding.cli.copied"))
    } catch {
      notifyError(t("onboarding.cli.copyFailed"))
    }
  }

  return (
    <div className="space-y-3">
      <p className="text-xs text-muted-foreground leading-relaxed flex items-center gap-1.5">
        <Terminal className="w-4 h-4 text-muted-foreground shrink-0" />
        {t("onboarding.deviceCli.note")}
      </p>
      <pre className="text-xs font-mono bg-background border border-border rounded-lg p-3 overflow-x-auto text-foreground whitespace-pre leading-relaxed">
        {DEVICE_CURL_COMMAND}
      </pre>
      <Button size="sm" variant="outline" onClick={handleCopy} className="gap-1.5">
        <Copy className="w-3.5 h-3.5" />
        {t("onboarding.cli.copy")}
      </Button>
      <div className="rounded-lg bg-muted-30 p-3">
        <p className="text-xs text-muted-foreground mb-1.5 leading-relaxed">
          {t("onboarding.deviceCli.followup")}
        </p>
        <pre className="text-xs font-mono text-muted-foreground whitespace-pre-wrap break-all leading-relaxed">
          {DEVICE_FOLLOWUP_COMMANDS}
        </pre>
      </div>
    </div>
  )
}

// ── Step 1: Core setup (master-detail layout) ──

type SetupCardId = "llm" | "device"

interface SetupItem {
  id: SetupCardId
  icon: React.ReactNode
  tint: string
  title: string
  description: string
  purpose: string
  completed: boolean
  completedLabel: string
  actionLabel: string
  onAction: () => void
  extra: React.ReactNode
  /** Optional primary CTA shown before the secondary `actionLabel` button
      (e.g. the built-in model download in the LLM card). */
  primaryAction?: { label: string; onClick: () => void }
}

function SetupStep({
  open,
  status,
  onAction,
}: {
  open: boolean
  status: OnboardingStatus
  onAction: (path: string) => void
}) {
  const { t } = useTranslation("common")
  const completedLabel = t("onboarding.completed")

  // First incomplete card wins; fall back to LLM when both done.
  const defaultSelected: SetupCardId = !status.steps.llm.completed
    ? "llm"
    : !status.steps.device.completed
      ? "device"
      : "llm"

  const [selected, setSelected] = useState<SetupCardId>(defaultSelected)
  const [builtinWizardOpen, setBuiltinWizardOpen] = useState(false)

  // Re-derive selection when the dialog opens (preserves manual selection while open).
  useEffect(() => {
    if (open) setSelected(defaultSelected)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open])

  const items: SetupItem[] = [
    {
      id: "llm",
      icon: <Sparkles className="w-5 h-5" />,
      tint: "bg-accent-indigo-light text-accent-indigo",
      title: t("onboarding.setup.llm.title"),
      description: t("onboarding.setup.llm.description"),
      purpose: t("onboarding.setup.llm.purpose"),
      completed: status.steps.llm.completed,
      completedLabel,
      actionLabel: t("onboarding.setup.llm.action"),
      onAction: () => onAction("/settings?tab=llm"),
      extra: <LlmCliHelper />,
      primaryAction: {
        label: t("common:llmGuide.builtinShort"),
        onClick: () => setBuiltinWizardOpen(true),
      },
    },
    {
      id: "device",
      icon: <Cpu className="w-5 h-5" />,
      tint: "bg-accent-cyan-light text-accent-cyan",
      title: t("onboarding.setup.device.title"),
      description: t("onboarding.setup.device.description"),
      purpose: t("onboarding.setup.device.purpose"),
      completed: status.steps.device.completed,
      completedLabel,
      actionLabel: t("onboarding.setup.device.action"),
      // Land on the pending-registration tab — the step is about approving
      // auto-discovered devices, and the general list starts empty for a
      // fresh install.
      onAction: () => onAction("/devices/drafts"),
      extra: <DeviceQuickStart />,
    },
  ]

  const active = items.find((i) => i.id === selected) ?? items[0]

  return (
    <div>
      <div className="mb-6">
        <div className="flex items-center gap-3 mb-2">
          <div className="w-10 h-10 rounded-xl bg-accent-indigo-light flex items-center justify-center shrink-0">
            <Rocket className="w-5 h-5 text-accent-indigo" />
          </div>
          <h2 className="text-lg font-bold text-foreground">{t("onboarding.setup.title")}</h2>
        </div>
        <p className="text-sm text-muted-foreground leading-relaxed">{t("onboarding.setup.heroSubtitle")}</p>
      </div>

      {/* Docs strip — fixed above the variable-height grid so it never shifts */}
      <div className="mb-4 rounded-lg bg-muted-30 px-4 py-2.5 flex items-center gap-4 flex-wrap">
        <BookOpen className="w-3.5 h-3.5 text-muted-foreground shrink-0" />
        {[
          { label: t("onboarding.setup.docs.quickStart"), href: "https://wiki.camthink.ai/docs/neomind/quick-start/five-minute-guide" },
          { label: t("onboarding.setup.docs.installSetup"), href: "https://wiki.camthink.ai/docs/neomind/user-guide/install-setup" },
          { label: t("onboarding.setup.docs.developerGuide"), href: "https://wiki.camthink.ai/docs/neomind/developer-guide/overview" },
        ].map((doc) => (
          <a
            key={doc.href}
            href={doc.href}
            target="_blank"
            rel="noopener noreferrer"
            className="text-xs text-primary hover:underline inline-flex items-center gap-1"
          >
            {doc.label}
            <ExternalLink className="w-3 h-3" />
          </a>
        ))}
      </div>

      <div className="mb-6">
        <SetupTopTabs items={items} selectedId={selected} onSelect={setSelected} />
        <SetupDetailPane item={active} />
      </div>

      {/* Hint */}
      <div className="rounded-xl bg-muted-30 p-4 text-center">
        <p className="text-sm text-muted-foreground">{t("onboarding.setup.hint")}</p>
      </div>
      <BuiltinModelWizard
        open={builtinWizardOpen}
        onOpenChange={setBuiltinWizardOpen}
        onActivated={() => setBuiltinWizardOpen(false)}
      />
    </div>
  )
}

// Top tab strip — app-standard capsule style (same shape as PageTabsBar),
// replacing the former left vertical list so the detail pane gets full
// width. Completed steps swap their icon for a check (green when idle).
function SetupTopTabs({
  items,
  selectedId,
  onSelect,
}: {
  items: SetupItem[]
  selectedId: SetupCardId
  onSelect: (id: SetupCardId) => void
}) {
  return (
    <div className="mb-4 flex items-center gap-1 rounded-lg border border-border bg-card p-1">
      {items.map((item) => {
        const isActive = item.id === selectedId
        return (
          <button
            key={item.id}
            type="button"
            onClick={() => onSelect(item.id)}
            className={cn(
              "inline-flex h-9 flex-1 items-center justify-center gap-2 rounded-sm px-4 text-sm font-medium whitespace-nowrap transition-all md:max-w-[15rem]",
              isActive
                ? "bg-foreground text-background shadow-sm"
                : "text-muted-foreground hover:text-foreground",
              item.completed && !isActive && "text-success hover:text-success",
            )}
          >
            <span className="h-4 w-4 shrink-0">
              {item.completed ? <Check className="h-4 w-4" /> : item.icon}
            </span>
            <span>{item.title}</span>
          </button>
        )
      })}
    </div>
  )
}

// Detail pane: two equal columns — intro/purpose/actions on the left,
// the CLI quick-start on the right — so full-width doesn't leave a void.
function SetupDetailPane({ item }: { item: SetupItem }) {
  if (item.completed) {
    return (
      <div className="flex items-center gap-1.5 rounded-2xl border border-success bg-success-light p-5 text-xs font-medium text-success">
        <Check className="w-4 h-4" />
        {item.completedLabel}
      </div>
    )
  }

  return (
    <div className="rounded-2xl border border-border bg-card p-5 transition-colors">
      <div className="grid items-stretch gap-6 md:grid-cols-2">
        {/* Left: intro + purpose + actions */}
        <div className="flex min-w-0 flex-col">
          <div className="flex items-start gap-3 mb-3">
            <div className={cn(
              "w-10 h-10 rounded-xl flex items-center justify-center shrink-0",
              item.tint,
            )}>
              {item.icon}
            </div>
            <div className="flex-1 min-w-0">
              <h3 className="font-semibold text-sm">
                {item.title}
              </h3>
              <p className="text-xs text-muted-foreground mt-1 leading-relaxed">{item.description}</p>
            </div>
          </div>
          <p className="text-xs text-muted-foreground mb-4 leading-relaxed">{item.purpose}</p>
          <div className="mt-auto pt-4 flex flex-wrap justify-end gap-2">
            {item.primaryAction && (
              <Button size="sm" onClick={item.primaryAction.onClick} className="gap-1.5">
                <Download className="w-3.5 h-3.5" />
                {item.primaryAction.label}
              </Button>
            )}
            <Button
              size="sm"
              variant={item.primaryAction ? "secondary" : "default"}
              onClick={item.onAction}
              className="gap-1.5"
            >
              {item.actionLabel}
              <ChevronRight className="w-3.5 h-3.5" />
            </Button>
          </div>
        </div>

        {/* Right: quick-start (CLI helper / curl) */}
        <div className="min-w-0">{item.extra}</div>
      </div>
    </div>
  )
}

// ── Step 2: Ready — actionable prompt cards that hand off to chat ──

function ReadyStep({
  status,
  onPromptNavigate,
  onStartChat,
}: {
  status: OnboardingStatus
  onPromptNavigate: (prompt: string) => void
  onStartChat: () => void
}) {
  const { t } = useTranslation("common")
  const allComplete = status.steps.llm.completed && status.steps.device.completed

  const statusItems = [
    { key: "llm", completed: status.steps.llm.completed },
    { key: "device", completed: status.steps.device.completed },
  ] as const

  const cards = [
    {
      icon: <LayoutDashboard className="w-5 h-5" />,
      key: "monitoring",
      tint: "bg-accent-purple-light text-accent-purple",
    },
    {
      icon: <Zap className="w-5 h-5" />,
      key: "automation",
      tint: "bg-accent-orange-light text-accent-orange",
    },
    {
      icon: <Puzzle className="w-5 h-5" />,
      key: "extensions",
      tint: "bg-accent-cyan-light text-accent-cyan",
    },
  ]

  return (
    <div>
      {/* Header — celebration banner when all complete */}
      <div className={cn(
        "rounded-2xl p-5 mb-6",
        allComplete ? "bg-success-light" : "bg-card border border-border",
      )}>
        <div className="flex items-center gap-3 mb-2">
          <div className={cn(
            "w-10 h-10 rounded-xl flex items-center justify-center shrink-0",
            allComplete ? "bg-success text-primary-foreground" : "bg-accent-indigo-light text-accent-indigo",
          )}>
            {allComplete ? <Check className="w-5 h-5" /> : <Sparkles className="w-5 h-5" />}
          </div>
          <h2 className="text-lg font-bold text-foreground">
            {allComplete ? t("onboarding.ready.allSetTitle") : t("onboarding.ready.partialTitle")}
          </h2>
        </div>
        <p className="text-sm text-muted-foreground leading-relaxed mb-3">
          {allComplete ? t("onboarding.ready.allSetSubtitle") : t("onboarding.ready.partialSubtitle")}
        </p>
        {/* Status summary chips */}
        <div className="flex items-center gap-2 flex-wrap">
          {statusItems.map((item) => (
            <div
              key={item.key}
              className={cn(
                "inline-flex items-center gap-1.5 px-2.5 py-1 rounded-full text-xs font-medium",
                item.completed
                  ? allComplete
                    ? "bg-card text-success"
                    : "bg-success-light text-success"
                  : "bg-muted-30 text-muted-foreground",
              )}
            >
              {item.completed ? (
                <Check className="w-3.5 h-3.5" />
              ) : (
                <span className="w-2.5 h-2.5 rounded-full border-2 border-current opacity-40" />
              )}
              {t(`onboarding.ready.statusLabels.${item.key}`)}
            </div>
          ))}
        </div>
      </div>

      {/* Prompt cards */}
      <div className="grid grid-cols-1 sm:grid-cols-3 gap-4 mb-6">
        {cards.map((c) => (
          <button
            key={c.key}
            type="button"
            onClick={() => onPromptNavigate(t(`onboarding.ready.prompts.${c.key}.prompt`))}
            className="group text-left rounded-2xl border border-border bg-card p-5 flex flex-col h-full hover:border-primary transition-colors"
          >
            <div className={cn("w-10 h-10 rounded-xl flex items-center justify-center mb-3 shrink-0", c.tint)}>
              {c.icon}
            </div>
            <h3 className="font-semibold text-sm text-foreground mb-1.5 shrink-0">
              {t(`onboarding.ready.prompts.${c.key}.title`)}
            </h3>
            <p className="text-xs text-muted-foreground leading-relaxed mb-3">
              {t(`onboarding.ready.prompts.${c.key}.desc`)}
            </p>
            <div className="mt-auto flex items-start gap-1.5 rounded-lg bg-muted-30 px-3 py-2 shrink-0 group-hover:bg-muted-50 transition-colors">
              <MessageSquareText className="w-3.5 h-3.5 text-muted-foreground shrink-0 mt-0.5" />
              <span className="text-xs text-muted-foreground italic leading-relaxed">
                {t(`onboarding.ready.prompts.${c.key}.prompt`)}
              </span>
            </div>
          </button>
        ))}
      </div>

      <div className="flex justify-center">
        <Button size="lg" onClick={onStartChat} className="gap-2">
          <MessageSquareText className="w-4 h-4" />
          {t("onboarding.ready.chatButton")}
        </Button>
      </div>
    </div>
  )
}
