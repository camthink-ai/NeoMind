"""Small-sample statistics for gate verdicts.

A gate run is a proportion over N rounds, not a measurement with negligible
error: 80 rounds at ~75% carries a 95% interval roughly ±10pp. Reporting a
bare percentage invites over-reading 3-5pp differences that the sample cannot
resolve (2026-10: Spark 73.8% vs MiniCPM-V 70.5% was called a ranking on 80
rounds — the interval is wider than the gap). Every gate summary therefore
prints a Wilson interval and the implied resolution.
"""
from __future__ import annotations

import math


def wilson_interval(passes: int, total: int, z: float = 1.96) -> tuple[float, float]:
    """95% Wilson score interval for a binomial proportion.

    Preferred over the normal approximation at small n / extreme p (gate runs
    land near 0.7-0.95, where the normal interval understates the upper tail).
    """
    if total <= 0:
        return (0.0, 1.0)
    return wilson_interval_for(passes / total, total, z)


def wilson_interval_for(p: float, n: int, z: float = 1.96) -> tuple[float, float]:
    """Wilson interval for a proportion `p` observed over `n` trials."""
    if n <= 0:
        return (0.0, 1.0)
    p = min(1.0, max(0.0, p))
    denom = 1 + z * z / n
    center = (p + z * z / (2 * n)) / denom
    half = (z * math.sqrt(p * (1 - p) / n + z * z / (4 * n * n))) / denom
    return (max(0.0, center - half), min(1.0, center + half))


def resolution_pp(passes: int, total: int) -> float:
    """Half-width of the 95% interval, in percentage points.

    Two runs whose rates differ by less than roughly twice this are not
    distinguishable at this sample size — treat them as ties.
    """
    lo, hi = wilson_interval(passes, total)
    return (hi - lo) * 50.0


def rounds_for(p: float, target_pp: float, z: float = 1.96) -> int:
    """Trials needed for a ±target_pp 95% Wilson half-width at a rate of p.

    Numerical search, not the normal approximation: at p near 0 or 1 the
    normal formula collapses to 0 trials, which is exactly wrong — an observed
    100% still has real uncertainty (47/47 carries ±3.8pp).
    """
    if target_pp <= 0:
        raise ValueError("target_pp must be positive")
    target = target_pp / 100.0
    lo, hi = 1, 2_000_000
    while lo < hi:
        mid = (lo + hi) // 2
        a, b = wilson_interval_for(p, mid, z)
        if (b - a) / 2 <= target:
            hi = mid
        else:
            lo = mid + 1
    return lo
