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

      {/* Scrollable content — every step centers vertically as one block:
          uniform treatment across all four steps beats a stable title
          anchor, per iteration with the design. Auto margins collapse when
          content overflows, degrading to top-aligned scrolling. Bottom
          padding exceeds the top so the centered block rides slightly above
          center — pure geometric centering reads as sitting too low. */}
      <div className="flex-1 overflow-y-auto">
        <div className="max-w-5xl mx-auto px-6 sm:px-10 min-h-full flex flex-col pt-8 sm:pt-10 pb-20 sm:pb-28">
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
              <ReadyStep status={status} onPromptNavigate={handlePromptNavigate} />
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

// ── Shared step header ──
// Every step opens with the same header block (icon + step counter + title +
// subtitle). Top-anchored and left-aligned so the title sits at the exact
// same spot on every step — centered headers shift around when switching,
// because each step's title/subtitle length differs.
function StepHeader({
  icon,
  tint,
  step,
  title,
  subtitle,
  badge,
}: {
  icon: React.ReactNode
  tint: string
  step: number
  title: string
  subtitle: string
  /** Inline chip next to the title (e.g. the setup steps' "Done" badge) —
      keeps completion state visible without spending a row of height. */
  badge?: React.ReactNode
}) {
  const { t } = useTranslation("common")

  return (
    <div className="flex items-center gap-4 sm:gap-5 mb-5 sm:mb-6">
      <div className={cn("w-14 h-14 rounded-2xl flex items-center justify-center shrink-0", tint)}>
        {icon}
      </div>
      <div className="min-w-0 flex-1">
        <p className="text-xs text-muted-foreground mb-1">
          {t("onboarding.stepIndicator", { current: step, total: STEPS.length })}
        </p>
        <div className="flex items-center flex-wrap gap-x-3 gap-y-1 mb-1">
          <h2 className="text-2xl font-bold text-foreground">{title}</h2>
          {badge}
        </div>
        <p className="text-sm text-muted-foreground leading-relaxed max-w-2xl">{subtitle}</p>
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
    <div>
      <StepHeader
        icon={<Rocket className="w-7 h-7 text-accent-indigo" />}
        tint="bg-accent-indigo-light"
        step={1}
        title={t("onboarding.setup.title")}
        subtitle={t("onboarding.setup.heroSubtitle")}
      />

      <div className="grid grid-cols-1 sm:grid-cols-3 gap-3">
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
  /** Icon / tint / title / purpose feed the step header; the feature list
      stays inside the card so the card isn't a title-less orphan. */
  icon: React.ReactNode
  tint: string
  title: string
  /** Bullet-list accent, matching the header tint (dot color per feature row). */
  accent: string
  features: { title: string; desc: string }[]
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

// Feature-row keys per setup item, mapping into
// onboarding.setup.<llm|device>.features.<key>.{title,desc}.
const LLM_FEATURE_KEYS = ["builtin", "local", "cloud"] as const
const DEVICE_FEATURE_KEYS = ["mqtt", "other", "camera"] as const

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
          icon: <Sparkles className="w-7 h-7" />,
          tint: "bg-accent-indigo-light text-accent-indigo",
          accent: "bg-accent-indigo",
          title: t("onboarding.setup.llm.title"),
          features: LLM_FEATURE_KEYS.map((k) => ({
            title: t(`onboarding.setup.llm.features.${k}.title`),
            desc: t(`onboarding.setup.llm.features.${k}.desc`),
          })),
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
          icon: <Cpu className="w-7 h-7" />,
          tint: "bg-accent-cyan-light text-accent-cyan",
          accent: "bg-accent-cyan",
          title: t("onboarding.setup.device.title"),
          features: DEVICE_FEATURE_KEYS.map((k) => ({
            title: t(`onboarding.setup.device.features.${k}.title`),
            desc: t(`onboarding.setup.device.features.${k}.desc`),
          })),
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
      <StepHeader
        icon={item.icon}
        tint={item.tint}
        step={which === "llm" ? 2 : 3}
        title={item.title}
        subtitle={item.purpose}
        badge={item.completed ? (
          <span className="inline-flex items-center gap-1.5 rounded-full bg-success-light px-2.5 py-1 text-xs font-medium text-success shrink-0">
            <Check className="w-3.5 h-3.5" />
            {item.completedLabel}
          </span>
        ) : undefined}
      />
      <SetupDetailPane item={item} />

      {which === "llm" && (
        <div className="mt-6 rounded-xl bg-muted-30 p-4">
          <p className="text-sm text-muted-foreground">{t("onboarding.setup.hint")}</p>
        </div>
      )}
    </div>
  )
}

// Detail pane: two equal columns — feature list/actions on the left,
// the CLI quick-start on the right. The completed state shows as a badge
// beside the StepHeader title (not a strip in here), so the card keeps a
// constant height and the actions stay reachable. The step title/purpose
// also live in the StepHeader above the card, not in here.
function SetupDetailPane({ item }: { item: SetupItem }) {
  return (
    <div className="rounded-2xl border border-border bg-card p-5 transition-colors">
      <div className="grid items-stretch gap-6 md:grid-cols-2">
        {/* Left: feature list + actions */}
        <div className="flex min-w-0 flex-col">
          <ul className="space-y-3">
            {item.features.map((f) => (
              <li key={f.title} className="flex items-start gap-2.5">
                <span className={cn("w-1.5 h-1.5 rounded-full mt-[7px] shrink-0", item.accent)} />
                <div className="min-w-0">
                  <p className="text-sm font-medium text-foreground">{f.title}</p>
                  <p className="text-xs text-muted-foreground mt-0.5 leading-relaxed">{f.desc}</p>
                </div>
              </li>
            ))}
          </ul>
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
}: {
  status: OnboardingStatus
  onPromptNavigate: (prompt: string) => void
}) {
  const { t } = useTranslation("common")
  const allComplete = status.steps.llm.completed && status.steps.device.completed

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
      {/* Step header — celebration or partial-state title, matching the
          other steps' hero block */}
      <StepHeader
        icon={allComplete ? <Check className="w-7 h-7" /> : <Sparkles className="w-7 h-7" />}
        tint={allComplete
          ? "bg-success text-primary-foreground"
          : "bg-accent-indigo-light text-accent-indigo"}
        step={4}
        title={allComplete ? t("onboarding.ready.allSetTitle") : t("onboarding.ready.partialTitle")}
        subtitle={allComplete ? t("onboarding.ready.allSetSubtitle") : t("onboarding.ready.partialSubtitle")}
      />

      {/* Prompt cards — each card hands off to chat with its prompt, so no
          extra CTA button is needed below; exiting is the footer's Finish. */}
      <div className="grid grid-cols-1 sm:grid-cols-3 gap-4">
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
    </div>
  )
}
