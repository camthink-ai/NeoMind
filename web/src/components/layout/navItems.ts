/**
 * Shared navigation item definitions — the single source for the desktop
 * AppSidebar and the mobile MobileNav drawer. Both group these items
 * differently but must stay in sync on paths/icons/i18n keys.
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
} from "lucide-react"

export type PageType =
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
  id: PageType
  path: string
  icon: React.ComponentType<{ className?: string }>
  labelKey: string
  /** Shorter label for mobile contexts (falls back to labelKey if not set) */
  mobileLabelKey?: string
}

/** Route order = display order in the sidebar's PRIMARY group */
export const navItems: NavItem[] = [
  { id: "dashboard", path: "/chat", icon: MessageSquare, labelKey: "nav.dashboard", mobileLabelKey: "navShort.dashboard" },
  { id: "agents", path: "/agents", icon: Bot, labelKey: "nav.agents", mobileLabelKey: "navShort.agents" },
  { id: "devices", path: "/devices", icon: Cpu, labelKey: "nav.devices", mobileLabelKey: "navShort.devices" },
  { id: "visual-dashboard", path: "/visual-dashboard", icon: LayoutDashboard, labelKey: "nav.visual-dashboard", mobileLabelKey: "navShort.visual-dashboard" },
  { id: "automation", path: "/automation", icon: Workflow, labelKey: "nav.automation", mobileLabelKey: "navShort.automation" },
  { id: "data", path: "/data", icon: Database, labelKey: "nav.data", mobileLabelKey: "navShort.data" },
  { id: "messages", path: "/messages", icon: Bell, labelKey: "nav.messages", mobileLabelKey: "navShort.messages" },
  { id: "extensions", path: "/extensions", icon: Puzzle, labelKey: "nav.extensions", mobileLabelKey: "navShort.extensions" },
]

/** Sidebar groups — mirrors MobileNav's PRIMARY/SYSTEM split */
export const PRIMARY_NAV_IDS: PageType[] = ["dashboard", "agents", "devices", "visual-dashboard"]
export const SYSTEM_NAV_IDS: PageType[] = ["automation", "data", "messages", "extensions"]

/** Active-route check, shared by sidebar and any nav surface */
export function isNavItemActive(item: NavItem, currentPath: string): boolean {
  const path = currentPath.endsWith("/") && currentPath !== "/"
    ? currentPath.slice(0, -1)
    : currentPath
  return path === item.path ||
    (item.path === "/chat" && path === "/") ||
    path.startsWith(`${item.path}/`)
}
