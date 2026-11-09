#!/usr/bin/env bash
set -euo pipefail
shopt -s inherit_errexit

# The experiment builds the standalone Rust/WASM engine only.
# No legacy parser feature or baseline bypass exists.
FEATURES="perftest"

if ! [ "${RELEASE:-0}" = "1" ]; then
	WASMOPTFLAGS="${WASMOPTFLAGS:-} -g"
	FEATURES="debug,${FEATURES:-}"
else
	: "${WASMOPTFLAGS:=}"
	: "${FEATURES:=}"
fi

MODE="release"
if [ "${RELEASE:-0}" != "1" ]; then MODE="debug"; fi
# shellcheck disable=SC2046
SRC_HASH=$( (echo "MODE=${MODE}"; echo "FEATURES=${FEATURES}"; sha256sum $(find ../ -type f -not -path "*/\.*" -and \( -name "*.rs" -o -name "*.toml" -o -name "*.sh" -o -name "*.json" -o -name "*.md" \); echo Cargo.toml; echo build.sh) 2>/dev/null | sort -k2 | sha256sum ) | sha256sum | cut -d' ' -f1 ) || SRC_HASH="unknown"

if [ -f out/.build-hash ] && [ -f ../../dist/scramjet.wasm ] && [ "$SRC_HASH" != "unknown" ] && grep -q "$SRC_HASH" out/.build-hash; then
  echo "Rewriter sources unchanged (hash $SRC_HASH); skipping rebuild."
  exit 0
fi

which cargo wasm-bindgen wasm-opt wasm-snip &> /dev/null || {
	echo "Please install cargo, wasm-bindgen, wasm-opt from https://github.com/WebAssembly/binaryen, and wasm-snip from https://github.com/r58playz/wasm-snip!"
	exit 1
}

WBG="wasm-bindgen 0.2.105"
if ! [[ "$(wasm-bindgen -V)" =~ ^"$WBG" ]]; then
	echo "Incorrect wasm-bindgen-cli version: '$(wasm-bindgen -V)' != '$WBG'"
	exit 1
fi

(
	export RUSTFLAGS='-Zlocation-detail=none -Zfmt-debug=none'
	if [ "${OPTIMIZE_FOR_SIZE:-0}" = "1" ]; then
		export RUSTFLAGS="${RUSTFLAGS} -C opt-level=z"
	fi
	STD_FEATURES=""
	if [ "${OPTIMIZE_FOR_SPEED:-0}" = "0" ]; then
		STD_FEATURES="${STD_FEATURES},optimize_for_size"
	fi
	cargo +nightly build --release --target wasm32-unknown-unknown \
		-Z build-std=panic_abort,std -Z build-std-features=${STD_FEATURES} \
		--no-default-features --features "$FEATURES"
)
wasm-bindgen --target web --out-dir out/ ../target/wasm32-unknown-unknown/release/wasm.wasm

if [[ "$OSTYPE" == "darwin"* ]] || [[ "$OSTYPE" == "freebsd"* ]] || [[ "$OSTYPE" == "dragonfly"* ]]; then
	sed -i '' 's/import.meta.url/""/g' out/wasm.js
else
	sed -i 's/import.meta.url/""/g' out/wasm.js
fi

cd ../../

# No parser-specific symbol pruning: preserve the standalone engine unchanged.
cp rewriter/wasm/out/wasm_bg.wasm rewriter/wasm/out/wasm_snipped.wasm


if [ "${RELEASE:-0}" = "1" ]; then
	(
		G="--generate-global-effects"
		# shellcheck disable=SC2086
		time wasm-opt $WASMOPTFLAGS \
			rewriter/wasm/out/wasm_snipped.wasm -o rewriter/wasm/out/optimized.wasm \
			--converge -tnh --vacuum \
			$G -O4 $G --flatten $G --rereloop $G -O4 $G -O4 $G -O4 \
			$G -Oz $G --flatten $G --rereloop $G -Oz $G -Oz $G -Oz \
			$G --code-folding $G --const-hoisting $G --dae $G --flatten $G --merge-locals \
			$G -O4 $G --flatten $G --rereloop $G -O4 $G -O4 $G -O4 \
			$G -Oz $G --flatten $G --rereloop $G -Oz $G -Oz $G -Oz
	)
else
	cp rewriter/wasm/out/wasm_snipped.wasm rewriter/wasm/out/optimized.wasm
fi

mkdir -p dist/

cp rewriter/wasm/out/optimized.wasm dist/scramjet.wasm
echo "$SRC_HASH" > rewriter/wasm/out/.build-hash || true
echo "Rewriter Build Complete!"
