# Slopjet Performance Rewrite — Rust/WASM

**Experimental, no AST parser in the rewrite runtime.** This branch uses
a single-pass, resumable lexer and a deferred token-span semantic rewriting
stage. It is not yet a complete ECMAScript implementation.

## Design

- The source is scanned once as chunks arrive; lexical state survives
  arbitrary chunk boundaries. Source spans are stored instead of AST nodes.
- All output is withheld until end-of-input, when token-level binding checks
  and ordered source-preserving edits are applied. Nothing speculative escapes.
- The WASM rewrite crate has no dependencies on a JavaScript parser crate.
  Both inline rewriting and network response rewriting use the same engine.
- Unrecognized or unsafe syntax produces an explicit error. The proxy must
  not serve the original JS instead of a protective rewrite.
- Native concurrency is across requests. Browser concurrency uses a bounded
  dedicated-Web-Worker pool hosted by the controller page, not by the service
  worker. Per-request scanner and WASM memory are isolated.

## Current implemented coverage

- Incremental tokens: identifiers, numbers, comments, quoted strings,
  regex literals in unambiguous positions, and nested template interpolations.
- Source-preserving rewrite of protected identifier references and static
  property names, including inside function bodies and normal expressions.
- Computed member access rewriting for dynamic keys; literal numeric and
  ordinary quoted keys remain untouched.
- Conservative detection of unsafe assignment targets, binding shadowing,
  dynamic imports, direct eval, and other unsupported features.
- Chunk partition invariance and no-output-on-error regression tests.

## Not yet covered / do not deploy as a general-purpose proxy

- Complete ECMAScript grammar and full scope analysis, including certain
  destructuring and advanced parameter forms.
- Dynamic module import/export URL rewriting, direct eval wrappers,
  assignment setters, postMessage receiver rewriting, class syntax and
  debugger transformations.
- Source maps, error capture transforms, and source instrumentation.
- Extensive real-world website compatibility and end-to-end latency testing.

Unsupported cases fail closed. This is why the branch remains a **draft PR**.

## Build

    cd packages/core/rewriter/wasm
    bash build.sh

The script builds the standalone Rust/WASM rewriter directly. No fallback
feature or alternate legacy parser is enabled.

## Tests

    cd packages/core/rewriter
    cargo test -p slopjet-perftest-scanner
    cargo check -p wasm --target wasm32-unknown-unknown
    cargo run --release -p slopjet-perftest-scanner --example throughput -- 10000
    cd ../../..
    node --test experiments/perftest/browser-pool.test.mjs experiments/perftest/wasm-worker.test.mjs
    node --test experiments/perftest/compiled-wasm.test.mjs

The last command requires a fresh WASM build. GitHub Actions runs these
steps automatically in .github/workflows/perftest.yml.

## Worker hosting

Serve wasm-worker.mjs, browser-pool.mjs, create-pool.mjs, the generated
wasm-bindgen JS module, and matching WASM binary on the controller origin.

    import { createPerftestPool } from '/perftest/create-pool.mjs';
    const pool = createPerftestPool({
      wasmGlueUrl: '/perftest/wasm.js',
      wasmUrl: '/scramjet/scramjet.wasm',
    });
    const controller = new Controller({
      serviceworker, transport, perftestWorkers: pool,
    });
    // On shutdown, call pool.close().

Worker counts default to a hardware-concurrency-based value and can be
overridden. Worker jobs use Transferable buffers to avoid unnecessary
request cloning and have a configurable 60-second default timeout. A hung
worker is terminated and replaced so other requests can continue.
Workers never split a source file into independent rewrite tasks.
