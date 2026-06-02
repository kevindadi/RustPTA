#!/usr/bin/env bash

# this script's location (rust_petri_net_analysis project root)
DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"

usage() {
	cat <<EOF
Usage: ./detect.sh <detect-dir> [pn-options...]

Build pn and run analysis on every crate under <detect-dir>.
Options after <detect-dir> are forwarded to pn via PN_FLAGS (-p is set per crate).

Examples:
  ./detect.sh bench/deadlock/call-no-deadlock/
  ./detect.sh bench/deadlock/call-no-deadlock/ -m datarace --pn-analysis-dir=tmp
  ./detect.sh bench/deadlock/ -m deadlock --viz-petrinet --pn-analysis-dir=tmp/out

Default pn flags (when none given):
  -m deadlock --pn-analysis-dir=${DIR}/tmp

Environment:
  PN_LOG          log level for pn (default: info)
  PN_FLAGS_BASE   deprecated; pass flags as script arguments instead
EOF
}

if [[ $# -lt 1 || "$1" == "-h" || "$1" == "--help" ]]; then
	usage
	exit "$([[ $# -lt 1 ]] && echo 1 || echo 0)"
fi

DETECT_DIR="$1"
shift

# Remaining args → pn flags; default when omitted
if [[ $# -gt 0 ]]; then
	PN_ARGS=("$@")
elif [[ -n "${PN_FLAGS_BASE:-}" ]]; then
	# shellcheck disable=SC2206
	PN_ARGS=(${PN_FLAGS_BASE})
else
	PN_ARGS=("-m" "deadlock" "--pn-analysis-dir=${DIR}/tmp")
fi

# Build pn (rustc driver wrapper)
pushd "$DIR" > /dev/null
# For development use debug build
cargo build --bin pn
# For usage use release
# cargo build --release --bin pn
# Enable atomicity-violation detection:
# cargo build --features atomic-violation --bin pn
popd > /dev/null

# For development of pn use debug
export RUSTC_WRAPPER=${DIR}/target/debug/pn
# For usage use release
# export RUSTC_WRAPPER=${DIR}/target/release/pn

export RUST_BACKTRACE=full
export PN_LOG="${PN_LOG:-info}"

# Find all Cargo.tomls recursively under the detecting directory
# and record them in cargo_dir.txt
cargo_dir_file=$(realpath "$DIR/cargo_dir.txt")
rm -f "$cargo_dir_file"
touch "$cargo_dir_file"

pushd "$DETECT_DIR" > /dev/null
cargo clean
cargo_tomls=$(find . -name "Cargo.toml")
for cargo_toml in ${cargo_tomls[@]}
do
	echo "$(cd "$(dirname "$cargo_toml")" && pwd)" >> "$cargo_dir_file"
done

IFS=$'\n' read -d '' -r -a lines < "$cargo_dir_file"
for cargo_dir in ${lines[@]}
do
	crate_name=$(basename "$cargo_dir" | tr '-' '_')
	export PN_FLAGS="${PN_ARGS[*]} -p ${crate_name}"
	pushd "$cargo_dir" > /dev/null
	cargo build
	popd > /dev/null
done
popd > /dev/null

rm -f "$cargo_dir_file"
