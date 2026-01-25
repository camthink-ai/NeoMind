/**
 * AgentCreatorDialogSplit Component
 *
 * Step-by-step wizard for creating/editing AI Agents.
 * Pattern: 3-step wizard with progress indicator (Basic Info → Schedule → Resources)
 * Responsive: Tab-based on mobile, step-based on desktop
 */

import { useState, useEffect, useCallback, useMemo } from "react"
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import { Textarea } from "@/components/ui/textarea"
import { Badge } from "@/components/ui/badge"
import { Tabs, TabsList, TabsTrigger, TabsContent } from "@/components/ui/tabs"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import {
  Thermometer,
  Droplets,
  Lightbulb,
  Fan,
  Loader2,
  Save,
  Eye,
  Target,
  BarChart3,
  ChevronDown,
  ChevronRight,
  ChevronLeft,
  Search,
  Clock,
  Zap,
  Bell,
  Check,
  Info,
  X,
} from "lucide-react"
import { cn } from "@/lib/utils"
import { api } from "@/lib/api"
import { useToast } from "@/hooks/use-toast"
import { useTranslation } from "react-i18next"
import type {
  AiAgentDetail,
  CreateAgentRequest,
  Device,
  DeviceType,
  AgentScheduleType,
  AgentRole,
} from "@/types"

// ============================================================================
// Types & Interfaces
// ============================================================================

interface MetricInfo {
  name: string
  display_name: string
  unit?: string
  is_virtual?: boolean
}

interface AgentCreatorDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  agent: AiAgentDetail | undefined
  devices: Device[]
  deviceTypes: DeviceType[]
  onSave: (data: CreateAgentRequest | Partial<AiAgentDetail>) => Promise<void>
}

interface SelectedResource {
  deviceId: string
  deviceName: string
  deviceType: string
  metrics: Array<{ name: string; displayName: string }>
  commands: Array<{ name: string; displayName: string }>
}

// ============================================================================
// Constants
// ============================================================================

const ROLES: Array<{ value: AgentRole; label: string; icon: typeof Eye; description: string; color: string }> = [
  {
    value: 'Monitor',
    label: '监控专员',
    icon: Eye,
    description: '持续监控设备状态，检测异常情况并发送告警',
    color: 'text-blue-600 bg-blue-50 border-blue-200'
  },
  {
    value: 'Executor',
    label: '执行专员',
    icon: Fan,
    description: '根据条件自动控制设备，执行预设的操作指令',
    color: 'text-orange-600 bg-orange-50 border-orange-200'
  },
  {
    value: 'Analyst',
    label: '分析专员',
    icon: BarChart3,
    description: '分析数据趋势，生成报告并提供决策建议',
    color: 'text-purple-600 bg-purple-50 border-purple-200'
  },
]

const RESOURCE_ICONS: Record<string, React.ReactNode> = {
  temperature: <Thermometer className="h-4 w-4" />,
  humidity: <Droplets className="h-4 w-4" />,
  light: <Lightbulb className="h-4 w-4" />,
  switch: <Fan className="h-4 w-4" />,
  default: <Target className="h-4 w-4" />,
}

const INTERVAL_MINUTES = [1, 5, 10, 15, 30, 60]
const INTERVAL_HOURS = [1, 2, 3, 6, 12, 24]

const WEEKDAYS = [
  { value: 1, label: '周一' },
  { value: 2, label: '周二' },
  { value: 3, label: '周三' },
  { value: 4, label: '周四' },
  { value: 5, label: '周五' },
  { value: 6, label: '周六' },
  { value: 0, label: '周日' },
]

const HOURS = Array.from({ length: 24 }, (_, i) => ({
  value: i,
  label: i.toString().padStart(2, '0')
}))

const MINUTES = Array.from({ length: 60 }, (_, i) => ({
  value: i,
  label: i.toString().padStart(2, '0')
}))

const EVENT_TYPES = [
  { value: 'device.online', label: '设备上线', icon: <Zap className="h-3 w-3" /> },
  { value: 'device.offline', label: '设备离线', icon: <Target className="h-3 w-3" /> },
  { value: 'metric.threshold', label: '指标阈值', icon: <Bell className="h-3 w-3" /> },
  { value: 'metric.anomaly', label: '异常检测', icon: <Eye className="h-3 w-3" /> },
  { value: 'manual', label: '手动触发', icon: <Clock className="h-3 w-3" /> },
]

const COMMON_TIMEZONES = [
  { value: 'Asia/Shanghai', label: '中国 (UTC+8)' },
  { value: 'Asia/Tokyo', label: '日本 (UTC+9)' },
  { value: 'Asia/Seoul', label: '韩国 (UTC+9)' },
  { value: 'Asia/Singapore', label: '新加坡 (UTC+8)' },
  { value: 'Australia/Sydney', label: '悉尼 (UTC+10/+11)' },
  { value: 'Europe/London', label: '伦敦 (UTC+0/+1)' },
  { value: 'Europe/Paris', label: '巴黎 (UTC+1/+2)' },
  { value: 'Europe/Berlin', label: '柏林 (UTC+1/+2)' },
  { value: 'America/New_York', label: '纽约 (UTC-5/-4)' },
  { value: 'America/Los_Angeles', label: '洛杉矶 (UTC-8/-7)' },
  { value: 'UTC', label: 'UTC (UTC+0)' },
]

// ============================================================================
// Step Components
// ============================================================================

// Step 1: Basic Info
function BasicInfoStep({
  name,
  setName,
  role,
  setRole,
  userPrompt,
  setUserPrompt,
}: {
  name: string
  setName: (v: string) => void
  role: AgentRole
  setRole: (v: AgentRole) => void
  userPrompt: string
  setUserPrompt: (v: string) => void
}) {
  const { t } = useTranslation('agents')

  const ROLES = [
    {
      value: 'Monitor' as AgentRole,
      icon: Eye,
      color: 'text-blue-600 bg-blue-50 border-blue-200'
    },
    {
      value: 'Executor' as AgentRole,
      icon: Fan,
      color: 'text-orange-600 bg-orange-50 border-orange-200'
    },
    {
      value: 'Analyst' as AgentRole,
      icon: BarChart3,
      color: 'text-purple-600 bg-purple-50 border-purple-200'
    },
  ]

  return (
    <div className="space-y-5">
      {/* Role Selection */}
      <div>
        <Label className="text-sm font-medium mb-3 block">{t('creator.basicInfo.selectRole')}</Label>
        <div className="grid grid-cols-3 gap-3">
          {ROLES.map((r) => {
            const Icon = r.icon
            const isSelected = role === r.value
            const roleKey = r.value.toLowerCase()
            return (
              <button
                key={r.value}
                type="button"
                onClick={() => setRole(r.value)}
                className={cn(
                  "relative p-4 rounded-lg border-2 text-left transition-all",
                  isSelected ? r.color : "border-border hover:border-muted-foreground/30 bg-background"
                )}
              >
                <div className={cn("flex items-center justify-center w-10 h-10 rounded-lg mb-3", isSelected ? "bg-current/10" : "bg-muted")}>
                  <Icon className={cn("h-5 w-5", isSelected ? "" : "text-muted-foreground")} />
                </div>
                <div className={cn("text-sm font-semibold mb-1", isSelected ? "" : "text-muted-foreground")}>
                  {t(`creator.basicInfo.roles.${roleKey}.label`)}
                </div>
                <div className="text-xs text-muted-foreground line-clamp-2">
                  {t(`creator.basicInfo.roles.${roleKey}.description`)}
                </div>
                {isSelected && (
                  <div className="absolute top-2 right-2">
                    <div className="w-5 h-5 rounded-full bg-current flex items-center justify-center">
                      <Check className="h-3 w-3 text-background" />
                    </div>
                  </div>
                )}
              </button>
            )
          })}
        </div>
      </div>

      {/* Name */}
      <div>
        <Label htmlFor="agent-name" className="text-sm font-medium">
          {t('creator.basicInfo.name')} <span className="text-destructive">*</span>
        </Label>
        <Input
          id="agent-name"
          value={name}
          onChange={(e) => setName(e.target.value)}
          placeholder={t('creator.basicInfo.namePlaceholder')}
          className="mt-2 h-10"
        />
      </div>

      {/* User Prompt */}
      <div>
        <Label className="text-sm font-medium">
          {t('creator.basicInfo.requirement')} <span className="text-destructive">*</span>
        </Label>
        <p className="text-xs text-muted-foreground mt-1 mb-2">
          {t('creator.basicInfo.requirementHint')}
        </p>
        <Textarea
          value={userPrompt}
          onChange={(e) => setUserPrompt(e.target.value)}
          placeholder={t('creator.basicInfo.requirementPlaceholder')}
          rows={5}
          className="resize-none text-sm"
        />
        <div className="flex items-center gap-2 mt-2 text-xs text-muted-foreground">
          <Info className="h-3.5 w-3.5" />
          <span>{t('creator.basicInfo.autoParseHint')}</span>
        </div>
      </div>
    </div>
  )
}

