import { memo } from "react"

/**
 * Aurora background — three large blurred color fields drifting slowly
 * behind the auth surface, over a faint dot grid.
 *
 * Replaced the honeycomb breathe (ported from the landing page) whose
 * texture was tuned so low (stroke-opacity 0.015) it read as a plain
 * black page on most displays. The aurora keeps the same restraint —
 * brand orange dominant, a warm ember echo, a whisper of the purple
 * accent for depth — but stays perceptible at a glance.
 *
 * Performance: only transform/opacity animate (GPU-composited); the
 * blur is a static filter. `prefers-reduced-motion` freezes the drift.
 * Shared by the login + setup pages so the auth flow reads as one
 * surface.
 */
export const AuroraBackground = memo(function AuroraBackground() {
  return (
    <div className="absolute inset-0 overflow-hidden" aria-hidden>
      <style>{`
        @keyframes aurora-drift-a {
          0%, 100% { transform: translate3d(-6%, -4%, 0) scale(1); }
          50%      { transform: translate3d(5%, 6%, 0) scale(1.15); }
        }
        @keyframes aurora-drift-b {
          0%, 100% { transform: translate3d(4%, 5%, 0) scale(1.1); }
          50%      { transform: translate3d(-5%, -6%, 0) scale(0.92); }
        }
        @keyframes aurora-drift-c {
          0%, 100% { transform: translate3d(0%, 6%, 0) scale(1); opacity: 0.65; }
          50%      { transform: translate3d(3%, -5%, 0) scale(1.2); opacity: 0.9; }
        }
        .aurora-blob {
          position: absolute;
          border-radius: 9999px;
          filter: blur(90px);
          will-change: transform;
        }
        .aurora-blob-a {
          animation: aurora-drift-a 38s ease-in-out infinite;
        }
        .aurora-blob-b {
          animation: aurora-drift-b 46s ease-in-out infinite;
          animation-delay: -12s;
        }
        .aurora-blob-c {
          animation: aurora-drift-c 54s ease-in-out infinite;
          animation-delay: -25s;
        }
        @media (prefers-reduced-motion: reduce) {
          .aurora-blob { animation: none !important; }
        }
      `}</style>

      {/* Dominant brand-orange field — sits behind the card, a little
          below center, echoing the glow the layout already had. */}
      <div
        className="aurora-blob aurora-blob-a"
        style={{
          top: "34%",
          left: "50%",
          width: "min(72rem, 90vw)",
          height: "min(52rem, 70vh)",
          marginLeft: "min(-36rem, -45vw)",
          background:
            "color-mix(in oklch, var(--accent-orange) 24%, transparent)",
        }}
      />
      {/* Warm ember echo — smaller, cooler depth on the opposite drift. */}
      <div
        className="aurora-blob aurora-blob-b"
        style={{
          top: "12%",
          left: "62%",
          width: "min(40rem, 60vw)",
          height: "min(34rem, 50vh)",
          background:
            "color-mix(in oklch, var(--accent-orange) 16%, transparent)",
        }}
      />
      {/* Purple whisper — the chart primary accent at trace level, gives
          the dark field depth instead of a flat void. */}
      <div
        className="aurora-blob aurora-blob-c"
        style={{
          bottom: "6%",
          left: "18%",
          width: "min(46rem, 60vw)",
          height: "min(30rem, 45vh)",
          background:
            "color-mix(in oklch, var(--chart-1, oklch(0.65 0.18 300)) 19%, transparent)",
        }}
      />

      {/* Dot grid — the tech texture the honeycomb used to carry, at a
          level that is actually visible: dots every 28px, masked to fade
          toward the edges so it frames rather than tiles. */}
      <div
        className="absolute inset-0"
        style={{
          backgroundImage:
            "radial-gradient(circle, color-mix(in oklch, var(--foreground) 22%, transparent) 1px, transparent 1px)",
          backgroundSize: "28px 28px",
          maskImage:
            "radial-gradient(ellipse 75% 65% at 50% 45%, black 25%, transparent 80%)",
          WebkitMaskImage:
            "radial-gradient(ellipse 75% 65% at 50% 45%, black 25%, transparent 80%)",
          opacity: 0.5,
        }}
      />
    </div>
  )
})
