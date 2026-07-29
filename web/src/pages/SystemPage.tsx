/**
 * SystemPage - iOS-Settings-style container page (mobile-only entry point)
 *
 * Aggregates demoted routes (automation/data/messages/extensions/settings) and
 * global actions (theme/language/instance/health/onboarding/help/about/logout)
 * into grouped card lists.
 *
 * Renders on desktop too via /system route for deep-link safety, but is
 * primarily designed for mobile where the bottom 系统 tab routes here.
 */

import { useState, useMemo, type ReactNode } from "react"
import { useNavigate } from "react-router-dom"
import { useTranslation } from "react-i18next"
import {
  Workflow,
  Database,
  Bell,
  Puzzle,
  Settings,
  Palette,
  Languages,
  Server,
  Activity,
  Rocket,
  HelpCircle,
  LogOut,
  ChevronRight,
  type LucideIcon,
} from "lucide-react"
import { useStore } from "@/store"
import { cn } from "@/lib/utils"
import { Avatar, AvatarFallback } from "@/components/ui/avatar"
import { Badge } from "@/components/ui/badge"
import { BrandLogoWithName } from "@/components/shared/BrandName"
import { ThemeToggle } from "@/components/layout/ThemeToggle"
import { SystemHealthButton } from "@/components/layout/SystemHealthButton"
import { InstanceManagerDialog } from "@/components/instances/InstanceManagerDialog"
import { OnboardingDialog } from "@/components/onboarding/OnboardingDialog"
import { useOnboarding } from "@/hooks/useOnboarding"
import { useBrand } from "@/hooks/useBrand"
import { MobilePageHeader } from "@/components/layout/MobilePageHeader"
import { useIsMobile } from "@/hooks/useMobile"

interface CardDef {
  id: string
  icon: LucideIcon
  iconTone: string
  titleKey: string
  descKey?: string
  /** Right-side trailing node (badge, toggle, chevron). */
  trailing?: ReactNode
  onClick?: () => void
}

function tintedTile(Icon: LucideIcon, tone: string, size = "h-5 w-5") {
  return (
    <span className={cn("flex h-7 w-7 items-center justify-center rounded-md", tone)}>
      <Icon className={size} />
    </span>
  )
}

function Card({ card }: { card: CardDef }) {
  const { t } = useTranslation("common")
  return (
    <button
      type="button"
      onClick={card.onClick}
      className="flex w-full items-center gap-3 px-3 py-2.5 text-left active:bg-muted-30 transition-colors"
    >
      {tintedTile(card.icon, card.iconTone)}
      <span className="min-w-0 flex-1">
        <span className="block truncate text-sm font-medium text-foreground">{t(card.titleKey)}</span>
        {card.descKey && (
          <span className="block truncate text-[11px] text-muted-foreground">{t(card.descKey)}</span>
        )}
      </span>
      {card.trailing ?? (
        <ChevronRight className="h-4 w-4 shrink-0 text-muted-foreground" />
      )}
    </button>
  )
}

function CardWithDropdown({
  card,
  children,
}: {
  card: Omit<CardDef, "onClick">
  children: ReactNode
}) {
  const { t } = useTranslation("common")
  // Wrap a custom dropdown trigger (e.g. ThemeToggle) — the whole card is
  // visual only, the embedded control handles interaction.
  return (
    <div className="relative flex w-full items-center gap-3 px-3 py-2.5">
      {tintedTile(card.icon, card.iconTone)}
      <span className="min-w-0 flex-1">
        <span className="block truncate text-sm font-medium text-foreground">{t(card.titleKey)}</span>
        {card.descKey && (
          <span className="block truncate text-[11px] text-muted-foreground">{t(card.descKey)}</span>
        )}
      </span>
      <span className="shrink-0">{children}</span>
    </div>
  )
}

function SectionLabel({ children }: { children: ReactNode }) {
  return (
    <div className="px-4 pb-1.5 pt-4 text-[11px] font-semibold uppercase tracking-wide text-muted-foreground">
      {children}
    </div>
  )
}

