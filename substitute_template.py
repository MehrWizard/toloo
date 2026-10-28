#!/usr/bin/env python3
"""Substitute template variables with benchmark data to produce static HTML."""

import json
import math
import sys
from pathlib import Path
from datetime import datetime, timezone

# Reference protocol latencies (ns) - fixed estimates
REF_PROTOCOLS = {
    'canonical/encode/medium': {'IRC': 150000, 'XMPP': 800000, 'MATRIX': 2500000},
    'envelope_verify/verify_chain/depth1': {'IRC': 50000, 'XMPP': 300000, 'MATRIX': 1200000},
    'parse_envelope/parse_envelope_bytes': {'IRC': 20000, 'XMPP': 150000, 'MATRIX': 600000},
    'event_build/make_room_message': {'IRC': 100000, 'XMPP': 500000, 'MATRIX': 2000000},
    'pool_query/get_room_channel_1k': {'IRC': 500000, 'XMPP': 2000000, 'MATRIX': 8000000},
    'crypto/ed25519_verify': {'IRC': 45000, 'XMPP': 200000, 'MATRIX': 800000},
}

CATEGORIES = [
    ('canonical/encode/medium', 'Canonical Encode'),
    ('envelope_verify/verify_chain/depth1', 'Envelope Verify'),
    ('parse_envelope/parse_envelope_bytes', 'Parse Envelope'),
    ('event_build/make_room_message', 'Event Build'),
    ('pool_query/get_room_channel_1k', 'Pool Query (1k)'),
    ('crypto/ed25519_verify', 'Crypto (ed25519)'),
]

PLATFORMS = ['linux-x86', 'linux-arm', 'windows', 'macos-x86', 'macos-arm']
PLAT_COLORS = {
    'linux-x86': '#4a9cf0',
    'linux-arm': '#3dba6e',
    'windows': '#e0a020',
    'macos-x86': '#e05050',
    'macos-arm': '#8b6fd0',
}

def fmt_ns(ns):
    if ns >= 1_000_000:
        return f"{ns/1_000_000:.2f} ms"
    elif ns >= 1_000:
        return f"{ns/1_000:.1f} µs"
    return f"{ns:.0f} ns"

def load_data(json_path):
    with open(json_path) as f:
        return json.load(f)

def get_mean(data, platform, key):
    return data.get('platforms', {}).get(platform, {}).get(key, {}).get('mean_ns', 0)

def compute_proto_vars(data):
    """Compute all protocol comparison variables.

    Bar widths use log scale: log10(ratio+1)/log10(max_ratio+1)*100.
    This keeps Toloo visible (~10%) while Matrix reaches 100%,
    unlike linear scale where extreme ratios (<0.1%) make Toloo invisible.
    """
    vars = {}
    p = data.get('platforms', {}).get('linux-x86', {})

    PREFIX_MAP = {
        'canonical/encode/medium': 'CANONICAL',
        'envelope_verify/verify_chain/depth1': 'VERIFY',
        'parse_envelope/parse_envelope_bytes': 'PARSE',
        'event_build/make_room_message': 'BUILD',
        'pool_query/get_room_channel_1k': 'POOL',
        'crypto/ed25519_verify': 'CRYPTO',
    }

    for key, label in CATEGORIES:
        prefix = PREFIX_MAP[key]
        toloo_ns = p.get(key, {}).get('mean_ns', 0)
        refs = REF_PROTOCOLS.get(key, {})

        all_ns = [toloo_ns] + [refs.get(r, 0) for r in ['IRC', 'XMPP', 'MATRIX']]
        min_ns = min(v for v in all_ns if v > 0)
        max_ns = max(all_ns)
        max_ratio = max_ns / min_ns if min_ns > 0 else 1
        log_max = math.log10(max_ratio + 1) if max_ratio > 1 else 1

        def log_pct(ns):
            if ns <= 0 or min_ns <= 0:
                return 0
            ratio = ns / min_ns
            log_ratio = math.log10(ratio + 1)
            return (log_ratio / log_max * 100)

        vars[f'PROTO_{prefix}_TOLOO_NS'] = f"{toloo_ns:,.0f}"
        vars[f'PROTO_{prefix}_TOLOO_PCT'] = f"{log_pct(toloo_ns):.1f}%"

        for ref_name in ['IRC', 'XMPP', 'MATRIX']:
            ref_ns = refs.get(ref_name, 0)
            if ref_ns > 0:
                vars[f'PROTO_{prefix}_{ref_name}_PCT'] = f"{log_pct(ref_ns):.1f}%"
                vars[f'PROTO_{prefix}_{ref_name}_RATIO'] = f"{round(ref_ns / toloo_ns):,}" if toloo_ns > 0 else "N/A"
            else:
                vars[f'PROTO_{prefix}_{ref_name}_PCT'] = "0%"
                vars[f'PROTO_{prefix}_{ref_name}_RATIO'] = "N/A"

    return vars

def compute_platform_chart(data):
    """Generate HTML rows for cross-platform chart."""
    rows = []

    for key, label in CATEGORIES:
        # Get values for all platforms
        platform_data = {}
        for plat in PLATFORMS:
            ns = get_mean(data, plat, key)
            if ns > 0:
                platform_data[plat] = ns

        if not platform_data:
            continue

        max_ns = max(platform_data.values())

        rows.append(f'          <div class="chart-group">')
        rows.append(f'            <div class="chart-group-label">{label}</div>')

        for plat in PLATFORMS:
            ns = platform_data.get(plat, 0)
            if ns == 0:
                continue
            pct = (ns / max_ns * 100) if max_ns > 0 else 0
            color = PLAT_COLORS.get(plat, '#888')
            rows.append(f'            <div class="chart-platform-row">')
            rows.append(f'              <span class="chart-platform-label">{plat}</span>')
            rows.append(f'              <div class="chart-platform-bar">')
            rows.append(f'                <div class="chart-platform-fill" style="width:{pct:.1f}%;background:{color}"></div>')
            rows.append(f'              </div>')
            rows.append(f'              <span class="chart-platform-val">{fmt_ns(ns)}</span>')
            rows.append(f'            </div>')

        rows.append(f'          </div>')

    return '\n'.join(rows)

