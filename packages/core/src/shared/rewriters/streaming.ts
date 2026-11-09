import type { BareResponse } from "@mercuryworkshop/proxy-transports";
import type { ScramjetContext } from "@/shared";
import type { URLMeta } from "@rewriters/url";
import { createPerftestRewriteSession } from "@rewriters/wasm";
import { flagEnabled } from "@/shared";

/**
 * Rust/WASM-only JavaScript response rewriter.
 *
 * One forward scan per response. Per-request state stays private. No parser
 * fallback, original-source passthrough, or speculative output is allowed.
 * If a script is not supported, the response is rejected explicitly.
 */
export async function rewriteJsResponse(
    response: BareResponse,
    source: string | null,
    context: ScramjetContext,
    meta: URLMeta,
    isModule = false
): Promise<Uint8Array> {
    for (const name of ["sourcemaps", "captureErrors", "scramitize", "destructureRewrites"] as const) {
        if (flagEnabled(name, context, meta.base)) {
            throw new Error("Slopjet scanner does not support transform: " + name);
        }
    }

    const options = {
        wrapfn: context.config.globals.wrapfn,
        propertybase: context.config.globals.wrappropertybase,
        propertyfn: context.config.globals.wrappropertyfn,
        disableComputedWrap: flagEnabled("disableComputedWrap", context, meta.base),
        importfn: context.config.globals.importfn,
        rewritefn: context.config.globals.rewritefn,
        postmessagefn: context.config.globals.wrappostmessagefn,
        base: meta.base.href,
        metafn: context.config.globals.metafn,
        trysetfn: context.config.globals.trysetfn,
        module: isModule,
    };
    const parallel = context.interface.rewriteJsFastAsync;
    if (parallel) {
        // Workers process independent requests, never parts of one source.
        const bytes = new Uint8Array(await response.arrayBuffer());
        const rewritten = await parallel(bytes, options);
        if (typeof rewritten !== "string") {
            throw new Error("Unsupported JS in " + (source ?? "(unknown)") + "; module=" + isModule);
        }
        return new TextEncoder().encode(rewritten);
    }

    const session = createPerftestRewriteSession(
        options.wrapfn, options.propertybase,
        options.propertyfn, options.disableComputedWrap,
        options.importfn, options.rewritefn, options.postmessagefn, options.base,
        options.metafn, options.module, options.trysetfn
    );
    if (!session) {
        throw new Error("Slopjet scanner WASM not available; refusing unrewritten JS");
    }

    try {
        const decoder = new TextDecoder("utf-8", { fatal: true });
        if (response.body) {
            const reader = response.body.getReader();
            try {
                while (true) {
                    const { done, value } = await reader.read();
                    if (done) break;
                    session.write(decoder.decode(value, { stream: true }));
                }
                session.write(decoder.decode());
            } finally {
                reader.releaseLock();
            }
        } else {
            session.write(decoder.decode(new Uint8Array(await response.arrayBuffer())));
        }
        const rewritten = session.finish();
        if (typeof rewritten !== "string") {
            throw new Error("Unsupported JavaScript syntax in " + (source ?? "(unknown)"));
        }
        return new TextEncoder().encode(rewritten);
    } finally {
        session.free?.();
    }
}
