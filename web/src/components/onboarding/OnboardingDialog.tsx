/**
 * OnboardingDialog — Full-screen getting-started wizard
 *
 * Four steps, mapping 1:1 onto the progress stages:
 *   1. Welcome — platform intro + docs entry points
 *   2. LLM backend — configure the AI model (built-in download, custom
 *      backend, or CLI) with live completion status
 *   3. Devices  — connect/approve devices (UI action + webhook quick-start)
 *   4. Ready    — clickable prompt cards that hand off to chat via ?q=
 *
 * Freely browsable; the progress stages jump directly between steps, and
 * clicking Finish or Skip marks the guide as seen.
 */

import { useState, useEffect, useMemo, useRef } from "react"
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
  Terminal, Copy, BookOpen, ExternalLink, Download, AlertTriangle,
} from "lucide-react"
import { Button } from "@/components/ui/button"
import { Select, SelectTrigger, SelectValue, SelectContent, SelectItem } from "@/components/ui/select"
import { cn } from "@/lib/utils"
import { notifySuccess, notifyError } from "@/lib/notify"
import { useServerUrl, useServerLanReachable } from "@/lib/server-url"
import type { OnboardingStatus } from "@/hooks/useOnboarding"
import { copyToClipboard } from '@/lib/clipboard'

interface OnboardingDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  status: OnboardingStatus | null
  onDismiss: () => void
}

const STEPS = ["welcome", "llm", "device", "ready"] as const

type StepKey = (typeof STEPS)[number]

