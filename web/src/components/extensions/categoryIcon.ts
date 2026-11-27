import {
  Code2,
  Terminal,
  Database,
  Eye,
  Plug,
  CloudSun,
  Cpu,
  Factory,
  Wifi,
  Camera,
  Brain,
  MonitorPlay,
  ScanText,
  UserCheck,
  type LucideIcon,
} from "lucide-react"

/**
 * Pick a scannable category icon from the extension id/name keywords.
 * Backend sends no category/icon field, so heuristics keep grids distinguishable.
 */
export function categoryIcon(id: string, name: string): LucideIcon {
  const haystack = `${id} ${name}`.toLowerCase()
  const pairs: [RegExp, LucideIcon][] = [
    [/vision|yolo|object.?detect|image|analy|ocr|face/, Eye],
    [/bridge|modbus|opc.?ua|bacnet|mqtt|lorawan|ha\b|home.?assist/, Plug],
    [/weather|forecast/, CloudSun],
    [/stream|player|rtsp|video|media/, MonitorPlay],
    [/onvif|camera|ip.?cam/, Camera],
    [/transcri|speech|voice|asr|tts/, Brain],
    [/scan|text|ocr/, ScanText],
    [/ident|recogn/, UserCheck],
    [/demo|core|runtime/, Cpu],
    [/industrial|scada|plc/, Factory],
    [/wifi|sensor|iot/, Wifi],
    [/push|export|telemetry|notif/, Database],
    [/tool|command|cli|skill/, Terminal],
  ]
  for (const [re, icon] of pairs) {
    if (re.test(haystack)) return icon
  }
  return Code2
}
