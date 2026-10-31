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
    'linux-x86': '#f43f5e',
    'linux-arm': '#34d399',
    'windows': '#fbbf24',
    'macos-x86': '#a78bfa',
    'macos-arm': '#60a5fa',
}

# Measured cross-protocol rows (toloo-protocol-bench, linux-x86 only).
# Each entry: (display name, criterion id, tooltip/operation text).
MEASURED = {
    'CANONICAL': [
        ('Matrix', 'protocol_canonical/matrix', 'Matrix event canonical JSON (ruma)'),
        ('Nostr', 'protocol_canonical/nostr', 'Nostr event JSON serialization'),
    ],
    'VERIFY': [
        ('Matrix', 'protocol_verify/matrix', 'Matrix event signature verify (ruma)'),
        ('Nostr', 'protocol_verify/nostr', 'Nostr event id + Schnorr verify'),
    ],
    'PARSE': [
        ('Matrix', 'protocol_parse/matrix', 'Matrix canonical JSON parse'),
        ('Signal', 'protocol_parse/signal', 'Signal protobuf message parse'),
        ('Nostr', 'protocol_parse/nostr', 'Nostr event JSON parse'),
    ],
    'BUILD': [
        ('Matrix', 'protocol_build/matrix', 'Matrix event sign (canonical JSON + ed25519)'),
        ('Nostr', 'protocol_build/nostr', 'Nostr event id + Schnorr sign'),
    ],
    'POOL': [],
    'CRYPTO': [
        ('Matrix', 'protocol_crypto/matrix_verify', 'Ed25519 verify (ed25519-dalek, Matrix stack)'),
    ],
}
MEASURED_COLORS = {
    'Matrix': '#a78bfa',
    'Signal': '#60a5fa',
    'Nostr': '#fb923c',
}
EST_TAG = ' <span class="proto-est">est</span>'

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

def log_scale(values):
    """Log-scale width function; keeps small ratios readable next to huge ones."""
    vals = [v for v in values if v > 0]
    if not vals:
        return lambda _ns: 0.0
    min_ns, max_ns = min(vals), max(vals)
    max_ratio = max_ns / min_ns if min_ns > 0 else 1
    log_max = math.log10(max_ratio + 1) if max_ratio > 1 else 1

    def pct(ns):
        if ns <= 0 or min_ns <= 0:
            return 0.0
        return math.log10(ns / min_ns + 1) / log_max * 100

    return pct

def ratio_parts(ref_ns, toloo_ns, estimate_name=None):
    """Return (ratio display, color, label, parity class) vs Toloo."""
    if toloo_ns <= 0 or ref_ns <= 0:
        return ('N/A', 'var(--text)', 'N/A', '')
    ratio = ref_ns / toloo_ns
    if ratio < 0.9:
        inv = 1 / ratio
        disp = f"{inv:.1f}" if inv < 10 else f"{round(inv):,}"
        return (disp, 'var(--success)', f"{disp}x faster", '')
    if ratio <= 1.1:
        return (f"{ratio:.1f}", 'var(--success)', 'parity', ' proto-parity')
    disp = f"{ratio:.1f}" if ratio < 10 else f"{round(ratio):,}"
    if ratio < 1.5:
        return (disp, 'var(--success)', f"{disp}x slower", '')
    if estimate_name is not None:
        color = 'var(--amber)' if estimate_name == 'IRC' else 'var(--red)'
    else:
        color = 'var(--amber)' if ratio < 10 else 'var(--red)'
    return (disp, color, f"{disp}x slower", '')

def proto_row_html(card_label, name, ns, pct, color, ratio_label, parity_class, op,
                   is_estimate=False):
    """A reference row; `is_estimate` rows carry the compact `est` tag."""
    ns_str = f"{ns:,.0f}"
    tag = EST_TAG if is_estimate else ''
    tooltip = f"{ns_str} ns · {ratio_label} · {op}"
    return (
        f'            <div class="proto-row ref">\n'
        f'              <span class="proto-label">{name}</span>\n'
        f'              <div class="proto-bar" role="img" aria-label="{card_label}, {name}: {ns_str} ns">\n'
        f'                <div class="proto-fill" style="width:{pct:.1f}%;background:{color}"></div>\n'
        f'              </div>\n'
        f'              <span class="proto-val{parity_class}" tabindex="0" aria-label="{name}: {ns_str} ns, {ratio_label}, {op}"><span class="proto-inline-ns">{ns_str} ns</span> <span class="proto-sub">{ratio_label}</span>{tag}<span class="proto-tooltip" aria-hidden="true">{tooltip}</span></span>\n'
        f'            </div>'
    )

