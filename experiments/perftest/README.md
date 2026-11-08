# Slopjet `perftest`: Rust/WASM deferred scanner and request-level concurrency

**Status: working feature-gated experiment with OXC fallback; NOT a full no-AST
replacement of Scramjet's JavaScript grammar and transformations.**

## Design

1. Rust/WASM is the execution engine. StreamingProgram accepts chunks in
   one forward pass and retains state across chunk boundaries.
2. All output is deferred until EOF. A supported whole-file subset emits
   source-preserving rewrites; unsupported syntax gets the original OXC
   parser/visitor. No irreversible speculative output is permitted.
3. Safe subset: inert string/comment statements, standalone unsafe globals,
   static member chains, and narrow computed member expressions with
   identifier, literal-string or decimal indices. It honors
   disableComputedWrap. Calls, assignments, imports, declarations, complex
   expression grammar, template literals, regexp ambiguity, etc. fall back.
4. Worker parallelism is across independent requests, NEVER file chunks.
   Worker count is configurable or derived from hardware concurrency.
   One user/request never shares scanner state with another.
5. Inline scripts and synchronous rewriting continue to use the WASM
   entry point with the same optional fast path and OXC fallback.

## Build and use

With the documented Rust nightly, wasm-bindgen, wasm-opt and wasm-snip toolchain:

    cd packages/core
    FEATURES=perftest pnpm rewriter:build
    pnpm build

Default builds without the perftest Cargo feature retain the OXC path.
External JS fetches use the incremental scanner when the feature is present;
failed or unrecognized cases use OXC with the original bytes.

## Optional browser worker pool

Scramjet's controller can handle fetches in a window context. Its Controller
constructor now accepts `perftestWorkers: pool` to opt in to browser Workers.
ServiceWorkerGlobalScope cannot construct Worker instances, so the service
worker keeps a local OXC path unless a client-hosted broker is in use.

Serve these assets on your origin:

- experiments/perftest/browser-pool.mjs
- experiments/perftest/wasm-worker.mjs
- experiments/perftest/create-pool.mjs
- the wasm-bindgen generated wasm.js glue script with perftest exports
- the corresponding scramjet.wasm binary

Example (paths are deployment-specific):

    import { createPerftestPool } from '/perftest/create-pool.mjs';
    const pool = createPerftestPool({
      wasmGlueUrl: '/perftest/wasm.js',
      wasmUrl: '/scramjet/scramjet.wasm',
      workers: Math.max(1, Math.floor(navigator.hardwareConcurrency / 2)),
    });
    const controller = new Controller({
      serviceworker, transport, perftestWorkers: pool,
    });
    // On app shutdown: pool.close();

The page-hosted pool only runs the AST-free safe subset. Unsupported sources
or failed Worker requests fall back to the original OXC rewrite on the
controller thread. It does not require passing arbitrary codec functions
across worker boundaries.

## Tests

    cd packages/core/rewriter
    cargo test -p slopjet-perftest-scanner
    cargo check -p wasm --features perftest --target wasm32-unknown-unknown
    cargo run --release -p slopjet-perftest-scanner --example oxc_compare -- 1000
    cargo run --release -p slopjet-perftest-scanner --example throughput -- 200
    cd ../../..
    node --test experiments/perftest/browser-pool.test.mjs

The draft PR (#1) activates Rust, WASM, OXC parity and browser pool CI.

## Observed GitHub Actions measurements

Prior passing CI run 42519669174 (small synthetic fixtures; not live sites):

| Test | AST-free / OXC ratio |
| --- | ---: |
| Single member expression | 3.02x |
| Multi-statement | 1.63x |
| Member chains | 1.79x |

Separate native lexer throughput run (200 requests, 940,000 bytes):

| Workers | Throughput MiB/s |
| --- | ---: |
| Sequential | 46.25 |
| 1 | 30.82 |
| 2 | 55.54 |
| 4 | 64.62 |
| 8 | 63.02 |

These figures do not predict real-world site performance or linear speedup.
Actual rewrite coverage, browser worker overhead, and time-to-first-byte
must be profiled on representative JavaScript and HTML corpora.

## Remaining incompatibilities / blockers

- Full ECMAScript lexical grammar, binding/scope resolution, destructuring,
  dynamic imports, assignment operators, eval, new, postMessage, import.meta,
  special flag transforms, and complete sourcemap parity are NOT implemented
  in the AST-free path. OXC handles them.
- The fast path's forward scan does not produce a complete grammar or AST,
  and it does not resolve shadowing. It safely declines unsupported files.
- Network rewriting still buffers output until EOF to ensure fallback.
  The original HTML rewrite path is unchanged.
- Browser Worker mode requires a live Controller window and hosted glue/assets;
  it is not automatically enabled by building with the perftest feature.
- Production deployment is not recommended until complete differential corpus
  and browser compatibility testing have been performed.

Main branch has not been modified. Draft PR must not be merged as production
optimization without further validation.