def compute_key_results(data):
    """Generate key results rows."""
    p = data.get('platforms', {}).get('linux-x86', {})
    rows = []

    for key, label in CATEGORIES:
        ns = p.get(key, {}).get('mean_ns', 0)
        fmt = fmt_ns(ns)
        color_class = 'text-green' if ns < 50000 else ('text-amber' if ns < 100000 else 'text-red')
        rows.append(f'      <div style="display:flex;justify-content:space-between;padding:.3rem 0;border-bottom:1px solid var(--line);font-size:.75rem;align-items:center">')
        rows.append(f'        <span style="color:var(--text)">{label}</span>')
        rows.append(f'        <span style="font-family:var(--mono);color:var(--{color_class.replace("text-", "")});font-weight:600">{fmt}</span>')
        rows.append(f'      </div>')

    return '\n'.join(rows)

def compute_simulation(data):
    """Compute simulation values from linux-x86 data."""
    p = data.get('platforms', {}).get('linux-x86', {})

    # Background load (modeled)
    bg_load = 10 * 1000 * 0.02 + 1000 * 0.1  # 300 msg/s

    # Single-thread critical path
    encode_ns = p.get('canonical/encode/medium', {}).get('mean_ns', 0)
    verify_ns = p.get('envelope_verify/verify_chain/depth1', {}).get('mean_ns', 0)
    parse_ns = p.get('envelope_parse/parse_envelope_bytes', {}).get('mean_ns', 0)
    build_ns = p.get('event_build/make_room_message', {}).get('mean_ns', 0)
    pool_ns = p.get('pool_query/get_room_channel_1k', {}).get('mean_ns', 0)
    crypto_ns = p.get('crypto/ed25519_verify', {}).get('mean_ns', 0)

    total_ns = encode_ns + verify_ns + parse_ns + build_ns + pool_ns + crypto_ns
    single_thread = round(1_000_000_000 / total_ns) if total_ns > 0 else 0

    steady = round(single_thread / bg_load, 1) if bg_load > 0 else 0
    burst = round(single_thread / (bg_load * 50), 2) if bg_load > 0 else 0

    return {
        'SIM_BG_LOAD': f"{int(bg_load):,}",
        'SIM_SINGLE_THREAD': f"{single_thread:,}",
        'SIM_STEADY': f"{steady:.1f}",
        'SIM_BURST': f"{burst:.2f}",
    }

def compute_capacity(data):
    """Compute capacity planning from single-thread baseline."""
    sim = compute_simulation(data)
    single = int(sim['SIM_SINGLE_THREAD'].replace(',', ''))

    # Measured scaling: 1 thread (ubuntu-slim), 4 threads (ubuntu-24.04)
    # Estimated for 16/64
    configs = [
        ('1', single, 1.0),      # 1 thread measured
        ('4', single, 3.2),      # 4 threads measured (contention)
        ('16', single, 8.5),     # 16 threads estimated (sharded)
        ('64', single, 18.0),    # 64 threads estimated (NUMA)
    ]

    vars = {}
    for id, base, scale in configs:
        throughput = round(base * scale)
        users = throughput * 10  # at 0.1 msg/s
        vars[f'CAP_{id}_THREAD'] = f"{throughput:,}"
        vars[f'CAP_{id}_USERS'] = f"{users:,}"

    return vars

def main():
    if len(sys.argv) < 3:
        print("Usage: substitute_template.py <template> <benchmark.json> [output.html]")
        sys.exit(1)

    template_path = Path(sys.argv[1])
    data_path = Path(sys.argv[2])
    output_path = Path(sys.argv[3]) if len(sys.argv) > 3 else Path('index.html')

    # Load data
    data = load_data(data_path)

    # Read template
    template = template_path.read_text()

    # Compute all variables
    vars = {}

    # Metadata
    vars['COMMIT_SHORT'] = data.get('commit', '')[:8]
    vars['BRANCH'] = data.get('branch', 'main')
    vars['BUILD_DATE'] = datetime.now(timezone.utc).strftime('%Y-%m-%d %H:%M UTC')

    # Runner info
    runner_parts = []
    for plat in PLATFORMS:
        if plat in data.get('platforms', {}):
            runner_parts.append(plat)
    vars['RUNNER_INFO'] = ', '.join(runner_parts) if runner_parts else 'unknown'

    # Protocol comparison
    vars.update(compute_proto_vars(data))

    # Platform chart
    vars['PLATFORM_CHART_ROWS'] = compute_platform_chart(data)

    # Simulation
    vars.update(compute_simulation(data))

    # Capacity
    vars.update(compute_capacity(data))

    # Key results
    vars['KEY_RESULTS_ROWS'] = compute_key_results(data)

    # Substitute all variables
    for key, value in vars.items():
        placeholder = f'%{{{key}}}%'
        template = template.replace(placeholder, str(value))

    # Write output
    output_path.write_text(template)
    print(f"Generated {output_path}")

if __name__ == '__main__':
    main()