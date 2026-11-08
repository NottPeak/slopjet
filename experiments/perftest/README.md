# perftest: streaming semantic rewriting experiment

This branch is experimental. The production OXC/WASM rewrite path remains untouched until a replacement is proven equivalent on a defined corpus.

## Goal
Build a stateful, incremental JavaScript rewriting engine which can retain lexical, grammatical and scope context across arbitrarily split input, emit patches rather than regenerate entire source files, and eventually parallelize *independent* regions without compromising semantics.

## Current upstream integration points
- `packages/core/src/shared/rewriters/js.ts`: OXC/WASM delegation; eventual feature-flagged dispatch / fallback.
- `packages/core/src/shared/rewriters/html.ts`: separate HTML streaming work, not part of the JS lexer.
- `packages/core/src/fetch/body.ts`: currently buffers JS and HTML responses.
- Existing Rust rewrite implementation and its tests: source of transformation truth, not just syntax examples.

## Design constraints
1. Byte-chunk boundaries must never change output or decisions.
2. Strings, comments, regexp literals, template strings/interpolations, and nested scopes require persistent state.
3. Scope resolution must account for declarations seen later (hoisting / lexical shadowing). Defer decisions or use a scope prepass; never assume a name is global from its spelling alone.
4. Preserve source text outside targeted patches. Use ordered, non-overlapping source offsets for edits.
5. Maintain source maps and strict-directive semantics where enabled.
6. A parser/AST fallback remains necessary for unsupported cases during rollout.
7. No arbitrary chunks to independent workers: dispatch only after determining safe syntactic boundaries and dependencies.
8. Concurrency is a measured optimization, not an architectural prerequisite.
9. Benchmark correctness *before* speed; report warmed/cold startup, byte throughput, allocations, and time-to-first-output separately.
10. Never silently use a partial transform on syntax the prototype cannot understand.

## Proposed milestones
- M0: baseline fixtures and differential test runner, measuring existing rewriter.
- M1: resumable lexical scanner + token/span output, with adversarial chunk-boundary tests.
- M2: grammar/scope tracking + safe-emission barrier + patch application.
- M3: Scramjet transform parity + OXC fallback and feature flag.
- M4: response streaming & backpressure.
- M5: bounded task pool for independently rewritable regions, only if profiling shows benefits.

## Open architecture decisions
- Implement the core in Rust/WASM alongside the current OXC rewriter, or first prototype in TypeScript for iteration speed?
- Require full Scramjet rewrite parity before enabling the feature flag on live sites, or allow explicitly opt-in partial support with automatic fallback?
- Should HTML streaming be a separate benchmark track after the JavaScript semantic engine?

No performance claims are made until measured. This document is a plan, not a completed engine.
