/**
 * Scroll Performance Debugger for Tauri/WKWebView
 *
 * 使用方法:
 *   1. 控制台: localStorage.setItem('DEBUG_SCROLL', 'true') 然后刷新
 *   2. 滚动 dashboard，观察右下角
 *   3. 关闭: localStorage.removeItem('DEBUG_SCROLL')
 *
 * 诊断项:
 * - 实时 FPS
 * - 每帧耗时 (frame budget = 16ms)
 * - 长任务 (>50ms)
 * - ResizeObserver 触发次数
 * - DOM 节点数
 *
 * 注意: 合成层数量计数已移除 (countLayers 会遍历 6000+ DOM 元素
 * 调用 getComputedStyle，本身就会造成 3000-6000ms 卡顿)
 */

let overlay: HTMLDivElement | null = null
let fps = 0
let frameCount = 0
let lastFpsTime = performance.now()
let maxFrameTime = 0
let avgFrameTime = 0
const frameTimes: number[] = []
const longTasks: { name: string; duration: number; time: string }[] = []
let resizeObserverFires = 0
let scrollEvents = 0
let isScrolling = false
let scrollStartFps = 0
let domNodes = 0

function updateOverlay() {
  if (!overlay) return

  const recentTasks = longTasks.slice(-4)
  const tasksHtml = recentTasks.length === 0
    ? '<div style="color:#4ade80">None</div>'
    : recentTasks.map(t =>
        `<div style="color:#f87171">${t.time} ${t.name}: ${t.duration}ms</div>`
      ).join('')

  const scrollBadge = isScrolling
    ? '<span style="color:#fbbf24;font-weight:bold">SCROLLING</span>'
    : '<span style="color:#64748b">idle</span>'

  overlay.innerHTML = `
    <div style="font-family:ui-monospace,monospace;font-size:11px;line-height:1.5">
      <div style="font-weight:bold;margin-bottom:4px;color:#e2e8f0">
        Debug ${scrollBadge}
      </div>
      <div>FPS: <span style="color:${fps >= 55 ? '#4ade80' : fps >= 30 ? '#fbbf24' : '#f87171'};font-weight:bold">${fps}</span>
        ${isScrolling ? `(start: ${scrollStartFps})` : ''}
      </div>
      <div>Frame: avg=${avgFrameTime}ms max=${maxFrameTime}ms (budget: 16ms)</div>
      <div>DOM: ${domNodes}</div>
      <div>RO: ${resizeObserverFires} | ScrollEv: ${scrollEvents}</div>
      <div style="margin-top:4px;font-weight:bold;border-top:1px solid #334155;padding-top:4px">
        Long Tasks:
      </div>
      ${tasksHtml}
    </div>
  `
}

export function initScrollDebugger() {
  if (typeof window === 'undefined') return
  if (document.getElementById('scroll-debugger')) return

  overlay = document.createElement('div')
  overlay.id = 'scroll-debugger'
  overlay.style.cssText = `
    position:fixed;bottom:12px;right:12px;z-index:99999;
    background:rgba(0,0,0,0.92);color:#e2e8f0;padding:12px 16px;
    border-radius:10px;pointer-events:none;min-width:260px;max-width:360px;
    border:1px solid rgba(255,255,255,0.15);font-size:11px;
  `
  document.body.appendChild(overlay)

  // FPS + frame time measurement
  let lastFrameTime = performance.now()
  function measureFrame() {
    const now = performance.now()
    const dt = now - lastFrameTime
    lastFrameTime = now
    frameCount++
    frameTimes.push(dt)
    if (frameTimes.length > 60) frameTimes.shift()
    // Log frame spikes to console for diagnosis
    if (dt > 500) {
      console.warn(`[perf] Frame spike: ${Math.round(dt)}ms at ${new Date().toLocaleTimeString('en', { hour12: false })}`)
    }
    requestAnimationFrame(measureFrame)
  }
  requestAnimationFrame(measureFrame)

  // Update stats every 500ms (lightweight — only counts DOM nodes, no getComputedStyle)
  setInterval(() => {
    const now = performance.now()
    fps = Math.round(frameCount * 1000 / (now - lastFpsTime))
    lastFpsTime = now
    frameCount = 0

    // Frame time stats
    if (frameTimes.length > 0) {
      avgFrameTime = Math.round(frameTimes.reduce((a, b) => a + b, 0) / frameTimes.length)
      maxFrameTime = Math.round(frameTimes.reduce((a, b) => Math.max(a, b), -Infinity))
    }

    // DOM node count (fast — just .length, no style recalculation)
    domNodes = document.body ? document.body.getElementsByTagName('*').length : 0
    updateOverlay()
  }, 500)

  // Long task detection
  try {
    const observer = new PerformanceObserver((list) => {
      for (const entry of list.getEntries()) {
        if (entry.duration > 50) {
          const time = new Date().toLocaleTimeString('en', { hour12: false })
          longTasks.push({ name: entry.name || 'task', duration: Math.round(entry.duration), time })
          if (longTasks.length > 20) longTasks.shift()
          updateOverlay()
        }
      }
    })
    observer.observe({ type: 'longtask', buffered: false })
  } catch {
    try {
      const observer = new PerformanceObserver((list) => {
        for (const entry of list.getEntries()) {
          if (entry.duration > 50) {
            const time = new Date().toLocaleTimeString('en', { hour12: false })
            longTasks.push({ name: entry.name, duration: Math.round(entry.duration), time })
            if (longTasks.length > 20) longTasks.shift()
            updateOverlay()
          }
        }
      })
      observer.observe({ entryTypes: ['measure'] })
    } catch { /* ignore */ }
  }

  // Track scroll
  let scrollEndTimer: ReturnType<typeof setTimeout> | undefined
  document.addEventListener('scroll', () => {
    scrollEvents++
    if (!isScrolling) {
      isScrolling = true
      scrollStartFps = fps
    }
    if (scrollEndTimer) clearTimeout(scrollEndTimer)
    scrollEndTimer = setTimeout(() => {
      isScrolling = false
      updateOverlay()
    }, 300)
  }, true)

  // Track ResizeObserver
  const OrigRO = window.ResizeObserver
  window.ResizeObserver = class DebugResizeObserver extends OrigRO {
    constructor(callback: ResizeObserverCallback) {
      const wrapped: ResizeObserverCallback = (entries, obs) => {
        resizeObserverFires++
        callback(entries, obs)
      }
      super(wrapped)
    }
  }

  updateOverlay()
}

export function destroyScrollDebugger() {
  if (overlay) { overlay.remove(); overlay = null }
}

// Auto-init
if (typeof window !== 'undefined' && typeof localStorage !== 'undefined' && localStorage.getItem('DEBUG_SCROLL') === 'true') {
  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', () => initScrollDebugger())
  } else {
    setTimeout(initScrollDebugger, 100)
  }
}
