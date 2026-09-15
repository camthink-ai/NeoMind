import { memo, useEffect, useRef } from "react"

/**
 * Network background — the product drawn as its own metaphor.
 *
 * NeoMind is an edge hub: devices (MQTT/BLE) link to it, AI agents run on
 * it, telemetry streams through it. The background IS that picture —
 * drifting nodes (devices/agents), faint links between neighbors (data
 * paths), and brand-orange pulses travelling the links (live telemetry).
 *
 * Two design attempts preceded this: a honeycomb breathe (imperceptible)
 * and an aurora gradient (pretty but generic — could belong to any SaaS).
 * The network is specific to what the product does.
 *
 * Performance notes (the login page is first paint inside Tauri's
 * WKWebView): node count scales with area but stays clamped (24–56);
 * pulses draw via a pre-rendered radial-gradient sprite instead of
 * ctx.shadowBlur (which is slow in WKWebView); the rAF loop pauses when
 * the tab hides; `prefers-reduced-motion` renders one static frame.
 * Colors are read from theme CSS vars and re-read when the html class
 * flips (dark ⇄ light) so both themes stay honest — the lesson from the
 * aurora iteration where light mode silently lost its texture.
 */
export const NetworkBackground = memo(function NetworkBackground() {
  const canvasRef = useRef<HTMLCanvasElement>(null)

  useEffect(() => {
    const canvas = canvasRef.current
    if (!canvas) return
    const ctx = canvas.getContext("2d")
    if (!ctx) return

    let raf = 0
    let width = 0
    let height = 0
    let dpr = 1

    type Node = { x: number; y: number; vx: number; vy: number; hub: boolean; flash: number }
    type Pulse = { a: number; b: number; t: number; speed: number }

    let nodes: Node[] = []
    let pulses: Pulse[] = []
    let nextPulseAt = 0

    // Theme colors, resolved from CSS vars; re-read on class flip.
    // Theme colors. Deliberately HEX/RGBA literals, NOT the theme's CSS
    // vars: the vars resolve to `oklch(...)` strings, and appending an
    // alpha suffix for gradient stops (`${color}cc`) yields an illegal
    // canvas color — the first version did exactly that, and the
    // resulting exception killed the rAF loop the moment a pulse
    // spawned, freezing the network with no pulses ever visible. Hex
    // values mirror the tokens (--accent-orange oklch(0.55 0.22 37) ≈
    // #f83c00); if the tokens change, update these.
    // Light bases also need stronger marks than dark bases at the same
    // "feels subtle" level, and light-theme pulses go ember-deep — the
    // light accent orange melts into the warm base.
    const DARK = {
      node: "rgba(255,255,255,0.30)",
      hub: "rgba(255,255,255,0.55)",
      edge: "rgba(255,255,255,0.06)",
      pulse: "#f83c00",
    }
    const LIGHT = {
      node: "rgba(30,30,40,0.42)",
      hub: "rgba(30,30,40,0.50)",
      edge: "rgba(30,30,40,0.13)",
      pulse: "#d13c00",
    }
    const colors = { ...DARK }
    const sprite = document.createElement("canvas")
    const SPRITE = 48
    sprite.width = sprite.height = SPRITE

    const readTheme = () => {
      const light = document.documentElement.classList.contains("light")
      Object.assign(colors, light ? LIGHT : DARK)
      // Pre-render the pulse glow sprite in the pulse color.
      const sctx = sprite.getContext("2d")
      if (sctx) {
        sctx.clearRect(0, 0, SPRITE, SPRITE)
        const g = sctx.createRadialGradient(
          SPRITE / 2, SPRITE / 2, 0,
          SPRITE / 2, SPRITE / 2, SPRITE / 2,
        )
        g.addColorStop(0, colors.pulse)
        g.addColorStop(0.25, `${colors.pulse}cc`)
        g.addColorStop(1, `${colors.pulse}00`)
        sctx.fillStyle = g
        sctx.fillRect(0, 0, SPRITE, SPRITE)
      }
    }
    readTheme()

    const themeObserver = new MutationObserver(readTheme)
    themeObserver.observe(document.documentElement, {
      attributes: true,
      attributeFilter: ["class"],
    })

    const seed = () => {
      const count = Math.max(24, Math.min(56, Math.round((width * height) / 34000)))
      nodes = Array.from({ length: count }, () => ({
        x: Math.random() * width,
        y: Math.random() * height,
        vx: (Math.random() - 0.5) * 0.12,
        vy: (Math.random() - 0.5) * 0.12,
        hub: Math.random() < 0.18,
        flash: 0,
      }))
      pulses = []
      nextPulseAt = performance.now() + 500
    }

    const resize = () => {
      dpr = Math.min(window.devicePixelRatio || 1, 2)
      width = canvas.clientWidth
      height = canvas.clientHeight
      canvas.width = Math.round(width * dpr)
      canvas.height = Math.round(height * dpr)
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0)
      seed()
    }
    resize()
    const ro = new ResizeObserver(resize)
    ro.observe(canvas)

    const LINK_DIST = 170
    // Adjacency is recomputed each frame (needed for pulse travel anyway).
    const edges: Array<[number, number]> = []
    const buildEdges = () => {
      edges.length = 0
      for (let i = 0; i < nodes.length; i++) {
        for (let j = i + 1; j < nodes.length; j++) {
          const dx = nodes[i].x - nodes[j].x
          const dy = nodes[i].y - nodes[j].y
          if (dx * dx + dy * dy < LINK_DIST * LINK_DIST) edges.push([i, j])
        }
      }
    }

    const drawStatic = (animate: boolean) => {
      ctx.clearRect(0, 0, width, height)
      buildEdges()

      ctx.lineWidth = 1
      for (const [i, j] of edges) {
        ctx.strokeStyle = colors.edge
        ctx.beginPath()
        ctx.moveTo(nodes[i].x, nodes[i].y)
        ctx.lineTo(nodes[j].x, nodes[j].y)
        ctx.stroke()
      }

      for (const n of nodes) {
        ctx.beginPath()
        ctx.arc(n.x, n.y, n.hub ? 2.2 : 1.4, 0, Math.PI * 2)
        ctx.fillStyle = n.hub ? colors.hub : colors.node
        ctx.fill()
        if (n.flash > 0) {
          const r = 4 + (1 - n.flash) * 8
          ctx.globalAlpha = n.flash
          ctx.drawImage(sprite, n.x - r, n.y - r, r * 2, r * 2)
          ctx.globalAlpha = 1
        }
      }

      for (const p of pulses) {
        const a = nodes[p.a]
        const b = nodes[p.b]
        if (!a || !b) continue
        const x = a.x + (b.x - a.x) * p.t
        const y = a.y + (b.y - a.y) * p.t
        const r = 7
        // Trail: a short fading segment behind the pulse.
        const tt = Math.max(0, p.t - 0.12)
        const tx = a.x + (b.x - a.x) * tt
        const ty = a.y + (b.y - a.y) * tt
        const grad = ctx.createLinearGradient(tx, ty, x, y)
        grad.addColorStop(0, `${colors.pulse}00`)
        grad.addColorStop(1, `${colors.pulse}66`)
        ctx.strokeStyle = grad
        ctx.lineWidth = 1.5
        ctx.beginPath()
        ctx.moveTo(tx, ty)
        ctx.lineTo(x, y)
        ctx.stroke()
        ctx.drawImage(sprite, x - r, y - r, r * 2, r * 2)
      }

      if (!animate) return
      // Advance state for the next frame.
      for (const n of nodes) {
        n.x += n.vx
        n.y += n.vy
        if (n.flash > 0) n.flash = Math.max(0, n.flash - 0.03)
        if (n.x < -20 || n.x > width + 20) n.vx *= -1
        if (n.y < -20 || n.y > height + 20) n.vy *= -1
      }
      const now = performance.now()
      if (now >= nextPulseAt && edges.length > 0) {
        const [a, b] = edges[(Math.random() * edges.length) | 0]
        pulses.push({ a, b, t: 0, speed: 0.006 + Math.random() * 0.008 })
        nextPulseAt = now + 350 + Math.random() * 650
      }
      for (let i = pulses.length - 1; i >= 0; i--) {
        const p = pulses[i]
        p.t += p.speed
        if (p.t >= 1) {
          const dest = nodes[p.b]
          if (dest) dest.flash = 1
          pulses.splice(i, 1)
        }
      }
    }

    const reduced = window.matchMedia("(prefers-reduced-motion: reduce)").matches
    const loop = () => {
      drawStatic(true)
      raf = requestAnimationFrame(loop)
    }
    if (reduced) {
      drawStatic(false)
    } else {
      raf = requestAnimationFrame(loop)
    }

    const onVisibility = () => {
      if (reduced) return
      if (document.hidden) {
        cancelAnimationFrame(raf)
      } else {
        raf = requestAnimationFrame(loop)
      }
    }
    document.addEventListener("visibilitychange", onVisibility)

    return () => {
      cancelAnimationFrame(raf)
      ro.disconnect()
      themeObserver.disconnect()
      document.removeEventListener("visibilitychange", onVisibility)
    }
  }, [])

  return (
    <div className="absolute inset-0 overflow-hidden" aria-hidden>
      {/* Soft brand warmth behind the card so the network floats in a
          space rather than on a void — the one thing the pre-network
          designs got right, kept. */}
      <div
        className="absolute top-[38%] left-1/2 -translate-x-1/2 w-[46rem] h-[34rem] rounded-full blur-3xl"
        style={{
          background:
            "color-mix(in oklch, var(--accent-orange) 9%, transparent)",
        }}
      />
      <canvas
        ref={canvasRef}
        className="absolute inset-0 h-full w-full"
        style={{
          // Calm center: the mesh frames the card instead of running
          // through it. Same masking family as the previous designs.
          maskImage:
            "radial-gradient(ellipse 70% 62% at 50% 45%, transparent 16%, black 55%)",
          WebkitMaskImage:
            "radial-gradient(ellipse 70% 62% at 50% 45%, transparent 16%, black 55%)",
        }}
      />
    </div>
  )
})

/* hmr-ping */