export default function SystemPage() {
  const { t, i18n } = useTranslation("common")
  const { name: brandName } = useBrand()
  const navigate = useNavigate()

  const user = useStore((s) => s.user)
  const openSettings = useStore((s) => s.openSettings)
  const logout = useStore((s) => s.logout)
  const alerts = useStore((s) => s.alerts)
  const { status: onboardingStatus, dismiss: dismissOnboarding } = useOnboarding()

  const [instanceManagerOpen, setInstanceManagerOpen] = useState(false)
  const [onboardingOpen, setOnboardingOpen] = useState(false)

  const unreadCount = useMemo(
    () =>
      alerts.filter(
        (a) => !a.acknowledged && a.status !== "resolved" && a.status !== "acknowledged",
      ).length,
    [alerts],
  )

  const toggleLanguage = () => i18n.changeLanguage(i18n.language === "zh" ? "en" : "zh")

  const businessCards: CardDef[] = [
    {
      id: "automation",
      icon: Workflow,
      iconTone: "bg-primary-light text-primary",
      titleKey: "nav.automation",
      descKey: "system.cards.automation",
      onClick: () => navigate("/automation"),
    },
    {
      id: "data",
      icon: Database,
      iconTone: "bg-info-light text-info",
      titleKey: "nav.data",
      descKey: "system.cards.data",
      onClick: () => navigate("/data"),
    },
    {
      id: "messages",
      icon: Bell,
      iconTone: "bg-warning-light text-warning",
      titleKey: "nav.messages",
      descKey: "system.cards.messages",
      trailing: unreadCount > 0 ? (
        <Badge variant="destructive" className="h-5 min-w-5 justify-center px-1 text-xs">
          {unreadCount > 99 ? "99+" : unreadCount}
        </Badge>
      ) : (
        <ChevronRight className="h-4 w-4 text-muted-foreground" />
      ),
      onClick: () => navigate("/messages"),
    },
  ]

  const systemCardsTop: CardDef[] = [
    {
      id: "extensions",
      icon: Puzzle,
      iconTone: "bg-accent-purple-light text-accent-purple",
      titleKey: "nav.extensions",
      descKey: "system.cards.extensions",
      onClick: () => navigate("/extensions"),
    },
    {
      id: "theme",
      icon: Palette,
      iconTone: "bg-primary-light text-primary",
      titleKey: "theme.title",
      descKey: "system.cards.theme",
    },
    {
      id: "language",
      icon: Languages,
      iconTone: "bg-info-light text-info",
      titleKey: "system.language",
      descKey: "system.cards.language",
      trailing: (
        <span className="text-xs text-muted-foreground">
          {i18n.language === "zh" ? "中文" : "English"}
        </span>
      ),
      onClick: toggleLanguage,
    },
    {
      id: "instance",
      icon: Server,
      iconTone: "bg-success-light text-success",
      titleKey: "system.instanceManager",
      descKey: "system.cards.instance",
      onClick: () => setInstanceManagerOpen(true),
    },
    {
      id: "settings",
      icon: Settings,
      iconTone: "bg-muted text-foreground",
      titleKey: "nav.settings",
      descKey: "system.cards.settings",
      onClick: () => openSettings(),
    },
  ]

  const aboutCards: CardDef[] = [
    {
      id: "health",
      icon: Activity,
      iconTone: "bg-success-light text-success",
      titleKey: "systemHealth.title",
      descKey: "system.cards.health",
    },
    {
      id: "onboarding",
      icon: Rocket,
      iconTone: "bg-primary-light text-primary",
      titleKey: "onboarding.title",
      descKey: "system.cards.onboarding",
      onClick: () => setOnboardingOpen(true),
    },
    {
      id: "about",
      icon: HelpCircle,
      iconTone: "bg-muted text-foreground",
      titleKey: "system.about",
      descKey: "system.cards.about",
      onClick: () => openSettings("about"),
    },
    {
      id: "logout",
      icon: LogOut,
      iconTone: "bg-error-light text-error",
      titleKey: "logout",
      onClick: () => logout(),
    },
  ]

  const userInitials = (user?.username ?? "").slice(0, 2).toUpperCase()

  return (
    <div className="h-full overflow-y-auto overscroll-contain bg-background">
      <MobilePageHeader title={t("system.pageTitle")} />
      {/* Brand + user header (mobile ~70px) */}
      <div className="border-b border-border bg-background px-4 pb-3 pt-3">
        <div className="flex items-center gap-2.5">
          <BrandLogoWithName logoClassName="h-8" />
          <div className="min-w-0 flex-1">
            <div className="truncate text-lg font-bold leading-tight text-foreground">{brandName}</div>
            <div className="truncate text-[11px] text-muted-foreground">
              {t("system.version", { version: "0.8.19" })}
            </div>
          </div>
        </div>
        {user && (
          <div className="mt-2 flex items-center gap-2 rounded-lg bg-muted-30 px-2.5 py-1.5">
            <Avatar className="h-6 w-6 rounded-full">
              <AvatarFallback className="bg-muted text-[10px] font-medium text-foreground">
                {userInitials}
              </AvatarFallback>
            </Avatar>
            <span className="truncate text-xs text-foreground">{user.username}</span>
            {user.role && (
              <Badge variant="outline" className="ml-auto text-[10px]">
                {user.role}
              </Badge>
            )}
          </div>
        )}
      </div>

      {/* Section 1 — Business */}
      <SectionLabel>{t("system.sectionBusiness")}</SectionLabel>
      <div className="mx-3 overflow-hidden rounded-xl border border-border bg-surface">
        {businessCards.map((c, i) => (
          <div key={c.id} className={cn(i > 0 && "border-t border-border")}>
            <Card card={c} />
          </div>
        ))}
      </div>

      {/* Section 2 — System */}
      <SectionLabel>{t("system.sectionSystem")}</SectionLabel>
      <div className="mx-3 overflow-hidden rounded-xl border border-border bg-surface">
        {systemCardsTop.map((c, i) => (
          <div key={c.id} className={cn(i > 0 && "border-t border-border")}>
            {c.id === "theme" ? (
              <CardWithDropdown card={c}>
                <ThemeToggle />
              </CardWithDropdown>
            ) : (
              <Card card={c} />
            )}
          </div>
        ))}
      </div>

      {/* Section 3 — About */}
      <SectionLabel>{t("system.sectionAbout")}</SectionLabel>
      <div className="mx-3 overflow-hidden rounded-xl border border-border bg-surface">
        {aboutCards.map((c, i) => (
          <div key={c.id} className={cn(i > 0 && "border-t border-border")}>
            {c.id === "health" ? (
              <CardWithDropdown card={c}>
                <SystemHealthButton />
              </CardWithDropdown>
            ) : (
              <Card card={c} />
            )}
          </div>
        ))}
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