export function OnboardingDialog({ open, onOpenChange, status, onDismiss }: OnboardingDialogProps) {
  const { t } = useTranslation("common")
  const navigate = useNavigate()
  const openSettings = useStore((s) => s.openSettings)
  const [step, setStep] = useState<StepKey>("welcome")
  // Lifted to dialog level so the wizard survives step navigation
  // mid-download — only closing the dialog itself dismisses it.
  const [builtinWizardOpen, setBuiltinWizardOpen] = useState(false)

  const stepIndex = STEPS.indexOf(step)
  const isFirst = stepIndex === 0
  const isLast = stepIndex === STEPS.length - 1

  // Sync the PWA status-bar/safe-area color to the onboarding surface while
  // open (bg-bg-90 → near-opaque background), so the notch strip matches the
  // dialog body (see useThemeColor).
  useThemeColor("bg-90", open)

  // Land on the first incomplete step each time the dialog opens (Ready when
  // everything is done) — returning users skip straight to what's left. Users
  // who haven't configured the LLM yet start from the Welcome step, since
  // that's the top of the journey. Status is read through a ref so the 5s
  // status poll never re-triggers navigation and yank the user off their
  // current step.
  const statusRef = useRef(status)
  statusRef.current = status
  useEffect(() => {
    if (!open) return
    const s = statusRef.current
    setStep(
      !s || !s.steps.llm.completed ? "welcome"
        : !s.steps.device.completed ? "device"
        : "ready",
    )
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

      {/* Scrollable content — my-auto centers each step vertically on tall
          viewports and degrades to top-aligned scrolling when content
          overflows (auto margins don't clip like justify-center does). */}
      <div className="flex-1 overflow-y-auto">
        <div className="max-w-5xl mx-auto px-6 sm:px-10 min-h-full flex flex-col py-6 sm:py-8">
          <div className="my-auto w-full">
            {step === "welcome" && <WelcomeStep />}
            {step === "llm" && (
              <SetupStep
                which="llm"
                status={status}
                onAction={handleAction}
                onOpenBuiltinWizard={() => setBuiltinWizardOpen(true)}
              />
            )}
            {step === "device" && (
              <SetupStep which="device" status={status} onAction={handleAction} />
            )}
            {step === "ready" && (
              <ReadyStep status={status} onPromptNavigate={handlePromptNavigate} onStartChat={handleStartChat} />
            )}
          </div>
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

      {/* Kept mounted at dialog level so an in-progress download survives
          step navigation (see state comment above). */}
      <BuiltinModelWizard
        open={builtinWizardOpen}
        onOpenChange={setBuiltinWizardOpen}
        onActivated={() => setBuiltinWizardOpen(false)}
      />
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

// backend_type passes straight through to the API (cli-ops/src/llm.rs).
// Protocol-first story (matches the Settings Cloud AI card): local runners
// use their native type; every cloud vendor rides --type openai with its own
// endpoint. The runtime sniffs the endpoint for vendor-specific params
// (DashScope enable_thinking, DeepSeek thinking toggle), so this is
// functionally identical to the legacy vendor types.
const LLM_PROVIDERS: LlmProvider[] = [
  { id: "ollama", label: "Ollama", type: "ollama", endpoint: "http://localhost:11434", model: "qwen3.5:4b", needsKey: false },
  // Endpoint must NOT include /v1 — the llamacpp backend appends its own path.
  { id: "llamacpp", label: "llama.cpp", type: "llamacpp", endpoint: "http://127.0.0.1:8080", model: "qwen3.5-4b-q4_k_m", needsKey: false },
  // OpenAI-compatible endpoints must carry /v1 — the runtime joins
  // base + /chat/completions and only Anthropic auto-appends /v1.
  { id: "openai", label: "OpenAI", type: "openai", endpoint: "https://api.openai.com/v1", model: "gpt-4.1-mini", needsKey: true },
  { id: "anthropic", label: "Anthropic", type: "anthropic", endpoint: "https://api.anthropic.com", model: "claude-sonnet-4-5", needsKey: true },
  { id: "deepseek", label: "DeepSeek", type: "openai", endpoint: "https://api.deepseek.com/v1", model: "deepseek-chat", needsKey: true },
  { id: "glm", label: "GLM", type: "openai", endpoint: "https://open.bigmodel.cn/api/paas/v4", model: "glm-4.5-flash", needsKey: true },
  { id: "qwen", label: "Qwen", type: "openai", endpoint: "https://dashscope.aliyuncs.com/compatible-mode/v1", model: "qwen-plus", needsKey: true },
  { id: "xai", label: "xAI Grok", type: "openai", endpoint: "https://api.x.ai/v1", model: "grok-3-mini", needsKey: true },
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
      await copyToClipboard(command)
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
// POSTing telemetry to the webhook endpoint auto-discovers unregistered devices
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

  // Loopback in the displayed URL is unreachable for LAN devices — either the
  // canonical-URL prefetch hasn't resolved yet (Tauri/dev first paint) or no
  // LAN host was detectable. Flag it so users copying the command for a
  // device don't walk into a connection refused. Exact-hostname comparison —
  // a substring test would false-positive on domains like localhost.example.com.
  const isLocalhostUrl = (() => {
    try {
      const h = new URL(serverUrl).hostname.toLowerCase()
      return h === "localhost" || h === "127.0.0.1" || h === "::1" || h === "[::1]"
    } catch {
      return false
    }
  })()
  // URL is a real LAN address but the server only listens on loopback — the
  // address is right, the bind isn't. Teach the rebind instead.
  const lanReachable = useServerLanReachable()
  const notLanReachable = !isLocalhostUrl && lanReachable === false

  const handleCopy = async () => {
    try {
      await copyToClipboard(DEVICE_CURL_COMMAND)
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
      {isLocalhostUrl && (
        <p className="text-xs text-warning leading-relaxed flex items-start gap-1.5">
          <AlertTriangle className="w-4 h-4 shrink-0 mt-0.5" />
          {t("onboarding.deviceCli.localhostHint")}
        </p>
      )}
      {notLanReachable && (
        <p className="text-xs text-warning leading-relaxed flex items-start gap-1.5">
          <AlertTriangle className="w-4 h-4 shrink-0 mt-0.5" />
          {t("onboarding.deviceCli.unreachableHint")}
        </p>
      )}
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

// ── Step 1: Welcome — platform intro + docs entry points ──
// Kept as its own step (not folded into the LLM card) so the two setup steps
// share an identical structure, and the welcome moment gets a full screen.

const DOC_LINKS = [
  { labelKey: "onboarding.setup.docs.quickStart", href: "https://wiki.camthink.ai/docs/neomind/quick-start/five-minute-guide" },
  { labelKey: "onboarding.setup.docs.installSetup", href: "https://wiki.camthink.ai/docs/neomind/user-guide/install-setup" },
  { labelKey: "onboarding.setup.docs.developerGuide", href: "https://wiki.camthink.ai/docs/neomind/developer-guide/overview" },
]

function WelcomeStep() {
  const { t } = useTranslation("common")

  return (
    <div className="max-w-2xl mx-auto text-center">
      <div className="w-16 h-16 rounded-2xl bg-accent-indigo-light flex items-center justify-center mx-auto mb-5">
        <Rocket className="w-8 h-8 text-accent-indigo" />
      </div>
      <h2 className="text-2xl font-bold text-foreground mb-3">{t("onboarding.setup.title")}</h2>
      <p className="text-sm text-muted-foreground leading-relaxed mb-8">{t("onboarding.setup.heroSubtitle")}</p>

      <div className="grid grid-cols-1 sm:grid-cols-3 gap-3 text-left">
        {DOC_LINKS.map((doc) => (
          <a
            key={doc.href}
            href={doc.href}
            target="_blank"
            rel="noopener noreferrer"
            className="group rounded-xl border border-border bg-card p-4 hover:border-primary transition-colors"
          >
            <div className="flex items-center justify-between mb-2">
              <BookOpen className="w-4 h-4 text-muted-foreground" />
              <ExternalLink className="w-3.5 h-3.5 text-muted-foreground opacity-60 group-hover:opacity-100 transition-opacity" />
            </div>
            <span className="text-sm font-medium text-foreground">{t(doc.labelKey)}</span>
          </a>
        ))}
      </div>
    </div>
  )
}

// ── Steps 2 & 3: setup items (LLM / Devices), one wizard step each ──

interface SetupItem {
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
  which,
  status,
  onAction,
  onOpenBuiltinWizard,
}: {
  which: "llm" | "device"
  status: OnboardingStatus
  onAction: (path: string) => void
  onOpenBuiltinWizard?: () => void
}) {
  const { t } = useTranslation("common")
  const completedLabel = t("onboarding.completed")

  const item: SetupItem =
    which === "llm"
      ? {
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
          primaryAction: onOpenBuiltinWizard
            ? { label: t("common:llmGuide.builtinShort"), onClick: onOpenBuiltinWizard }
            : undefined,
        }
      : {
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
        }

  return (
    <div>
      <SetupDetailPane item={item} />

      {which === "llm" && (
        <div className="mt-6 rounded-xl bg-muted-30 p-4 text-center">
          <p className="text-sm text-muted-foreground">{t("onboarding.setup.hint")}</p>
        </div>
      )}
    </div>
  )
}

// Detail pane: two equal columns — intro/purpose/actions on the left,
// the CLI quick-start on the right. A completed item shows a success strip
// above the content instead of replacing it, so the actions stay reachable
// (e.g. adding a second backend or more devices).
function SetupDetailPane({ item }: { item: SetupItem }) {
  return (
    <div className="rounded-2xl border border-border bg-card p-5 transition-colors">
      {item.completed && (
        <div className="mb-4 flex items-center gap-1.5 rounded-xl bg-success-light px-3.5 py-2.5 text-xs font-medium text-success">
          <Check className="w-4 h-4 shrink-0" />
          {item.completedLabel}
        </div>
      )}
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

// ── Step 4: Ready — actionable prompt cards that hand off to chat ──

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
