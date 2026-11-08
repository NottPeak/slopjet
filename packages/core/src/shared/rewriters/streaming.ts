import type { BareResponse } from "@mercuryworkshop/proxy-transports";
import type { ScramjetContext } from "@/shared";
import type { URLMeta } from "@rewriters/url";
import { rewriteJs } from "@rewriters/js";
import { createPerftestRewriteSession } from "@rewriters/wasm";
import { flagEnabled } from "@/shared";

/**
 * Consume the response exactly once. A compiled perftest WASM build scans
 * each network chunk in one forward pass, defers all output until EOF, and
 * uses OXC for unsupported syntax. Normal builds use the original OXC
 * response path, preserving existing behavior.
 *
 * This API is async because fetch response streams are async. It does not
 * change synchronous rewriting for inline scripts and DOM APIs.
 */
export async function rewriteJsResponse(
    response: BareResponse,
    source: string | null,
    context: ScramjetContext,
    meta: URLMeta,
    isModule = false
): Promise<string | Uint8Array> {
    if (
        flagEnabled("sourcemaps", context, meta.base) ||
        flagEnabled("captureErrors", context, meta.base) ||
        flagEnabled("scramitize", context, meta.base)
    ) {
        return rewriteJs(
            new Uint8Array(await response.arrayBuffer()),
            source,
            context,
            meta,
            isModule
        );
    }

    const options = {
        wrapfn: context.config.globals.wrapfn,
        propertybase: context.config.globals.wrappropertybase,
        propertyfn: context.config.globals.wrappropertyfn,
        disableComputedWrap: flagEnabled("disableComputedWrap", context, meta.base),
    };
    const parallel = context.interface.rewriteJsFastAsync;
    const session = parallel ? null : createPerftestRewriteSession(
        options.wrapfn, options.propertybase,
        options.propertyfn, options.disableComputedWrap
    );
    if ((!parallel && !session) || !response.body) {
        return rewriteJs(
            new Uint8Array(await response.arrayBuffer()),
            source,
            context,
            meta,
            isModule
        );
    }

    const chunks: Uint8Array[] = [];
    let total = 0;
    let fastScanFailed = false;
    // TextDecoder stream mode preserves multibyte UTF-8 across ReadableStream chunks.
    const decoder = new TextDecoder("utf-8");
    const reader = response.body.getReader();
    try {
        while (true) {
            const { done, value } = await reader.read();
            if (done) break;
            // Snapshot chunks before an underlying transport can reuse its buffer.
            chunks.push(value.slice());
            total += value.byteLength;
            if (session && !fastScanFailed) {
                try {
                    session.write(decoder.decode(value, { stream: true }));
                } catch {
                    fastScanFailed = true;
                }
            }
        }
        if (session && !fastScanFailed) {
            try { session.write(decoder.decode()); }
            catch { fastScanFailed = true; }
        }
    } finally {
        reader.releaseLock();
    }

    // Always preserve the original network bytes for OXC fallback.
    const original = new Uint8Array(total);
    let offset = 0;
    for (const part of chunks) {
        original.set(part, offset);
        offset += part.byteLength;
    }

    let fast: string | null = null;
    if (parallel) {
        try {
            // Keep original intact in case the worker rejects or cannot
            // recognize this source; its bytes are needed for OXC fallback.
            fast = await parallel(original.slice(), options);
        } catch {
            fast = null;
        }
    } else if (!fastScanFailed) {
        try { fast = session!.finish(); }
        catch { fast = null; }
    }
    // wasm-bindgen objects retain WASM heap allocations until explicitly freed.
    session?.free?.();
    if (fast !== null) {
        return new TextEncoder().encode(fast);
    }
    return rewriteJs(original, source, context, meta, isModule);
}
