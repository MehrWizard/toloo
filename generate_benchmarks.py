#!/usr/bin/env python3
"""Generate latest.json from criterion benchmark results for the dashboard."""

import json
from pathlib import Path

CRITERION_DIR = Path("/home/mahdi/Code/irandawn/toloo/target/criterion")

# Mapping from criterion benchmark names to dashboard keys
BENCHMARK_KEYS = {
    "canonical/encode/small": "canonical/encode/small",
    "canonical/encode/medium": "canonical/encode/medium",
    "canonical/encode/large": "canonical/encode/large",
    "envelope_verify/verify_chain/depth1": "envelope_verify/verify_chain/depth1",
    "envelope_verify/verify_chain/depth2": "envelope_verify/verify_chain/depth2",
    "envelope_parse/parse_envelope_from_value": "parse_envelope/parse_envelope_from_value",
    "envelope_parse/parse_envelope_bytes": "parse_envelope/parse_envelope_bytes",
    "envelope_serialize/to_envelope_json_old": "envelope_serialize/to_envelope_json_old",
    "envelope_serialize/envelope_to_json_bytes": "envelope_serialize/envelope_to_json_bytes",
    "event_build/make_room_create": "event_build/make_room_create",
    "event_build/make_room_message": "event_build/make_room_message",
    "event_build/make_node_meta": "event_build/make_node_meta",
    "pool_query/get_room_channel_1k": "pool_query/get_room_channel_1k",
    "pool_query/get_wanted_after_0": "pool_query/get_wanted_after_0",
    "pool_query/get_wanted_after_tc": "pool_query/get_wanted_after_tc",
    "pool_query/get_by_id": "pool_query/get_by_id",
    "pool_write/put_single": "pool_write/put_single",
    "pool_write/put_batch_100": "pool_write/put_batch_100",
    "pool_write/put_batch_10": "pool_write/put_batch_10",
    "crypto/ed25519_generate": "crypto/ed25519_generate",
    "crypto/ed25519_sign": "crypto/ed25519_sign",
    "crypto/ed25519_verify": "crypto/ed25519_verify",
    "crypto/ed25519_verify_batch_10": "crypto/ed25519_verify_batch_10",
    "crypto/x25519_generate": "crypto/x25519_generate",
    "crypto/x25519_shared_secret": "crypto/x25519_shared_secret",
    "crypto/chacha20_encrypt": "crypto/chacha20_encrypt",
    "crypto/chacha20_decrypt": "crypto/chacha20_decrypt",
}

def read_estimate(bench_dir):
    """Read mean_ns from criterion estimates.json"""
    estimates_path = bench_dir / "new" / "estimates.json"
    if not estimates_path.exists():
        return None
    try:
        with open(estimates_path) as f:
            data = json.load(f)
        # Return mean point estimate in nanoseconds
        return data["mean"]["point_estimate"]
    except (json.JSONDecodeError, KeyError):
        return None

def collect_benchmarks():
    """Collect all benchmark results from criterion directory."""
    platforms = {}

    # We only have one platform (current machine) for now
    platform_name = "linux-x86"  # This should be detected properly

    platform_data = {}

    for bench_name, key in BENCHMARK_KEYS.items():
        bench_dir = CRITERION_DIR / bench_name
        if bench_dir.exists():
            mean_ns = read_estimate(bench_dir)
            if mean_ns is not None:
                platform_data[key] = {"mean_ns": mean_ns}

    platforms[platform_name] = platform_data
    return {"platforms": platforms}

def main():
    data = collect_benchmarks()

    # Write to results/latest.json
    output_dir = Path("/home/mahdi/Code/irandawn/toloo/results")
    output_dir.mkdir(exist_ok=True)

    output_file = output_dir / "latest.json"
    with open(output_file, "w") as f:
        json.dump(data, f, indent=2)

    print(f"Generated {output_file}")
    print(f"Collected {len(data['platforms']['linux-x86'])} benchmarks")

if __name__ == "__main__":
    main()