#!/usr/bin/env bash

set -euo pipefail

DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BENCH_DIR="${DIR}/bench"
OUTPUT_ROOT="${DIR}/tmp"

usage() {
	cat <<EOF
Usage: ./run-all-benches.sh [cargo-build-args...]

Build and analyze every benchmark crate grouped by bench mode directories:
  bench/deadlock/
  bench/data-race/
  bench/atomic-violation/

Artifacts are written to:
  ${OUTPUT_ROOT}/<crate_name>/

Default pn export flags per crate:
  --viz-callgraph --viz-petrinet --viz-stategraph
  --viz-pointsto --viz-cir --report-level=research

Typical outputs include summary/report files plus any supported exports
emitted for that crate and mode.

Behavior:
  - deadlock benches         -> -m deadlock
  - data-race benches        -> -m datarace
  - atomic-violation benches -> -m atomic
  - installs pn with the atomic-violation feature enabled
  - cleans each crate before building so analysis always reruns

Examples:
  ./run-all-benches.sh
  ./run-all-benches.sh --release

Environment:
  PN_LOG                log level for pn (default: info)
  BENCH_OUTPUT_ROOT     override output root (default: ${OUTPUT_ROOT})
  PN_EXTRA_FLAGS        append extra pn flags after the defaults
EOF
}

if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
	usage
	exit 0
fi

if [[ ! -d "$BENCH_DIR" ]]; then
	echo "bench directory not found: $BENCH_DIR" >&2
	exit 1
fi

OUTPUT_ROOT="${BENCH_OUTPUT_ROOT:-$OUTPUT_ROOT}"
mkdir -p "$OUTPUT_ROOT"

pushd "$DIR" > /dev/null
cargo install --path . --bin pn --features atomic-violation --force
popd > /dev/null

PN_BIN="$(command -v pn || true)"
if [[ -z "$PN_BIN" ]]; then
	PN_BIN="${CARGO_HOME:-$HOME/.cargo}/bin/pn"
fi

export RUSTC_WRAPPER="$PN_BIN"
RUSTC_SYSROOT="$(rustc --print sysroot)"
export LD_LIBRARY_PATH="${RUSTC_SYSROOT}/lib${LD_LIBRARY_PATH:+:${LD_LIBRARY_PATH}}"
export RUST_BACKTRACE=full
export PN_LOG="${PN_LOG:-info}"

cargo_build_args=("$@")
default_pn_flags=(
	--pn-analysis-dir="$OUTPUT_ROOT"
	--viz-callgraph
	--viz-petrinet
	--viz-stategraph
	--viz-pointsto
	--viz-cir
	--report-level=research
)
extra_pn_flags=()
if [[ -n "${PN_EXTRA_FLAGS:-}" ]]; then
	read -r -a extra_pn_flags <<< "${PN_EXTRA_FLAGS}"
fi

map_mode() {
	case "$1" in
		deadlock)
			echo "deadlock"
			;;
		data-race)
			echo "datarace"
			;;
		atomic-violation)
			echo "atomic"
			;;
		*)
			echo "unsupported bench mode directory: $1" >&2
			return 1
			;;
	esac
}

run_mode_dir() {
	local mode_dir="$1"
	local mode_name mode_flag
	local -a manifests pn_flags

	mode_name="$(basename "$mode_dir")"
	mode_flag="$(map_mode "$mode_name")"

	mapfile -t manifests < <(find "$mode_dir" -mindepth 2 -maxdepth 2 -name Cargo.toml | sort)
	if [[ ${#manifests[@]} -eq 0 ]]; then
		echo "==> [$mode_flag] no benchmark crates found under $mode_dir"
		return 0
	fi

	echo "==> [$mode_flag] scanning $mode_dir"
	for manifest in "${manifests[@]}"; do
		local crate_dir crate_name analysis_dir
		crate_dir="$(dirname "$manifest")"
		crate_name="$(basename "$crate_dir" | tr '-' '_')"
		analysis_dir="${OUTPUT_ROOT}/${crate_name}"

		echo "    -> ${crate_name}"
		rm -rf "$analysis_dir"
		cargo clean --manifest-path "$manifest"

		pn_flags=(
			-m "$mode_flag"
			-p "$crate_name"
			"${default_pn_flags[@]}"
			"${extra_pn_flags[@]}"
		)
		export PN_FLAGS="${pn_flags[*]}"
		cargo build --manifest-path "$manifest" "${cargo_build_args[@]}"
	done
}

mapfile -t mode_dirs < <(find "$BENCH_DIR" -mindepth 1 -maxdepth 1 -type d | sort)
if [[ ${#mode_dirs[@]} -eq 0 ]]; then
	echo "no bench mode directories found under $BENCH_DIR" >&2
	exit 1
fi

for mode_dir in "${mode_dirs[@]}"; do
	run_mode_dir "$mode_dir"
done
