#!/usr/bin/env python3
"""Compare two Criterion result trees measured on the same host."""

from __future__ import annotations

import argparse
import json
import math
import sys
from pathlib import Path


def load_results(root: Path) -> dict[str, float]:
    if not root.is_dir():
        raise SystemExit(f"criterion directory not found: {root}")

    results: dict[str, float] = {}
    for estimates in sorted(root.rglob("new/estimates.json")):
        relative = estimates.relative_to(root)
        name = "/".join(relative.parts[:-2])
        if not name:
            continue
        try:
            data = json.loads(estimates.read_text())
            results[name] = float(data["median"]["point_estimate"])
        except (OSError, ValueError, KeyError) as error:
            print(f"warning: skipping {estimates}: {error}", file=sys.stderr)
    return results


def format_ns(ns: float) -> str:
    if ns >= 1_000_000_000:
        return f"{ns / 1_000_000_000:.3f} s"
    if ns >= 1_000_000:
        return f"{ns / 1_000_000:.3f} ms"
    if ns >= 1_000:
        return f"{ns / 1_000:.3f} µs"
    return f"{ns:.1f} ns"


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("baseline", type=Path, help="baseline target/criterion directory")
    parser.add_argument("candidate", type=Path, help="candidate target/criterion directory")
    parser.add_argument("--threshold", type=float, default=10.0, help="noise threshold in percent")
    parser.add_argument("--markdown", type=Path, help="write the Markdown report here")
    parser.add_argument("--json", type=Path, help="write the machine-readable report here")
    parser.add_argument("--title", default="Same-host benchmark A/B")
    args = parser.parse_args()

    baseline = load_results(args.baseline)
    candidate = load_results(args.candidate)
    common = sorted(set(baseline) & set(candidate))
    if not common:
        raise SystemExit("no common benchmarks between baseline and candidate")

    rows = []
    ratios = []
    for name in common:
        base_ns = baseline[name]
        cand_ns = candidate[name]
        if base_ns <= 0:
            print(f"warning: non-positive baseline for {name}", file=sys.stderr)
            continue
        delta = (cand_ns / base_ns - 1.0) * 100.0
        ratios.append(cand_ns / base_ns)
        if delta >= args.threshold:
            status = "regression"
        elif delta <= -args.threshold:
            status = "improvement"
        else:
            status = "stable"
        rows.append((name, base_ns, cand_ns, delta, status))

    rows.sort(key=lambda row: row[3], reverse=True)
    regressions = [row[0] for row in rows if row[4] == "regression"]
    improvements = [row[0] for row in rows if row[4] == "improvement"]
    geometric_mean = (math.exp(sum(math.log(r) for r in ratios) / len(ratios)) - 1.0) * 100.0

    missing_in_baseline = sorted(set(candidate) - set(baseline))
    missing_in_candidate = sorted(set(baseline) - set(candidate))

    lines = [
        f"# {args.title}",
        "",
        f"Baseline `{args.baseline}` · candidate `{args.candidate}` · "
        f"threshold ±{args.threshold:g}% · {len(rows)} comparable benchmarks",
        "",
        "| Benchmark | Baseline | Candidate | Delta | Verdict |",
        "|---|---:|---:|---:|---|",
    ]
    for name, base_ns, cand_ns, delta, status in rows:
        lines.append(
            f"| `{name}` | {format_ns(base_ns)} | {format_ns(cand_ns)} | "
            f"{delta:+.1f}% | {status} |"
        )

    lines += [
        "",
        f"**Summary:** {len(regressions)} regression(s), {len(improvements)} improvement(s), "
        f"{len(rows) - len(regressions) - len(improvements)} stable · "
        f"geometric mean {geometric_mean:+.1f}% (positive = slower)",
    ]
    if regressions:
        lines += ["", "**Regressions:** " + ", ".join(f"`{name}`" for name in regressions)]
    if improvements:
        lines += ["", "**Improvements:** " + ", ".join(f"`{name}`" for name in improvements)]
    if missing_in_baseline:
        lines += ["", "**Only in candidate:** " + ", ".join(f"`{name}`" for name in missing_in_baseline)]
    if missing_in_candidate:
        lines += ["", "**Only in baseline:** " + ", ".join(f"`{name}`" for name in missing_in_candidate)]

    markdown = "\n".join(lines) + "\n"
    if args.markdown:
        args.markdown.write_text(markdown)
    print(markdown, end="")

    if args.json:
        payload = {
            "baseline": str(args.baseline),
            "candidate": str(args.candidate),
            "threshold_pct": args.threshold,
            "benchmark_count": len(rows),
            "geometric_mean_delta_pct": geometric_mean,
            "regressions": regressions,
            "improvements": improvements,
            "missing_in_baseline": missing_in_baseline,
            "missing_in_candidate": missing_in_candidate,
            "benchmarks": {
                name: {
                    "baseline_ns": base_ns,
                    "candidate_ns": cand_ns,
                    "delta_pct": delta,
                    "status": status,
                }
                for name, base_ns, cand_ns, delta, status in rows
            },
        }
        args.json.write_text(json.dumps(payload, indent=2) + "\n")


if __name__ == "__main__":
    main()
