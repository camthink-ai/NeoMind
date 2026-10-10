import { ReactNode } from "react"
import { cn } from "@/lib/utils"
import { Cpu, Plug, Send, Sliders, Info, ArrowLeft } from "lucide-react"
import { useTranslation } from "react-i18next"

export type SettingsSection = "llm" | "connections" | "im" | "preferences" | "about"

export interface SettingsSectionConfig {
  value: SettingsSection
  label: string
  icon: ReactNode
}

export function getSettingsSections(t: ReturnType<typeof useTranslation>["t"]): SettingsSectionConfig[] {
  return [
    { value: "llm", label: t("settings:llmBackends"), icon: <Cpu className="h-4 w-4" /> },
    { value: "connections", label: t("settings:deviceConnections"), icon: <Plug className="h-4 w-4" /> },
    { value: "im", label: t("settings:imChannels"), icon: <Send className="h-4 w-4" /> },
    { value: "preferences", label: t("settings:preferences"), icon: <Sliders className="h-4 w-4" /> },
    { value: "about", label: t("settings:about"), icon: <Info className="h-4 w-4" /> },
  ]
}

interface SettingsNavProps {
  sections: SettingsSectionConfig[]
  activeSection: SettingsSection
  onSectionChange: (section: SettingsSection) => void
  /** Closes the settings dialog (back link at the top of the sidebar). */
  onBack: () => void
}

export function SettingsNav({ sections, activeSection, onSectionChange, onBack }: SettingsNavProps) {
  const { t } = useTranslation(["common", "settings"])
  return (
    <nav
      // w-60 (240px) — within the 240–300px sidebar range recommended by UX
      // best practices; the old w-52 (208px) felt cramped.
      className="w-60 shrink-0 hidden md:block rounded-lg border border-border p-2 flex flex-col mb-6"
      role="tablist"
      aria-label="Settings sections"
    >
      {/* Back link (closes the dialog) */}
      <button
        type="button"
        onClick={onBack}
        className="flex items-center gap-1.5 rounded-lg px-2 py-1.5 text-sm text-muted-foreground hover:bg-muted-30 hover:text-foreground transition-colors shrink-0"
      >
        <ArrowLeft className="w-4 h-4" />
        {t("common:back", { defaultValue: "Back" })}
      </button>

      {/* Sections */}
      <div className="space-y-1 mt-3">
        {sections.map((section) => {
          const isActive = activeSection === section.value
          return (
            <button
              key={section.value}
              role="tab"
              aria-selected={isActive}
              onClick={() => onSectionChange(section.value)}
              className={cn(
                "flex w-full items-center gap-3 rounded-lg px-3 py-2 text-sm font-medium transition-colors",
                "focus-visible:outline-2 focus-visible:outline-ring focus-visible:outline-offset-2",
                isActive
                  ? "bg-muted text-foreground"
                  : "text-muted-foreground hover:bg-muted-50 hover:text-foreground"
              )}
            >
              <span
                className={cn(
                  "shrink-0 transition-colors",
                  isActive ? "text-primary" : "text-muted-foreground"
                )}
              >
                {section.icon}
              </span>
              <span>{section.label}</span>
            </button>
          )
        })}
      </div>
    </nav>
  )
}
