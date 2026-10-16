/**
 * Shared navigation item definitions — consumed by the desktop AppSidebar
 * (and available to any nav surface). The mobile MobileNav drawer currently
 * maintains its own entry list.
 */

import {
  MessageSquare,
  Bot,
  LayoutDashboard,
  Cpu,
  Workflow,
  Database,
  Bell,
  Puzzle,
  Wifi,
  Camera,
  Radio,
  Network,
  Plug,
} from "lucide-react"
import type { ExtensionPage } from "@/hooks/useExtensionPages"

type PageType =
  | "dashboard"
  | "agents"
  | "visual-dashboard"
  | "devices"
  | "automation"
  | "data"
  | "messages"
  | "extensions"
  | "settings"

export interface NavItem {
  id: string
  path: string
  icon: React.ComponentType<{ className?: string }>
  labelKey: string
  /** Shorter label for mobile contexts (falls back to labelKey if not set) */
  mobileLabelKey?: string
}

/** Route order = display order in the sidebar's PRIMARY group */
export const navItems: NavItem[] = [
  { id: "dashboard", path: "/chat", icon: MessageSquare, labelKey: "nav.dashboard" },
  { id: "agents", path: "/agents", icon: Bot, labelKey: "nav.agents" },
  { id: "devices", path: "/devices", icon: Cpu, labelKey: "nav.devices" },
  { id: "visual-dashboard", path: "/visual-dashboard", icon: LayoutDashboard, labelKey: "nav.visual-dashboard" },
  { id: "automation", path: "/automation", icon: Workflow, labelKey: "nav.automation" },
  { id: "data", path: "/data", icon: Database, labelKey: "nav.data" },
  { id: "messages", path: "/messages", icon: Bell, labelKey: "nav.messages" },
  { id: "extensions", path: "/extensions", icon: Puzzle, labelKey: "nav.extensions" },
]

/** Active-route check, shared by sidebar and any nav surface */
export function isNavItemActive(item: NavItem, currentPath: string): boolean {
  const path = currentPath.endsWith("/") && currentPath !== "/"
    ? currentPath.slice(0, -1)
    : currentPath
  return path === item.path ||
    (item.path === "/chat" && path === "/") ||
    path.startsWith(`${item.path}/`)
}

/* ============ Extension-contributed nav (contribution points) ============ */

const EXTENSION_ICON_MAP: Record<string, React.ComponentType<{ className?: string }>> = {
  wifi: Wifi,
  camera: Camera,
  radio: Radio,
  network: Network,
  plug: Plug,
  cpu: Cpu,
  puzzle: Puzzle,
}

function extensionIcon(name: string | null) {
  return (name && EXTENSION_ICON_MAP[name.toLowerCase()]) || Puzzle
}

/**
 * Merge extension-contributed pages into the nav list.
 * nav_section "main" → append at the end; a known section id (e.g. "devices")
 * inserts right after that item; pages without nav_section are route-only.
 * labelKey carries the literal nav_label — the sidebar renders labelKey via
 * t(), and i18next falls back to the key itself when no translation exists,
 * so plain-text labels from extensions work unmodified.
 */
export function mergeExtensionNavItems(
  base: NavItem[],
  pages: ExtensionPage[],
): NavItem[] {
  // "add-device" pages live in the Add Device dialog, not the sidebar
  const navPages = pages.filter((p) => p.nav_section && p.nav_label && p.nav_section !== 'add-device')
  if (navPages.length === 0) return base
  const merged = [...base]
  for (const page of navPages) {
    const item: NavItem = {
      id: `ext-${page.extension_id}-${page.type}`,
      path: page.route,
      icon: extensionIcon(page.icon),
      // Literal label as pseudo-key: t() falls through to the key text.
      labelKey: page.nav_label ?? page.title,
    }
    if (page.nav_section === "main") {
      merged.push(item)
      continue
    }
    const idx = merged.findIndex((n) => n.id === page.nav_section)
    if (idx >= 0) merged.splice(idx + 1, 0, item)
    else merged.push(item)
  }
  return merged
}
