import { useMemo, useState, type ReactNode } from "react"
import { useNavigate } from "react-router-dom"
import { useTranslation } from "react-i18next"
import { ChevronRight } from "lucide-react"
import { useStore } from "@/store"
import { version as appVersion } from "../../package.json"
import { MobilePageHeader } from "@/components/layout/MobilePageHeader"
import { InstanceManagerDialog } from "@/components/instances/InstanceManagerDialog"
import { OnboardingDialog } from "@/components/onboarding/OnboardingDialog"
import { useOnboarding } from "@/hooks/useOnboarding"
import { useExtensionPages } from "@/hooks/useExtensionPages"
import { useTheme } from "@/components/ui/theme"
import { cn } from "@/lib/utils"

interface MoreRowProps {
  title: ReactNode
  description?: ReactNode
  trailing?: ReactNode
  onClick?: () => void
  destructive?: boolean
}

function MoreRow({ title, description, trailing, onClick, destructive }: MoreRowProps) {
  const Component = onClick ? "button" : "div"
  return (
    <Component
      type={onClick ? "button" : undefined}
      onClick={onClick}
      className={cn(
        "flex min-h-[3.75rem] w-full items-center gap-3 rounded-xl px-3 py-3 text-left transition-[background-color,transform]",
        onClick && "active:scale-[0.99] active:bg-muted",
      )}
    >
      <span className="min-w-0 flex-1">
        <span className={cn("block truncate text-[15px] font-medium", destructive ? "text-error" : "text-foreground")}>
          {title}
        </span>
        {description && <span className="mt-0.5 block truncate text-xs text-muted-foreground">{description}</span>}
      </span>
      {trailing ?? (onClick ? <ChevronRight className="h-4 w-4 shrink-0 text-muted-foreground" aria-hidden="true" /> : null)}
    </Component>
  )
}

function Section({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section className="mt-7 first:mt-0">
      <h2 className="mb-2 ml-1 text-xs font-medium text-muted-foreground">{title}</h2>
      <div className="glass overflow-hidden rounded-2xl p-1">{children}</div>
    </section>
  )
}

