#!/usr/bin/env bash

# this script's location (rust_petri_net_analysis project root)
DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"

if [ -z "$1" ]; then
	echo "No detecting directory is provided"
	echo "Usage: ./detect.sh DIRNAME"
	exit 1
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
export PN_LOG=info

# Analysis mode and options (passed to pn via PN_FLAGS; -p is set per crate below)
# To only detect deadlock
# export PN_FLAGS_BASE="-m deadlock"
# To only detect datarace
# export PN_FLAGS_BASE="-m datarace"
# To only detect atomicity violation (requires atomic-violation feature at build time)
# export PN_FLAGS_BASE="-m atomic"
# To analyze only specific crates
# export PN_FLAGS_BASE="-m all --crate-whitelist inter,intra"
# To skip specific crates
# export PN_FLAGS_BASE="-m all --crate-blacklist inter,intra"
PN_FLAGS_BASE="${PN_FLAGS_BASE:--m all --pn-analysis-dir=${DIR}/tmp}"

# Find all Cargo.tomls recursively under the detecting directory
# and record them in cargo_dir.txt
cargo_dir_file=$(realpath "$DIR/cargo_dir.txt")
rm -f "$cargo_dir_file"
touch "$cargo_dir_file"

pushd "$1" > /dev/null
cargo clean
cargo_tomls=$(find . -name "Cargo.toml")
for cargo_toml in ${cargo_tomls[@]}
do
	echo $(dirname "$cargo_toml") >> "$cargo_dir_file"
done

IFS=$'\n' read -d '' -r -a lines < "$cargo_dir_file"
for cargo_dir in ${lines[@]}
do
	crate_name=$(basename "$cargo_dir" | tr '-' '_')
	export PN_FLAGS="${PN_FLAGS_BASE} -p ${crate_name}"
	pushd "$cargo_dir" > /dev/null
	cargo build
	popd > /dev/null
done
popd > /dev/null

rm -f "$cargo_dir_file"