def compute_proto_vars(data):
    """Compute all protocol comparison variables.

    Bar widths use log scale: log10(ratio+1)/log10(max_ratio+1)*100.
    This keeps Toloo visible (~10%) while Matrix reaches 100%,
    unlike linear scale where extreme ratios (<0.1%) make Toloo invisible.
    """
    vars = {}
    p = data.get('platforms', {}).get('linux-x86', {})
    protocols = data.get('platforms', {}).get('protocols', {})

    PREFIX_MAP = {
        'canonical/encode/medium': 'CANONICAL',
        'envelope_verify/verify_chain/depth1': 'VERIFY',
        'parse_envelope/parse_envelope_bytes': 'PARSE',
        'event_build/make_room_message': 'BUILD',
        'pool_query/get_room_channel_1k': 'POOL',
        'crypto/ed25519_verify': 'CRYPTO',
    }

    for key, card_label in CATEGORIES:
        prefix = PREFIX_MAP[key]
        toloo_ns = p.get(key, {}).get('mean_ns', 0)
        refs = REF_PROTOCOLS.get(key, {})

        measured = []
        for name, bench, op in MEASURED[prefix]:
            ns = protocols.get(bench, {}).get('mean_ns', 0)
            if ns > 0:
                measured.append((name, ns, op))
        measured_names = {name for name, _, _ in measured}

        estimate_names = ['IRC', 'XMPP']
        if 'Matrix' not in measured_names:
            estimate_names.append('MATRIX')

        log_pct = log_scale(
            [toloo_ns] + [refs.get(n, 0) for n in estimate_names] + [ns for _, ns, _ in measured]
        )

        vars[f'PROTO_{prefix}_TOLOO_NS'] = f"{toloo_ns:,.0f}"
        vars[f'PROTO_{prefix}_TOLOO_PCT'] = f"{log_pct(toloo_ns):.1f}%"
        vars[f'PROTO_{prefix}_IRC_TAG'] = EST_TAG
        vars[f'PROTO_{prefix}_XMPP_TAG'] = EST_TAG

        for ref_name in ['IRC', 'XMPP']:
            ref_ns = refs.get(ref_name, 0)
            _, color, label, parity_class = ratio_parts(ref_ns, toloo_ns, ref_name)
            vars[f'PROTO_{prefix}_{ref_name}_NS'] = f"{ref_ns:,.0f}" if ref_ns > 0 else "N/A"
            vars[f'PROTO_{prefix}_{ref_name}_PCT'] = f"{log_pct(ref_ns):.1f}%"
            vars[f'PROTO_{prefix}_{ref_name}_COLOR'] = color
            vars[f'PROTO_{prefix}_{ref_name}_LABEL'] = label
            vars[f'PROTO_{prefix}_{ref_name}_PARITY_CLASS'] = parity_class

        rows = []
        for name, ns, op in measured:
            _, _, label, parity_class = ratio_parts(ns, toloo_ns)
            rows.append(proto_row_html(
                card_label, name, ns, log_pct(ns), MEASURED_COLORS.get(name, 'var(--text)'),
                label, parity_class, op,
            ))

        if not measured and 'MATRIX' in refs:
            ref_ns = refs['MATRIX']
            _, _, label, parity_class = ratio_parts(ref_ns, toloo_ns, 'MATRIX')
            rows.append(proto_row_html(
                card_label, 'Matrix', ref_ns, log_pct(ref_ns), 'var(--red)',
                label, parity_class, 'reference estimate', is_estimate=True,
            ))

        vars[f'PROTO_{prefix}_MEASURED_ROWS'] = '\n'.join(rows)

    return vars