// Step 2: Schedule Strategy
function ScheduleStep({
  scheduleType,
  setScheduleType,
  intervalValue,
  setIntervalValue,
  intervalUnit,
  setIntervalUnit,
  scheduleHour,
  setScheduleHour,
  scheduleMinute,
  setScheduleMinute,
  weekday,
  setWeekday,
  monthDay,
  setMonthDay,
  eventType,
  setEventType,
  eventDeviceId,
  setEventDeviceId,
  eventThreshold,
  setEventThreshold,
  selectedResources,
  timezone,
  setTimezone,
}: {
  scheduleType: 'interval' | 'daily' | 'weekly' | 'monthly' | 'event' | 'once'
  setScheduleType: (v: typeof scheduleType) => void
  intervalValue: number
  setIntervalValue: (v: number) => void
  intervalUnit: 'minute' | 'hour'
  setIntervalUnit: (v: typeof intervalUnit) => void
  scheduleHour: number
  setScheduleHour: (v: number) => void
  scheduleMinute: number
  setScheduleMinute: (v: number) => void
  weekday: number
  setWeekday: (v: number) => void
  monthDay: number
  setMonthDay: (v: number) => void
  eventType: string
  setEventType: (v: string) => void
  eventDeviceId: string
  setEventDeviceId: (v: string) => void
  eventThreshold: string
  setEventThreshold: (v: string) => void
  selectedResources: SelectedResource[]
  timezone: string
  setTimezone: (v: string) => void
}) {
  const { t } = useTranslation('agents')

  const WEEKDAYS = [
    { value: 1, label: t('creator.weekdays.1') },
    { value: 2, label: t('creator.weekdays.2') },
    { value: 3, label: t('creator.weekdays.3') },
    { value: 4, label: t('creator.weekdays.4') },
    { value: 5, label: t('creator.weekdays.5') },
    { value: 6, label: t('creator.weekdays.6') },
    { value: 0, label: t('creator.weekdays.0') },
  ]

  const EVENT_TYPES = [
    { value: 'device.online', label: t('creator.schedule.event.events.device.online'), icon: <Zap className="h-3 w-3" /> },
    { value: 'device.offline', label: t('creator.schedule.event.events.device.offline'), icon: <Target className="h-3 w-3" /> },
    { value: 'metric.threshold', label: t('creator.schedule.event.events.metric.threshold'), icon: <Bell className="h-3 w-3" /> },
    { value: 'metric.anomaly', label: t('creator.schedule.event.events.metric.anomaly'), icon: <Eye className="h-3 w-3" /> },
    { value: 'manual', label: t('creator.schedule.event.events.manual'), icon: <Clock className="h-3 w-3" /> },
  ]

  const COMMON_TIMEZONES = [
    { value: 'Asia/Shanghai', label: t('creator.timezones.Asia/Shanghai') },
    { value: 'Asia/Tokyo', label: t('creator.timezones.Asia/Tokyo') },
    { value: 'Asia/Seoul', label: t('creator.timezones.Asia/Seoul') },
    { value: 'Asia/Singapore', label: t('creator.timezones.Asia/Singapore') },
    { value: 'Australia/Sydney', label: t('creator.timezones.Australia/Sydney') },
    { value: 'Europe/London', label: t('creator.timezones.Europe/London') },
    { value: 'Europe/Paris', label: t('creator.timezones.Europe/Paris') },
    { value: 'Europe/Berlin', label: t('creator.timezones.Europe/Berlin') },
    { value: 'America/New_York', label: t('creator.timezones.America/New_York') },
    { value: 'America/Los_Angeles', label: t('creator.timezones.America/Los_Angeles') },
    { value: 'UTC', label: t('creator.timezones.UTC') },
  ]

  const INTERVAL_MINUTES = [1, 5, 10, 15, 30, 60]
  const INTERVAL_HOURS = [1, 2, 3, 6, 12, 24]

  const HOURS = Array.from({ length: 24 }, (_, i) => ({
    value: i,
    label: i.toString().padStart(2, '0')
  }))

  const MINUTES = Array.from({ length: 60 }, (_, i) => ({
    value: i,
    label: i.toString().padStart(2, '0')
  }))

  const getSchedulePreview = () => {
    const timezoneLabel = COMMON_TIMEZONES.find(tz => tz.value === timezone)?.label || timezone
    const suffix = timezone !== 'UTC' ? ` (${timezoneLabel})` : ''

    switch (scheduleType) {
      case 'interval':
        const unit = intervalUnit === 'minute' ? t('creator.schedule.interval.minutes') : t('creator.schedule.interval.hours')
        return t('creator.schedule.interval.preview', { value: intervalValue, unit }) + suffix
      case 'daily':
        return t('creator.schedule.daily.preview', { hour: scheduleHour.toString().padStart(2, '0'), minute: scheduleMinute.toString().padStart(2, '0') }) + suffix
      case 'weekly':
        const dayLabel = WEEKDAYS.find(d => d.value === weekday)?.label || weekday.toString()
        return t('creator.schedule.weekly.preview', { day: dayLabel, hour: scheduleHour.toString().padStart(2, '0'), minute: scheduleMinute.toString().padStart(2, '0') }) + suffix
      case 'monthly':
        return t('creator.schedule.monthly.preview', { day: monthDay, hour: scheduleHour.toString().padStart(2, '0'), minute: scheduleMinute.toString().padStart(2, '0') }) + suffix
      case 'event':
        const eventLabel = EVENT_TYPES.find(e => e.value === eventType)?.label || eventType
        const preview = t('creator.schedule.event.preview', { event: eventLabel })
        return eventDeviceId !== 'all' ? `${preview} (${t('creator.schedule.event.relatedDevice')}: ${eventDeviceId})` : preview
      case 'once':
        return t('creator.schedule.once.preview')
      default:
        return ''
    }
  }

  return (
    <div className="space-y-5">
      {/* Schedule Type Selection */}
      <div>
        <Label className="text-sm font-medium mb-3 block">{t('creator.schedule.selectStrategy')}</Label>
        <div className="grid grid-cols-6 gap-2">
          <button
            type="button"
            onClick={() => setScheduleType('interval')}
            className={cn(
              "flex flex-col items-center gap-1.5 p-3 rounded-lg border-2 transition-all",
              scheduleType === 'interval'
                ? "border-primary bg-primary/5"
                : "border-border hover:border-muted-foreground/30"
            )}
          >
            <Clock className={cn("h-5 w-5", scheduleType === 'interval' ? "text-primary" : "text-muted-foreground")} />
            <span className={cn("text-xs font-medium", scheduleType === 'interval' ? "text-primary" : "")}>{t('creator.schedule.strategies.interval')}</span>
          </button>
          <button
            type="button"
            onClick={() => setScheduleType('daily')}
            className={cn(
              "flex flex-col items-center gap-1.5 p-3 rounded-lg border-2 transition-all",
              scheduleType === 'daily'
                ? "border-primary bg-primary/5"
                : "border-border hover:border-muted-foreground/30"
            )}
          >
            <Zap className={cn("h-5 w-5", scheduleType === 'daily' ? "text-primary" : "text-muted-foreground")} />
            <span className={cn("text-xs font-medium", scheduleType === 'daily' ? "text-primary" : "")}>{t('creator.schedule.strategies.daily')}</span>
          </button>
          <button
            type="button"
            onClick={() => setScheduleType('weekly')}
            className={cn(
              "flex flex-col items-center gap-1.5 p-3 rounded-lg border-2 transition-all",
              scheduleType === 'weekly'
                ? "border-primary bg-primary/5"
                : "border-border hover:border-muted-foreground/30"
            )}
          >
            <Target className={cn("h-5 w-5", scheduleType === 'weekly' ? "text-primary" : "text-muted-foreground")} />
            <span className={cn("text-xs font-medium", scheduleType === 'weekly' ? "text-primary" : "")}>{t('creator.schedule.strategies.weekly')}</span>
          </button>
          <button
            type="button"
            onClick={() => setScheduleType('monthly')}
            className={cn(
              "flex flex-col items-center gap-1.5 p-3 rounded-lg border-2 transition-all",
              scheduleType === 'monthly'
                ? "border-primary bg-primary/5"
                : "border-border hover:border-muted-foreground/30"
            )}
          >
            <Bell className={cn("h-5 w-5", scheduleType === 'monthly' ? "text-primary" : "text-muted-foreground")} />
            <span className={cn("text-xs font-medium", scheduleType === 'monthly' ? "text-primary" : "")}>{t('creator.schedule.strategies.monthly')}</span>
          </button>
          <button
            type="button"
            onClick={() => setScheduleType('event')}
            className={cn(
              "flex flex-col items-center gap-1.5 p-3 rounded-lg border-2 transition-all",
              scheduleType === 'event'
                ? "border-primary bg-primary/5"
                : "border-border hover:border-muted-foreground/30"
            )}
          >
            <Target className={cn("h-5 w-5", scheduleType === 'event' ? "text-primary" : "text-muted-foreground")} />
            <span className={cn("text-xs font-medium", scheduleType === 'event' ? "text-primary" : "")}>{t('creator.schedule.strategies.event')}</span>
          </button>
          <button
            type="button"
            onClick={() => setScheduleType('once')}
            className={cn(
              "flex flex-col items-center gap-1.5 p-3 rounded-lg border-2 transition-all",
              scheduleType === 'once'
                ? "border-primary bg-primary/5"
                : "border-border hover:border-muted-foreground/30"
            )}
          >
            <Check className={cn("h-5 w-5", scheduleType === 'once' ? "text-primary" : "text-muted-foreground")} />
            <span className={cn("text-xs font-medium", scheduleType === 'once' ? "text-primary" : "")}>{t('creator.schedule.strategies.once')}</span>
          </button>
        </div>
      </div>

      {/* Schedule Configuration */}
      <div className="bg-muted/30 rounded-lg p-4 border border-border/50">
        <Label className="text-xs font-medium text-muted-foreground mb-3 block">{t('creator.schedule.config')}</Label>

        {scheduleType === 'once' && (
          <div className="text-sm text-muted-foreground">
            {t('creator.schedule.onceDescription')}
          </div>
        )}

        {scheduleType === 'interval' && (
          <div className="space-y-3">
            <div className="flex items-center gap-3 flex-wrap">
              <span className="text-sm">{t('creator.schedule.interval.every')}</span>
              <div className="flex items-center gap-1.5">
                {intervalUnit === 'minute' ? (
                  INTERVAL_MINUTES.map((mins) => (
                    <button
                      key={mins}
                      type="button"
                      onClick={() => setIntervalValue(mins)}
                      className={cn(
                        "px-4 py-2 rounded-lg border text-sm font-medium transition-all min-w-[4rem]",
                        intervalValue === mins
                          ? "border-primary bg-primary text-primary-foreground"
                          : "border-border hover:bg-muted"
                      )}
                    >
                      {mins}
                    </button>
                  ))
                ) : (
                  INTERVAL_HOURS.map((hours) => (
                    <button
                      key={hours}
                      type="button"
                      onClick={() => setIntervalValue(hours)}
                      className={cn(
                        "px-4 py-2 rounded-lg border text-sm font-medium transition-all min-w-[4rem]",
                        intervalValue === hours
                          ? "border-primary bg-primary text-primary-foreground"
                          : "border-border hover:bg-muted"
                      )}
                    >
                      {hours}
                    </button>
                  ))
                )}
              </div>
              <Select value={intervalUnit} onValueChange={(v: 'minute' | 'hour') => setIntervalUnit(v)}>
                <SelectTrigger className="w-28 h-10">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="minute">{t('creator.schedule.interval.minutes')}</SelectItem>
                  <SelectItem value="hour">{t('creator.schedule.interval.hours')}</SelectItem>
                </SelectContent>
              </Select>
              <span className="text-sm">{t('creator.schedule.interval.executeOnce')}</span>
            </div>
            <div className="flex items-center gap-2 pt-2 border-t">
              <Label className="text-xs text-muted-foreground">{t('creator.schedule.timezone')}</Label>
              <Select value={timezone} onValueChange={setTimezone}>
                <SelectTrigger className="w-56 h-9 text-sm">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {COMMON_TIMEZONES.map((tz) => (
                    <SelectItem key={tz.value} value={tz.value} className="text-sm">
                      {tz.label}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
          </div>
        )}

        {scheduleType === 'daily' && (
          <div className="space-y-3">
            <div className="flex items-center gap-3 flex-wrap">
              <span className="text-sm">{t('creator.schedule.daily.everyDay')}</span>
              <Select value={scheduleHour.toString()} onValueChange={(v) => setScheduleHour(parseInt(v))}>
                <SelectTrigger className="w-24 h-10">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {HOURS.map((h) => (
                    <SelectItem key={h.value} value={h.value.toString()}>
                      {h.label}:00
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
              <span className="text-sm">{t('creator.schedule.daily.at')}</span>
              <Select value={scheduleMinute.toString()} onValueChange={(v) => setScheduleMinute(parseInt(v))}>
                <SelectTrigger className="w-24 h-10">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {MINUTES.map((m) => (
                    <SelectItem key={m.value} value={m.value.toString()}>
                      :{m.label}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
              <span className="text-sm">{t('creator.schedule.daily.execute')}</span>
            </div>
            <div className="flex items-center gap-2 pt-2 border-t">
              <Label className="text-xs text-muted-foreground">{t('creator.schedule.timezone')}</Label>
              <Select value={timezone} onValueChange={setTimezone}>
                <SelectTrigger className="w-56 h-9 text-sm">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {COMMON_TIMEZONES.map((tz) => (
                    <SelectItem key={tz.value} value={tz.value} className="text-sm">
                      {tz.label}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
          </div>
        )}

        {scheduleType === 'weekly' && (
          <div className="space-y-3">
            <div className="flex items-center gap-3 flex-wrap">
              <span className="text-sm">{t('creator.schedule.weekly.every')}</span>
              <Select value={weekday.toString()} onValueChange={(v) => setWeekday(parseInt(v))}>
                <SelectTrigger className="w-28 h-10">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {WEEKDAYS.map((d) => (
                    <SelectItem key={d.value} value={d.value.toString()}>
                      {d.label}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
              <Select value={scheduleHour.toString()} onValueChange={(v) => setScheduleHour(parseInt(v))}>
                <SelectTrigger className="w-24 h-10">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {HOURS.map((h) => (
                    <SelectItem key={h.value} value={h.value.toString()}>
                      {h.label}:00
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
              <span className="text-sm">{t('creator.schedule.weekly.at')}</span>
              <Select value={scheduleMinute.toString()} onValueChange={(v) => setScheduleMinute(parseInt(v))}>
                <SelectTrigger className="w-24 h-10">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {MINUTES.map((m) => (
                    <SelectItem key={m.value} value={m.value.toString()}>
                      :{m.label}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
              <span className="text-sm">{t('creator.schedule.weekly.execute')}</span>
            </div>
            <div className="flex items-center gap-2 pt-2 border-t">
              <Label className="text-xs text-muted-foreground">{t('creator.schedule.timezone')}</Label>
              <Select value={timezone} onValueChange={setTimezone}>
                <SelectTrigger className="w-56 h-9 text-sm">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {COMMON_TIMEZONES.map((tz) => (
                    <SelectItem key={tz.value} value={tz.value} className="text-sm">
                      {tz.label}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
          </div>
        )}

        {scheduleType === 'monthly' && (
          <div className="space-y-3">
            <div className="flex items-center gap-3 flex-wrap">
              <span className="text-sm">{t('creator.schedule.monthly.everyMonth')}</span>
              <Select value={monthDay.toString()} onValueChange={(v) => setMonthDay(parseInt(v))}>
                <SelectTrigger className="w-28 h-10">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {Array.from({ length: 28 }, (_, i) => i + 1).map((d) => (
                    <SelectItem key={d} value={d.toString()}>
                      {d}{t('creator.schedule.monthly.day')}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
              <Select value={scheduleHour.toString()} onValueChange={(v) => setScheduleHour(parseInt(v))}>
                <SelectTrigger className="w-24 h-10">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {HOURS.map((h) => (
                    <SelectItem key={h.value} value={h.value.toString()}>
                      {h.label}:00
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
              <span className="text-sm">{t('creator.schedule.monthly.at')}</span>
              <Select value={scheduleMinute.toString()} onValueChange={(v) => setScheduleMinute(parseInt(v))}>
                <SelectTrigger className="w-24 h-10">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {MINUTES.map((m) => (
                    <SelectItem key={m.value} value={m.value.toString()}>
                      :{m.label}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
              <span className="text-sm">{t('creator.schedule.monthly.execute')}</span>
            </div>
            <div className="flex items-center gap-2 pt-2 border-t">
              <Label className="text-xs text-muted-foreground">{t('creator.schedule.timezone')}</Label>
              <Select value={timezone} onValueChange={setTimezone}>
                <SelectTrigger className="w-56 h-9 text-sm">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {COMMON_TIMEZONES.map((tz) => (
                    <SelectItem key={tz.value} value={tz.value} className="text-sm">
                      {tz.label}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
          </div>
        )}

        {scheduleType === 'event' && (
          <div className="space-y-4">
            {/* Event Type Selector */}
            <div>
              <Label className="text-xs text-muted-foreground mb-2 block">{t('creator.schedule.event.triggerEvent')}</Label>
              <div className="grid grid-cols-5 gap-2">
                {EVENT_TYPES.map((event) => (
                  <button
                    key={event.value}
                    type="button"
                    onClick={() => setEventType(event.value)}
                    className={cn(
                      "flex flex-col items-center gap-1.5 p-2 rounded-lg border transition-all",
                      eventType === event.value
                        ? "border-primary bg-primary/10"
                        : "border-border hover:bg-muted/50"
                    )}
                  >
                    {event.icon}
                    <span className={cn("text-xs", eventType === event.value ? "text-primary font-medium" : "text-muted-foreground")}>
                      {event.label}
                    </span>
                  </button>
                ))}
              </div>
            </div>

            {/* Event-specific Configuration */}
            {eventType === 'device.online' && (
              <div className="p-3 bg-blue-50/50 border border-blue-200 rounded-lg space-y-3">
                <div className="flex items-center gap-2">
                  <Zap className="h-4 w-4 text-blue-600" />
                  <span className="text-sm font-medium">{t('creator.schedule.event.deviceOnline.title')}</span>
                </div>
                <p className="text-xs text-muted-foreground">{t('creator.schedule.event.deviceOnline.description')}</p>
                <div>
                  <Label className="text-xs text-muted-foreground mb-1 block">{t('creator.schedule.event.deviceOnline.relatedDevice')}</Label>
                  <Select value={eventDeviceId} onValueChange={setEventDeviceId}>
                    <SelectTrigger className="h-10 text-sm">
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      <SelectItem value="all">{t('creator.schedule.event.allDevices')}</SelectItem>
                      {selectedResources.map((r) => (
                        <SelectItem key={r.deviceId} value={r.deviceId}>{r.deviceName}</SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                </div>
              </div>
            )}

            {eventType === 'device.offline' && (
              <div className="p-3 bg-red-50/50 border border-red-200 rounded-lg space-y-3">
                <div className="flex items-center gap-2">
                  <Target className="h-4 w-4 text-red-600" />
                  <span className="text-sm font-medium">{t('creator.schedule.event.deviceOffline.title')}</span>
                </div>
                <p className="text-xs text-muted-foreground">{t('creator.schedule.event.deviceOffline.description')}</p>
                <div>
                  <Label className="text-xs text-muted-foreground mb-1 block">{t('creator.schedule.event.deviceOffline.relatedDevice')}</Label>
                  <Select value={eventDeviceId} onValueChange={setEventDeviceId}>
                    <SelectTrigger className="h-10 text-sm">
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      <SelectItem value="all">{t('creator.schedule.event.allDevices')}</SelectItem>
                      {selectedResources.map((r) => (
                        <SelectItem key={r.deviceId} value={r.deviceId}>{r.deviceName}</SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                </div>
              </div>
            )}

            {eventType === 'metric.threshold' && (
              <div className="p-3 bg-orange-50/50 border border-orange-200 rounded-lg space-y-3">
                <div className="flex items-center gap-2">
                  <Bell className="h-4 w-4 text-orange-600" />
                  <span className="text-sm font-medium">{t('creator.schedule.event.metricThreshold.title')}</span>
                </div>
                <div className="space-y-2">
                  <Label className="text-xs text-muted-foreground">{t('creator.schedule.event.metricThreshold.condition')}</Label>
                  <Input
                    value={eventThreshold}
                    onChange={(e) => setEventThreshold(e.target.value)}
                    placeholder={t('creator.schedule.event.metricThreshold.conditionPlaceholder')}
                    className="h-9 text-sm"
                  />
                  <p className="text-xs text-muted-foreground">{t('creator.schedule.event.metricThreshold.conditionHint')}</p>
                </div>
                <div>
                  <Label className="text-xs text-muted-foreground mb-1 block">{t('creator.schedule.event.metricThreshold.monitorDevice')}</Label>
                  <Select value={eventDeviceId} onValueChange={setEventDeviceId}>
                    <SelectTrigger className="h-10 text-sm">
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      <SelectItem value="all">{t('creator.schedule.event.allDevices')}</SelectItem>
                      {selectedResources.map((r) => (
                        <SelectItem key={r.deviceId} value={r.deviceId}>{r.deviceName}</SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                </div>
              </div>
            )}

            {eventType === 'metric.anomaly' && (
              <div className="p-3 bg-purple-50/50 border border-purple-200 rounded-lg space-y-3">
                <div className="flex items-center gap-2">
                  <Eye className="h-4 w-4 text-purple-600" />
                  <span className="text-sm font-medium">{t('creator.schedule.event.metricAnomaly.title')}</span>
                </div>
                <p className="text-xs text-muted-foreground">{t('creator.schedule.event.metricAnomaly.description')}</p>
                <div className="space-y-2">
                  <Label className="text-xs text-muted-foreground">{t('creator.schedule.event.metricAnomaly.threshold')}</Label>
                  <Input
                    value={eventThreshold}
                    onChange={(e) => setEventThreshold(e.target.value)}
                    placeholder={t('creator.schedule.event.metricAnomaly.thresholdPlaceholder')}
                    className="h-9 text-sm"
                  />
                </div>
                <div>
                  <Label className="text-xs text-muted-foreground mb-1 block">{t('creator.schedule.event.metricAnomaly.monitorDevice')}</Label>
                  <Select value={eventDeviceId} onValueChange={setEventDeviceId}>
                    <SelectTrigger className="h-10 text-sm">
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      <SelectItem value="all">{t('creator.schedule.event.allDevices')}</SelectItem>
                      {selectedResources.map((r) => (
                        <SelectItem key={r.deviceId} value={r.deviceId}>{r.deviceName}</SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                </div>
              </div>
            )}

            {eventType === 'manual' && (
              <div className="p-3 bg-muted/50 border border-border rounded-lg">
                <div className="flex items-center gap-2 mb-2">
                  <Clock className="h-4 w-4 text-muted-foreground" />
                  <span className="text-sm font-medium">{t('creator.schedule.event.manual.title')}</span>
                </div>
                <p className="text-xs text-muted-foreground">{t('creator.schedule.event.manual.description')}</p>
              </div>
            )}
          </div>
        )}
      </div>

      {/* Preview */}
      <div className="flex items-center gap-2 p-3 bg-primary/5 rounded-lg border border-primary/20">
        <Clock className="h-4 w-4 text-primary shrink-0" />
        <span className="text-sm">{getSchedulePreview()}</span>
      </div>
    </div>
  )
}

// Mini Topology Component - Data Flow Style
function MiniTopology({
  selectedResources,
  role,
}: {
  selectedResources: SelectedResource[]
  role: AgentRole
}) {
  const { t } = useTranslation('agents')

  if (selectedResources.length === 0) {
    return (
      <div className="h-24 flex items-center justify-center text-muted-foreground text-xs border rounded-lg bg-muted/10">
        {t('creator.resources.selectAfterShow')}
      </div>
    )
  }

  const allMetrics = selectedResources.flatMap(r =>
    r.metrics.map(m => ({ ...m, deviceName: r.deviceName }))
  )
  const allCommands = selectedResources.flatMap(r =>
    r.commands.map(c => ({ ...c, deviceName: r.deviceName }))
  )

  const isMonitor = role === 'Monitor'
  const isExecutor = role === 'Executor'
  const isAnalyst = role === 'Analyst'

  const primaryColor = isMonitor ? 'from-blue-500 to-blue-600' : isExecutor ? 'from-orange-500 to-orange-600' : 'from-purple-500 to-purple-600'
  const ringColor = isMonitor ? 'ring-blue-500/20' : isExecutor ? 'ring-orange-500/20' : 'ring-purple-500/20'
  const roleLabel = isMonitor ? t('creator.resources.metrics') : isExecutor ? t('resources.commands') : t('creator.resources.commands')

  // Create a limited set of metrics to display
  const displayMetrics = allMetrics.slice(0, 4)
  const remainingMetrics = allMetrics.length - 4

  // Create a limited set of outputs to display
  const outputs: Array<{ type: string; label: string; color: string; icon: React.ReactNode }> = []
  if (isMonitor) outputs.push({ type: 'alert', label: t('creator.resources.metrics'), color: 'bg-red-500', icon: <Bell className="h-3 w-3" /> })
  if (isAnalyst) outputs.push({ type: 'analysis', label: t('creator.resources.commands'), color: 'bg-purple-500', icon: <BarChart3 className="h-3 w-3" /> })
  allCommands.slice(0, 2).forEach(c => outputs.push({ type: 'command', label: c.displayName, color: 'bg-orange-500', icon: <Fan className="h-3 w-3" /> }))

  return (
    <div className="border rounded-lg p-4 bg-gradient-to-br from-muted/20 to-muted/5">
      {/* Data Flow Diagram */}
      <div className="relative flex items-center justify-between gap-2 min-h-[80px]">
        {/* Left: Input Nodes */}
        <div className="flex-1 flex flex-col items-center justify-center gap-2">
          {displayMetrics.length > 0 ? (
            <>
              {displayMetrics.map((m, i) => (
                <div key={i} className="relative w-full">
                  {/* Metric Node */}
                  <div className="flex items-center justify-end gap-2">
                    <div className="flex-1 text-right">
                      <div className="inline-flex items-center gap-1.5 px-2.5 py-1.5 rounded-lg bg-blue-50 border border-blue-200 text-blue-700 text-xs font-medium shadow-sm">
                        <Thermometer className="h-3.5 w-3.5" />
                        <span className="max-w-24 truncate">{m.displayName}</span>
                      </div>
                    </div>
                    {/* Arrow to center */}
                    <div className="flex items-center w-12">
                      <div className="h-px w-8 bg-gradient-to-r from-blue-400 to-transparent" />
                      <div className="w-1.5 h-1.5 rounded-full bg-blue-400" />
                    </div>
                  </div>
                </div>
              ))}
              {remainingMetrics > 0 && (
                <div className="text-xs text-muted-foreground italic">
                  {t('creator.resources.moreMetrics', { count: remainingMetrics })}
                </div>
              )}
            </>
          ) : (
            <div className="text-xs text-muted-foreground text-center">
              <div className="inline-flex items-center gap-1 px-2 py-1 rounded bg-muted/30">
                <Thermometer className="h-3 w-3 opacity-50" />
                <span>{t('creator.resources.noMetrics')}</span>
              </div>
            </div>
          )}
        </div>

        {/* Center: AI Agent Node */}
        <div className="flex flex-col items-center z-10">
          {/* Animated pulse ring */}
          <div className={cn(
            "absolute w-20 h-20 rounded-full bg-gradient-to-br animate-pulse",
            primaryColor,
            ringColor
          )} />
          {/* Main agent circle */}
          <div className="relative w-20 h-20 rounded-full bg-gradient-to-br text-white flex flex-col items-center justify-center shadow-xl z-10 ring-4 ring-background"
            style={{ background: `linear-gradient(135deg, var(--color-primary), ${isMonitor ? '#1e40af' : isExecutor ? '#c2410c' : '#6b21a8'})` }}>
            <Zap className="h-6 w-6 mb-0.5" />
            <span className="text-xs font-semibold leading-none">{roleLabel}</span>
          </div>
          {/* Device count badge */}
          <div className="absolute -bottom-1 -right-1 w-6 h-6 rounded-full bg-background border flex items-center justify-center text-xs font-bold shadow-sm">
            {selectedResources.length}
          </div>
        </div>

        {/* Right: Output Nodes */}
        <div className="flex-1 flex flex-col items-center justify-center gap-2">
          {outputs.length > 0 ? (
            <>
              {outputs.map((out, i) => (
                <div key={i} className="relative w-full">
                  {/* Output Node */}
                  <div className="flex items-center justify-start gap-2">
                    {/* Arrow from center */}
                    <div className="flex items-center w-12">
                      <div className="w-1.5 h-1.5 rounded-full" style={{ backgroundColor: out.color.replace('500', '400') }} />
                      <div className="h-px w-8 bg-gradient-to-l from-transparent" style={{ backgroundColor: out.color.replace('500', '40ccff') }} />
                    </div>
                    <div className="flex-1">
                      <div className={cn(
                        "inline-flex items-center gap-1.5 px-2.5 py-1.5 rounded-lg border text-white text-xs font-medium shadow-sm",
                        out.color,
                        out.type === 'alert' ? 'bg-red-500 border-red-600' : '',
                        out.type === 'analysis' ? 'bg-purple-500 border-purple-600' : '',
                        out.type === 'command' ? 'bg-orange-500 border-orange-600' : ''
                      )}>
                        {out.icon}
                        <span className="max-w-20 truncate">{out.label}</span>
                      </div>
                    </div>
                  </div>
                </div>
              ))}
              {allCommands.length > 2 && (
                <div className="text-xs text-muted-foreground italic">
                  {t('creator.resources.moreCommands', { count: allCommands.length - 2 })}
                </div>
              )}
            </>
          ) : (
            <div className="text-xs text-muted-foreground text-center">
              <div className="inline-flex items-center gap-1 px-2 py-1 rounded bg-muted/30">
                <Fan className="h-3 w-3 opacity-50" />
                <span>{t('creator.resources.noOutputs')}</span>
              </div>
            </div>
          )}
        </div>
      </div>

      {/* Flow Legend */}
      <div className="flex items-center justify-center gap-6 mt-4 pt-3 border-t border-border/50 text-xs">
        <div className="flex items-center gap-2">
          <div className="flex items-center gap-1">
            <div className="w-2 h-0.5 bg-blue-400 rounded-full" />
            <div className="w-1.5 h-1.5 rounded-full bg-blue-400" />
          </div>
          <span className="text-muted-foreground">{t('creator.resources.dataInput')}</span>
        </div>
        <div className="flex items-center gap-2">
          <div className="flex items-center gap-1">
            <div className="w-1.5 h-1.5 rounded-full bg-gradient-to-r from-gray-400 to-gray-400" />
            <div className="w-2 h-0.5 bg-gray-400 rounded-full" />
          </div>
          <span className="text-muted-foreground">{t('creator.resources.dataOutput')}</span>
        </div>
      </div>
    </div>
  )
}

// Step 3: Resource Selection
function ResourceStep({
  devices,
  deviceTypes,
  selectedResources,
  setSelectedResources,
  expandedDevices,
  setExpandedDevices,
  metricsCache,
  setMetricsCache,
  loadingMetrics,
  setLoadingMetrics,
  searchQuery,
  setSearchQuery,
  role,
}: {
  devices: Device[]
  deviceTypes: DeviceType[]
  selectedResources: SelectedResource[]
  setSelectedResources: (v: SelectedResource[]) => void
  expandedDevices: Set<string>
  setExpandedDevices: (s: Set<string>) => void
  metricsCache: Record<string, MetricInfo[]>
  setMetricsCache: (v: Record<string, MetricInfo[]>) => void
  loadingMetrics: Record<string, boolean>
  setLoadingMetrics: (v: Record<string, boolean>) => void
  searchQuery: string
  setSearchQuery: (v: string) => void
  role: AgentRole
}) {
  const { t } = useTranslation('agents')

  const fetchDeviceMetrics = useCallback(async (deviceId: string): Promise<MetricInfo[]> => {
    if (metricsCache[deviceId]) return metricsCache[deviceId]
    setLoadingMetrics({ ...loadingMetrics, [deviceId]: true })
    try {
      const device = devices.find(d => d.device_id === deviceId)
      let metrics: MetricInfo[] = []
      if (device?.device_type) {
        const deviceType = deviceTypes.find(dt => dt.device_type === device.device_type)
        if (deviceType?.metrics) {
          metrics = deviceType.metrics.map(m => ({
            name: m.name,
            display_name: m.display_name || m.name,
            unit: m.unit,
            is_virtual: false,
          }))
        }
      }
      try {
        const summary = await api.getDeviceTelemetrySummary(deviceId)
        for (const [metricName, metricData] of Object.entries(summary.summary)) {
          if (!metrics.find(m => m.name === metricName)) {
            metrics.push({
              name: metricName,
              display_name: metricData.display_name || metricName,
              unit: metricData.unit,
              is_virtual: metricData.is_virtual,
            })
          }
        }
      } catch { /* ignore */ }
      setMetricsCache({ ...metricsCache, [deviceId]: metrics })
      return metrics
    } finally {
      setLoadingMetrics({ ...loadingMetrics, [deviceId]: false })
    }
  }, [devices, deviceTypes, metricsCache, loadingMetrics, setLoadingMetrics, setMetricsCache])

  const toggleDeviceExpanded = async (deviceId: string) => {
    const newExpanded = new Set(expandedDevices)
    if (newExpanded.has(deviceId)) {
      newExpanded.delete(deviceId)
    } else {
      newExpanded.add(deviceId)
      await fetchDeviceMetrics(deviceId)
    }
    setExpandedDevices(newExpanded)
  }

  const toggleMetric = (deviceId: string, metricName: string, displayName: string) => {
    const existing = selectedResources.findIndex(r => r.deviceId === deviceId)
    if (existing < 0) {
      const device = devices.find(d => d.device_id === deviceId)!
      setSelectedResources([...selectedResources, {
        deviceId,
        deviceName: device.name,
        deviceType: device.device_type,
        metrics: [{ name: metricName, displayName }],
        commands: []
      }])
      return
    }
    const newResources = [...selectedResources]
    const resource = newResources[existing]
    const metricIndex = resource.metrics.findIndex(m => m.name === metricName)
    if (metricIndex >= 0) {
      resource.metrics = resource.metrics.filter(m => m.name !== metricName)
      if (resource.metrics.length === 0 && resource.commands.length === 0) {
        newResources.splice(existing, 1)
      }
    } else {
      resource.metrics = [...resource.metrics, { name: metricName, displayName }]
    }
    setSelectedResources(newResources)
  }

  const toggleCommand = (deviceId: string, commandName: string, displayName: string) => {
    const existing = selectedResources.findIndex(r => r.deviceId === deviceId)
    if (existing < 0) {
      const device = devices.find(d => d.device_id === deviceId)!
      setSelectedResources([...selectedResources, {
        deviceId,
        deviceName: device.name,
        deviceType: device.device_type,
        metrics: [],
        commands: [{ name: commandName, displayName }]
      }])
      return
    }
    const newResources = [...selectedResources]
    const resource = newResources[existing]
    const commandIndex = resource.commands.findIndex(c => c.name === commandName)
    if (commandIndex >= 0) {
      resource.commands = resource.commands.filter(c => c.name !== commandName)
      if (resource.metrics.length === 0 && resource.commands.length === 0) {
        newResources.splice(existing, 1)
      }
    } else {
      resource.commands = [...resource.commands, { name: commandName, displayName }]
    }
    setSelectedResources(newResources)
  }

  const getDeviceIcon = (deviceType: string) => {
    const type = deviceTypes.find(dt => dt.device_type === deviceType)
    const category = type?.categories?.[0] || 'default'
    return RESOURCE_ICONS[category] || RESOURCE_ICONS.default
  }

  const getDeviceMetrics = (deviceId: string): MetricInfo[] => metricsCache[deviceId] || []

  const getDeviceCommands = (deviceId: string) => {
    const device = devices.find(d => d.device_id === deviceId)
    if (!device) return []
    const type = deviceTypes.find(dt => dt.device_type === device.device_type)
    return type?.commands || []
  }

  const isMetricSelected = (deviceId: string, metricName: string) => {
    return selectedResources.find(r => r.deviceId === deviceId)?.metrics.some(m => m.name === metricName) || false
  }

  const isCommandSelected = (deviceId: string, commandName: string) => {
    return selectedResources.find(r => r.deviceId === deviceId)?.commands.some(c => c.name === commandName) || false
  }

  const filteredDevices = devices.filter(device => {
    if (!searchQuery) return true
    const query = searchQuery.toLowerCase()
    return device.name.toLowerCase().includes(query) || device.device_type.toLowerCase().includes(query)
  })

  const selectedCount = selectedResources.reduce((acc, r) => acc + r.metrics.length + r.commands.length, 0)

  return (
    <div className="space-y-4">
      {/* Search & Summary */}
      <div className="flex items-center gap-3">
        <div className="relative flex-1">
          <Search className="absolute left-3 top-1/2 -translate-y-1/2 h-4 w-4 text-muted-foreground" />
          <Input
            value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
            placeholder={t('creator.resources.searchPlaceholder')}
            className="pl-9 h-9"
          />
        </div>
        {selectedCount > 0 && (
          <Badge variant="secondary" className="h-7 px-3">
            {t('creator.resources.selectedCount', { count: selectedCount })}
          </Badge>
        )}
      </div>

      {/* Device List */}
      <div className="flex-1 overflow-y-auto -mx-1">
        {filteredDevices.length === 0 ? (
          <div className="text-center py-12 text-muted-foreground">
            <Search className="h-8 w-8 mx-auto mb-2 opacity-50" />
            <p className="text-sm">{t('creator.resources.noDevicesFound')}</p>
          </div>
        ) : (
          <div className="space-y-2 max-h-[280px] overflow-y-auto pr-1">
            {filteredDevices.map((device) => {
              const isExpanded = expandedDevices.has(device.device_id)
              const Icon = getDeviceIcon(device.device_type)
              const hasSelection = selectedResources.find(r => r.deviceId === device.device_id)
              const metricCount = selectedResources.find(r => r.deviceId === device.device_id)?.metrics.length || 0
              const commandCount = selectedResources.find(r => r.deviceId === device.device_id)?.commands.length || 0
              const metrics = getDeviceMetrics(device.device_id)
              const isLoading = loadingMetrics[device.device_id]
              const commands = getDeviceCommands(device.device_id)

              return (
                <div
                  key={device.device_id}
                  className={cn(
                    "border rounded-lg overflow-hidden transition-all",
                    hasSelection ? "border-primary/50 bg-primary/5" : "border-border"
                  )}
                >
                  <button
                    type="button"
                    onClick={() => void toggleDeviceExpanded(device.device_id)}
                    className="w-full px-3 py-2.5 flex items-center gap-3 hover:bg-muted/30 text-left"
                  >
                    <div className="p-1.5 rounded bg-muted">{Icon}</div>
                    <div className="flex-1 min-w-0">
                      <div className="text-sm font-medium truncate">{device.name}</div>
                      <div className="text-xs text-muted-foreground truncate">{device.device_type}</div>
                    </div>
                    {(metricCount > 0 || commandCount > 0) && (
                      <Badge variant="secondary" className="text-xs h-5 mr-1">
                        {metricCount > 0 && t('creator.resources.metricCount', { count: metricCount })}
                        {metricCount > 0 && commandCount > 0 && ' + '}
                        {commandCount > 0 && t('creator.resources.commandCount', { count: commandCount })}
                      </Badge>
                    )}
                    <ChevronDown className={cn(
                      "h-4 w-4 text-muted-foreground transition-transform flex-shrink-0",
                      isExpanded && "rotate-180"
                    )} />
                  </button>

                  {isExpanded && (
                    <div className="border-t p-2.5 bg-muted/20 space-y-3">
                      {isLoading ? (
                        <div className="flex items-center justify-center py-3">
                          <Loader2 className="h-4 w-4 animate-spin text-muted-foreground" />
                        </div>
                      ) : (
                        <>
                          {metrics.length > 0 && (
                            <div>
                              <div className="text-xs text-muted-foreground mb-2 px-1">{t('creator.resources.metrics')}</div>
                              <div className="flex flex-wrap gap-1.5">
                                {metrics.map((metric) => (
                                  <button
                                    key={metric.name}
                                    type="button"
                                    onClick={() => toggleMetric(device.device_id, metric.name, metric.display_name)}
                                    className={cn(
                                      "text-xs px-2.5 py-1 rounded-md transition-colors",
                                      isMetricSelected(device.device_id, metric.name)
                                        ? "bg-primary text-primary-foreground font-medium"
                                        : "bg-background hover:bg-muted border"
                                    )}
                                  >
                                    {metric.display_name}
                                  </button>
                                ))}
                              </div>
                            </div>
                          )}
                          {commands.length > 0 && (
                            <div>
                              <div className="text-xs text-muted-foreground mb-2 px-1">{t('creator.resources.commands')}</div>
                              <div className="flex flex-wrap gap-1.5">
                                {commands.map((command) => (
                                  <button
                                    key={command.name}
                                    type="button"
                                    onClick={() => toggleCommand(device.device_id, command.name, command.display_name)}
                                    className={cn(
                                      "text-xs px-2.5 py-1 rounded-md transition-colors",
                                      isCommandSelected(device.device_id, command.name)
                                        ? "bg-primary text-primary-foreground font-medium"
                                        : "bg-background hover:bg-muted border"
                                    )}
                                  >
                                    {command.display_name}
                                  </button>
                                ))}
                              </div>
                            </div>
                          )}
                        </>
                      )}
                    </div>
                  )}
                </div>
              )
            })}
          </div>
        )}
      </div>

      {/* Mini Topology */}
      <div>
        <div className="flex items-center gap-2 mb-2">
          <Label className="text-xs text-muted-foreground">{t('creator.resources.topologyPreview')}</Label>
          {selectedResources.length === 0 && (
            <span className="text-xs text-destructive">* {t('creator.resources.selectResourceHint')}</span>
          )}
        </div>
        <MiniTopology selectedResources={selectedResources} role={role} />
      </div>
    </div>
  )
}

// ============================================================================
// Main Dialog Component
// ============================================================================

export function AgentCreatorDialogSplit({
  open,
  onOpenChange,
  agent,
  devices,
  deviceTypes,
  onSave,
}: AgentCreatorDialogProps) {
  const { toast } = useToast()
  const { t } = useTranslation('agents')
  const isEditing = !!agent

  // Step state
  const [currentStep, setCurrentStep] = useState(0)

  // Computed steps using useMemo
  const steps = useMemo(() => [
    t('creator.steps.basic'),
    t('creator.steps.schedule'),
    t('creator.steps.resources')
  ], [t])

  // Form state
  const [name, setName] = useState("")
  const [role, setRole] = useState<AgentRole>('Monitor')
  const [userPrompt, setUserPrompt] = useState("")
  const [scheduleType, setScheduleType] = useState<'interval' | 'daily' | 'weekly' | 'monthly' | 'event' | 'once'>('interval')
  const [intervalValue, setIntervalValue] = useState(5)
  const [intervalUnit, setIntervalUnit] = useState<'minute' | 'hour'>('minute')
  const [scheduleHour, setScheduleHour] = useState(9)
  const [scheduleMinute, setScheduleMinute] = useState(0)
  const [weekday, setWeekday] = useState(1)
  const [monthDay, setMonthDay] = useState(1)
  const [eventType, setEventType] = useState('device.online')
  const [eventDeviceId, setEventDeviceId] = useState<string>('all')
  const [eventThreshold, setEventThreshold] = useState('')
  const [timezone, setTimezone] = useState(Intl.DateTimeFormat().resolvedOptions().timeZone)

  // Resource state
  const [selectedResources, setSelectedResources] = useState<SelectedResource[]>([])
  const [expandedDevices, setExpandedDevices] = useState<Set<string>>(new Set())
  const [metricsCache, setMetricsCache] = useState<Record<string, MetricInfo[]>>({})
  const [loadingMetrics, setLoadingMetrics] = useState<Record<string, boolean>>({})
  const [searchQuery, setSearchQuery] = useState("")

  // UI state
  const [saving, setSaving] = useState(false)
  const [validationErrors, setValidationErrors] = useState<string[]>([])

  // Reset form when dialog opens/closes
  useEffect(() => {
    if (open) {
      setCurrentStep(0)
      if (agent) {
        setName(agent.name)
        setRole(agent.role || 'Monitor')
        setUserPrompt(agent.user_prompt)
        // Parse schedule...
        if (agent.schedule) {
          const type = agent.schedule.schedule_type as AgentScheduleType
          // Parse timezone
          if (agent.schedule.timezone) {
            setTimezone(agent.schedule.timezone)
          }
          // Parse event filter
          if (agent.schedule.event_filter) {
            try {
              const filter = JSON.parse(agent.schedule.event_filter)
              if (filter.event_type) setEventType(filter.event_type)
              if (filter.device_id) setEventDeviceId(filter.device_id)
              if (filter.threshold) setEventThreshold(filter.threshold)
            } catch {
              // Ignore parse errors
            }
          }
          if (type === 'event') {
            setScheduleType('event')
          } else if (type === 'once') {
            setScheduleType('once')
          } else if (agent.schedule.cron_expression) {
            const cron = agent.schedule.cron_expression
            if (cron.startsWith('0 ')) {
              const parts = cron.split(' ')
              if (parts.length >= 5) {
                const hour = parseInt(parts[1]) || 9
                const minute = parseInt(parts[0]) || 0
                setScheduleHour(hour)
                setScheduleMinute(minute)
                if (parts[2] === '*') {
                  setScheduleType('daily')
                } else if (parts[2] !== '*' && parts[3] === '*') {
                  setWeekday(parseInt(parts[4]) || 1)
                  setScheduleType('weekly')
                } else {
                  setMonthDay(parseInt(parts[2]) || 1)
                  setScheduleType('monthly')
                }
              }
            }
          } else {
            setScheduleType('interval')
            if (agent.schedule.interval_seconds) {
              const seconds = agent.schedule.interval_seconds
              if (seconds >= 3600) {
                setIntervalUnit('hour')
                setIntervalValue(seconds / 3600)
              } else {
                setIntervalUnit('minute')
                setIntervalValue(seconds / 60)
              }
            }
          }
        }
        // Parse resources...
        if (agent.resources && agent.resources.length > 0) {
          const resourcesByDevice: Record<string, SelectedResource> = {}
          for (const resource of agent.resources) {
            const parts = resource.resource_id.split(':')
            if (parts.length < 2) continue
            const deviceId = parts[0]
            const resourceName = parts.slice(1).join(':')
            const device = devices.find(d => d.device_id === deviceId)
            if (!device) continue
            if (!resourcesByDevice[deviceId]) {
              resourcesByDevice[deviceId] = {
                deviceId,
                deviceName: device.name,
                deviceType: device.device_type,
                metrics: [],
                commands: []
              }
            }
            if (resource.resource_type === 'Metric') {
              resourcesByDevice[deviceId].metrics.push({
                name: resourceName,
                displayName: resource.name
              })
            } else if (resource.resource_type === 'Command') {
              resourcesByDevice[deviceId].commands.push({
                name: resourceName,
                displayName: resource.name
              })
            }
          }
          setSelectedResources(Object.values(resourcesByDevice))
          setExpandedDevices(new Set(Object.keys(resourcesByDevice)))
        }
      } else {
        setName("")
        setRole('Monitor')
        setUserPrompt("")
        setScheduleType('interval')
        setIntervalValue(5)
        setIntervalUnit('minute')
        setScheduleHour(9)
        setScheduleMinute(0)
        setWeekday(1)
        setMonthDay(1)
        setEventType('device.online')
        setEventDeviceId('all')
        setEventThreshold('')
        setTimezone(Intl.DateTimeFormat().resolvedOptions().timeZone)
        setSelectedResources([])
        setExpandedDevices(new Set())
      }
      setMetricsCache({})
      setLoadingMetrics({})
      setValidationErrors([])
      setSearchQuery("")
    }
  }, [agent, open, devices])

  // Step validation
  const validateStep = (step: number): boolean => {
    const errors: string[] = []

    if (step === 0) {
      if (!name.trim()) errors.push(t('creator.validation.nameRequired'))
      if (!userPrompt.trim()) errors.push(t('creator.validation.requirementRequired'))
    } else if (step === 1) {
      // Schedule is always valid (has defaults)
    } else if (step === 2) {
      if (selectedResources.length === 0) errors.push(t('creator.validation.resourceRequired'))
    }

    setValidationErrors(errors)
    return errors.length === 0
  }

  const handleNext = () => {
    if (validateStep(currentStep) && currentStep < steps.length - 1) {
      setCurrentStep(currentStep + 1)
    }
  }

  const handleBack = () => {
    if (currentStep > 0) {
      setCurrentStep(currentStep - 1)
    }
  }

  const handleSave = async () => {
    if (!validateStep(currentStep)) return

    setSaving(true)
    try {
      let cronExpression: string | undefined = undefined
      let intervalSeconds: number | undefined = undefined
      let finalScheduleType: AgentScheduleType = 'interval'

      if (scheduleType === 'interval') {
        finalScheduleType = 'interval'
        intervalSeconds = intervalUnit === 'hour'
          ? intervalValue * 3600
          : intervalValue * 60
      } else if (scheduleType === 'daily') {
        cronExpression = `${scheduleMinute} ${scheduleHour} * * *`
        finalScheduleType = 'cron'
      } else if (scheduleType === 'weekly') {
        cronExpression = `${scheduleMinute} ${scheduleHour} * * ${weekday}`
        finalScheduleType = 'cron'
      } else if (scheduleType === 'monthly') {
        cronExpression = `${scheduleMinute} ${scheduleHour} ${monthDay} * *`
        finalScheduleType = 'cron'
      } else if (scheduleType === 'event') {
        finalScheduleType = 'event'
      } else if (scheduleType === 'once') {
        finalScheduleType = 'once'
      }

      // Build event filter
      let eventFilter: string | undefined = undefined
      if (scheduleType === 'event') {
        const filterData: Record<string, unknown> = {
          event_type: eventType,
        }
        if (eventDeviceId !== 'all') {
          filterData.device_id = eventDeviceId
        }
        if (eventThreshold.trim()) {
          filterData.threshold = eventThreshold.trim()
        }
        eventFilter = JSON.stringify(filterData)
      }

      if (isEditing && agent) {
        await onSave({ name: name.trim(), role, user_prompt: userPrompt.trim() })
      } else {
        const data: CreateAgentRequest = {
          name: name.trim(),
          role,
          user_prompt: userPrompt.trim(),
          device_ids: selectedResources.map(r => r.deviceId),
          metrics: selectedResources.flatMap(r =>
            r.metrics.map(m => ({ device_id: r.deviceId, metric_name: m.name, display_name: m.displayName }))
          ),
          commands: selectedResources.flatMap(r =>
            r.commands.map(c => ({ device_id: r.deviceId, command_name: c.name, display_name: c.displayName, parameters: {} }))
          ),
          schedule: {
            schedule_type: finalScheduleType,
            interval_seconds: intervalSeconds,
            cron_expression: cronExpression,
            timezone: timezone,
            event_filter: eventFilter,
          },
        }
        await onSave(data)
      }
      onOpenChange(false)
      toast({ title: t('creator.actions.success'), description: isEditing ? t('creator.actions.updated') : t('creator.actions.created') })
    } catch (error) {
      toast({ title: t('creator.actions.failed'), description: (error as Error).message, variant: "destructive" })
    } finally {
      setSaving(false)
    }
  }

  // Progress indicator
  const StepProgress = () => (
    <div className="flex items-center justify-center gap-2 px-6 py-4">
      {steps.map((step, index) => (
        <div key={step} className="flex items-center">
          <button
            type="button"
            onClick={() => index < currentStep && setCurrentStep(index)}
            disabled={index >= currentStep || saving}
            className={cn(
              "flex items-center gap-2 px-3 py-1.5 rounded-full text-sm font-medium transition-all",
              index === currentStep
                ? "bg-primary text-primary-foreground"
                : index < currentStep
                  ? "bg-primary/10 text-primary hover:bg-primary/20"
                  : "bg-muted text-muted-foreground"
            )}
          >
            {index < currentStep ? (
              <Check className="h-3.5 w-3.5" />
            ) : (
              <span className="w-3.5 h-3.5 flex items-center justify-center text-xs">{index + 1}</span>
            )}
            <span>{step}</span>
          </button>
          {index < steps.length - 1 && (
            <div className={cn(
              "w-8 h-px mx-1",
              index < currentStep ? "bg-primary" : "bg-border"
            )} />
          )}
        </div>
      ))}
    </div>
  )

  return (
    <Dialog open={open} onOpenChange={(v) => !saving && onOpenChange(v)}>
      <DialogContent className="max-w-4xl h-[90vh] p-0 gap-0 flex flex-col [&>[data-radix-dialog-close]]:right-4 [&>[data-radix-dialog-close]]:top-4">
        <DialogHeader className="px-6 py-4 border-b shrink-0">
          <DialogTitle className="text-lg">
            {isEditing ? t('creator.editTitle') : t('creator.title')}
          </DialogTitle>
        </DialogHeader>

        {/* Progress */}
        <StepProgress />

        {/* Content */}
        <div className="flex-1 overflow-y-auto px-6 py-5">
          {/* Validation Errors */}
          {validationErrors.length > 0 && (
            <div className="mb-4 p-3 bg-destructive/10 border border-destructive/20 rounded-lg text-sm text-destructive flex items-start gap-2">
              <X className="h-4 w-4 shrink-0 mt-0.5" />
              <span>{validationErrors.join('，')}</span>
            </div>
          )}

          {/* Desktop: Step content */}
          <div className="hidden lg:block">
            {currentStep === 0 && (
              <BasicInfoStep
                name={name}
                setName={setName}
                role={role}
                setRole={setRole}
                userPrompt={userPrompt}
                setUserPrompt={setUserPrompt}
              />
            )}
            {currentStep === 1 && (
              <ScheduleStep
                scheduleType={scheduleType}
                setScheduleType={setScheduleType}
                intervalValue={intervalValue}
                setIntervalValue={setIntervalValue}
                intervalUnit={intervalUnit}
                setIntervalUnit={setIntervalUnit}
                scheduleHour={scheduleHour}
                setScheduleHour={setScheduleHour}
                scheduleMinute={scheduleMinute}
                setScheduleMinute={setScheduleMinute}
                weekday={weekday}
                setWeekday={setWeekday}
                monthDay={monthDay}
                setMonthDay={setMonthDay}
                eventType={eventType}
                setEventType={setEventType}
                eventDeviceId={eventDeviceId}
                setEventDeviceId={setEventDeviceId}
                eventThreshold={eventThreshold}
                setEventThreshold={setEventThreshold}
                selectedResources={selectedResources}
                timezone={timezone}
                setTimezone={setTimezone}
              />
            )}
            {currentStep === 2 && (
              <ResourceStep
                devices={devices}
                deviceTypes={deviceTypes}
                selectedResources={selectedResources}
                setSelectedResources={setSelectedResources}
                expandedDevices={expandedDevices}
                setExpandedDevices={setExpandedDevices}
                metricsCache={metricsCache}
                setMetricsCache={setMetricsCache}
                loadingMetrics={loadingMetrics}
                setLoadingMetrics={setLoadingMetrics}
                searchQuery={searchQuery}
                setSearchQuery={setSearchQuery}
                role={role}
              />
            )}
          </div>

          {/* Mobile: Tab-based */}
          <div className="lg:hidden">
            <Tabs value={currentStep.toString()} onValueChange={(v) => setCurrentStep(parseInt(v))}>
              <TabsList className="grid w-full grid-cols-3 h-auto p-1 mb-4">
                {steps.map((step, index) => (
                  <TabsTrigger
                    key={step}
                    value={index.toString()}
                    className="gap-1.5 data-[state=active]:bg-primary data-[state=active]:text-primary-foreground"
                  >
                    {index < currentStep ? (
                      <Check className="h-3.5 w-3.5" />
                    ) : (
                      <span className="text-xs">{index + 1}</span>
                    )}
                    <span className="text-xs">{step}</span>
                  </TabsTrigger>
                ))}
              </TabsList>

              <TabsContent value="0">
                <BasicInfoStep
                  name={name}
                  setName={setName}
                  role={role}
                  setRole={setRole}
                  userPrompt={userPrompt}
                  setUserPrompt={setUserPrompt}
                />
              </TabsContent>
              <TabsContent value="1">
                <ScheduleStep
                  scheduleType={scheduleType}
                  setScheduleType={setScheduleType}
                  intervalValue={intervalValue}
                  setIntervalValue={setIntervalValue}
                  intervalUnit={intervalUnit}
                  setIntervalUnit={setIntervalUnit}
                  scheduleHour={scheduleHour}
                  setScheduleHour={setScheduleHour}
                  scheduleMinute={scheduleMinute}
                  setScheduleMinute={setScheduleMinute}
                  weekday={weekday}
                  setWeekday={setWeekday}
                  monthDay={monthDay}
                  setMonthDay={setMonthDay}
                  eventType={eventType}
                  setEventType={setEventType}
                  eventDeviceId={eventDeviceId}
                  setEventDeviceId={setEventDeviceId}
                  selectedResources={selectedResources}
                />
              </TabsContent>
              <TabsContent value="2">
                <ResourceStep
                  devices={devices}
                  deviceTypes={deviceTypes}
                  selectedResources={selectedResources}
                  setSelectedResources={setSelectedResources}
                  expandedDevices={expandedDevices}
                  setExpandedDevices={setExpandedDevices}
                  metricsCache={metricsCache}
                  setMetricsCache={setMetricsCache}
                  loadingMetrics={loadingMetrics}
                  setLoadingMetrics={setLoadingMetrics}
                  searchQuery={searchQuery}
                  setSearchQuery={setSearchQuery}
                  role={role}
                />
              </TabsContent>
            </Tabs>
          </div>
        </div>

        {/* Footer */}
        <div className="flex items-center justify-between px-6 py-4 border-t bg-muted/20 shrink-0">
          <Button
            variant="ghost"
            size="sm"
            onClick={() => onOpenChange(false)}
            disabled={saving}
          >
            {t('creator.actions.cancel')}
          </Button>
          <div className="flex items-center gap-2">
            {currentStep > 0 && (
              <Button
                type="button"
                variant="outline"
                size="sm"
                onClick={handleBack}
                disabled={saving}
              >
                <ChevronLeft className="h-4 w-4 mr-1" />
                {t('creator.actions.previous')}
              </Button>
            )}
            {currentStep < steps.length - 1 ? (
              <Button type="button" onClick={handleNext}>
                {t('creator.actions.next')}
                <ChevronRight className="h-4 w-4 ml-1" />
              </Button>
            ) : (
              <Button type="button" onClick={handleSave} disabled={saving}>
                {saving ? (
                  <>
                    <Loader2 className="h-4 w-4 mr-2 animate-spin" />
                    {t('creator.actions.saving')}
                  </>
                ) : (
                  <>
                    <Save className="h-4 w-4 mr-2" />
                    {isEditing ? t('creator.actions.save') : t('creator.actions.create')}
                  </>
                )}
              </Button>
            )}
          </div>
        </div>
      </DialogContent>
    </Dialog>
  )
}