export default function SystemPage() {
  const { t, i18n } = useTranslation("common")
  const navigate = useNavigate()
  const user = useStore((s) => s.user)
  const devices = useStore((s) => s.devices)
  const alerts = useStore((s) => s.alerts)
  const wsConnected = useStore((s) => s.wsConnected)
  const openSettings = useStore((s) => s.openSettings)
  const logout = useStore((s) => s.logout)
  const { pages: extensionPages } = useExtensionPages()
  const { theme, setTheme } = useTheme()
  const { status: onboardingStatus, dismiss: dismissOnboarding } = useOnboarding()
  const [instanceManagerOpen, setInstanceManagerOpen] = useState(false)
  const [onboardingOpen, setOnboardingOpen] = useState(false)

  const onlineCount = useMemo(() => devices.filter((device) => device.online).length, [devices])
  const unreadCount = useMemo(
    () => alerts.filter((alert) => !alert.acknowledged && alert.status !== "resolved" && alert.status !== "acknowledged").length,
    [alerts],
  )
  const contributedPages = extensionPages.filter((page) => page.nav_section && page.nav_section !== "add-device")
  const nextTheme = theme === "light" ? "dark" : theme === "dark" ? "system" : "light"
  const themeLabel = theme === "light"
    ? t("common:theme.light")
    : theme === "dark"
      ? t("common:theme.dark")
      : t("common:theme.system")

  return (
    <div className="flex h-full flex-col bg-background">
      <MobilePageHeader title={t("common:system.pageTitle")} />
      <div className="flex-1 overflow-y-auto overscroll-contain">
        <div className="mx-auto w-full max-w-2xl px-5 pb-10 pt-4">
          <button
            type="button"
            onClick={() => setInstanceManagerOpen(true)}
            className="glass flex min-h-20 w-full items-center rounded-2xl px-4 py-4 text-left transition-transform active:scale-[0.99]"
          >
            <span className="min-w-0 flex-1">
              <span className="flex items-center gap-2 text-base font-semibold text-foreground">
                {t("common:system.localInstance")}
                <span className={cn("h-2 w-2 rounded-full", wsConnected ? "bg-success" : "bg-error")} aria-hidden="true" />
                <span className="text-sm font-normal text-muted-foreground">
                  {wsConnected ? t("common:system.connected") : t("common:system.disconnected")}
                </span>
              </span>
              <span className="mt-1 block text-sm text-muted-foreground">
                {t("common:system.mobileSummary", {
                  online: onlineCount,
                  total: devices.length,
                  alerts: unreadCount,
                })}
              </span>
            </span>
            <ChevronRight className="h-5 w-5 shrink-0 text-muted-foreground" aria-hidden="true" />
          </button>

          <Section title={t("common:system.sectionBusiness")}>
            <MoreRow title={t("common:nav.visual-dashboard")} description={t("common:system.cards.dashboard")} onClick={() => navigate("/visual-dashboard")} />
            <MoreRow title={t("common:nav.automation")} description={t("common:system.cards.automation")} onClick={() => navigate("/automation")} />
            <MoreRow title={t("common:nav.data")} description={t("common:system.cards.data")} onClick={() => navigate("/data")} />
            <MoreRow
              title={t("common:nav.messages")}
              description={t("common:system.cards.messages")}
              trailing={unreadCount > 0 ? <span className="rounded-full bg-muted px-2 py-0.5 text-xs tabular-nums">{unreadCount > 99 ? "99+" : unreadCount}</span> : undefined}
              onClick={() => navigate("/messages")}
            />
          </Section>

          <Section title={t("common:system.sectionSystem")}>
            <MoreRow title={t("common:nav.extensions")} description={t("common:system.cards.extensions")} onClick={() => navigate("/extensions")} />
            <MoreRow title={t("common:nav.settings")} description={t("common:system.cards.settings")} onClick={() => openSettings()} />
            <MoreRow title={t("common:system.instanceManager")} description={t("common:system.cards.instance")} onClick={() => setInstanceManagerOpen(true)} />
            {contributedPages.map((page) => (
              <MoreRow
                key={`${page.extension_id}-${page.type}`}
                title={page.nav_label ?? page.title}
                description={t("common:system.extensionPage")}
                trailing={<span className="flex items-center gap-2 text-xs text-muted-foreground">{t("common:nav.extensions")}<ChevronRight className="h-4 w-4" /></span>}
                onClick={() => navigate(page.route)}
              />
            ))}
          </Section>

          <Section title={t("common:system.preferences")}>
            {user && (
              <MoreRow
                title={user.username}
                description={user.role || t("common:system.account")}
                onClick={() => openSettings("preferences")}
              />
            )}
            <MoreRow
              title={t("common:system.language")}
              trailing={<span className="text-sm text-muted-foreground">{i18n.language === "zh" ? "中文" : "English"}</span>}
              onClick={() => i18n.changeLanguage(i18n.language === "zh" ? "en" : "zh")}
            />
            <MoreRow
              title={t("common:theme.title")}
              trailing={<span className="text-sm text-muted-foreground">{themeLabel}</span>}
              onClick={() => setTheme(nextTheme)}
            />
            <MoreRow title={t("common:onboarding.title")} onClick={() => setOnboardingOpen(true)} />
            <MoreRow title={t("common:system.about")} description={t("common:system.version", { version: appVersion })} onClick={() => openSettings("about")} />
            <MoreRow title={t("logout")} destructive onClick={() => logout()} />
          </Section>
        </div>
      </div>

      <InstanceManagerDialog open={instanceManagerOpen} onOpenChange={setInstanceManagerOpen} />
      <OnboardingDialog
        open={onboardingOpen}
        onOpenChange={setOnboardingOpen}
        status={onboardingStatus}
        onDismiss={dismissOnboarding}
      />
    </div>
  )
}