PRIVATE_ROWS = {
    'Encrypt': [
        ('Toloo', 'protocol_private/toloo_encrypt', 'X25519 + ChaCha20-Poly1305 (stateless)', True),
        ('Signal', 'protocol_private/signal_encrypt', 'Double Ratchet encrypt', False),
        ('Matrix', 'protocol_private/olm_encrypt', 'Olm encrypt (vodozemac)', False),
        ('Nostr', 'protocol_private/nostr_encrypt', 'NIP-44 v2 encrypt', False),
    ],
    'Decrypt': [
        ('Toloo', 'protocol_private/toloo_decrypt', 'X25519 + ChaCha20-Poly1305 (stateless)', True),
        ('Signal', 'protocol_private/signal_decrypt', 'Double Ratchet decrypt', False),
        ('Matrix', 'protocol_private/olm_decrypt', 'Olm decrypt (vodozemac)', False),
        ('Nostr', 'protocol_private/nostr_decrypt', 'NIP-44 v2 decrypt', False),
    ],
}

def compute_private_card(data):
    """The 1:1 E2E card; only rendered when cross-protocol data exists."""
    p = data.get('platforms', {}).get('protocols', {})
    baseline = {
        'Encrypt': p.get('protocol_private/toloo_encrypt', {}).get('mean_ns', 0),
        'Decrypt': p.get('protocol_private/toloo_decrypt', {}).get('mean_ns', 0),
    }
    if baseline['Encrypt'] <= 0 or baseline['Decrypt'] <= 0:
        return ''

    sections = []
    for group, specs in PRIVATE_ROWS.items():
        ns_values = [p.get(bench, {}).get('mean_ns', 0) for _, bench, _, _ in specs]
        log_pct = log_scale(ns_values)
        rows = [f'            <div class="proto-subhead" role="presentation">{group}</div>']
        for name, bench, op, is_toloo in specs:
            ns = p.get(bench, {}).get('mean_ns', 0)
            if ns <= 0:
                continue
            ns_str = f"{ns:,.0f}"
            pct = log_pct(ns)
            card = f'1:1 {group.lower()}'
            if is_toloo:
                rows.append(
                    f'            <div class="proto-row toloo">\n'
                    f'              <span class="proto-label">{name}</span>\n'
                    f'              <div class="proto-bar" role="img" aria-label="{card}, {name}: {ns_str} ns">\n'
                    f'                <div class="proto-fill toloo" style="width:{pct:.1f}%"></div>\n'
                    f'              </div>\n'
                    f'              <span class="proto-val">{ns_str} ns</span>\n'
                    f'            </div>'
                )
            else:
                _, _, label, parity_class = ratio_parts(ns, baseline[group])
                rows.append(proto_row_html(
                    card, name, ns, pct, MEASURED_COLORS[name], label, parity_class, op,
                ))
        sections.append('\n'.join(rows))

    notes = ['Steady-state encryption (setup excluded)']
    signal_setup = p.get('protocol_private/signal_setup', {}).get('mean_ns', 0)
    olm_setup = p.get('protocol_private/olm_setup', {}).get('mean_ns', 0)
    if signal_setup > 0:
        notes.append(f'Signal X3DH setup {fmt_ns(signal_setup)}')
    if olm_setup > 0:
        notes.append(f'Olm session setup {fmt_ns(olm_setup)}')
    notes.append('Toloo private messages are stateless and need no session.')

    return (
        '        <article class="bench-card" aria-labelledby="cat-private">\n'
        '          <div class="bench-top">\n'
        '            <h3 id="cat-private">Private Message (1:1)</h3>\n'
        '          </div>\n'
        '          <div class="proto-rows">\n'
        + '\n'.join(sections) + '\n'
        '          </div>\n'
        f'          <p class="proto-note">{" · ".join(notes)}</p>\n'
        '        </article>'
    )

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
        rows.append(f'      <div class="key-row">')
        rows.append(f'        <span class="key-label">{label}</span>')
        rows.append(f'        <span class="key-value {color_class}">{fmt}</span>')
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
    parse_ns = p.get('parse_envelope/parse_envelope_bytes', {}).get('mean_ns', 0)
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

    # Cross-protocol private card
    vars['PRIVATE_CARD'] = compute_private_card(data)

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