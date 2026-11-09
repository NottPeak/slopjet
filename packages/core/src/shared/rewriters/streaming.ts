import type { BareResponse } from "@mercuryworkshop/proxy-transports";
import type { ScramjetContext } from "@/shared";
import { rewriteUrl, type URLMeta } from "@rewriters/url";
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

    // Decoding follows the origin response's declared script charset.
    // Rewritten output is always emitted as UTF-8.
    const contentType = response.headers.get("content-type") || "";
    const charset = /(?:^|;)\s*charset\s*=\s*(?:"([^"]+)"|'([^']+)'|([^;\s]+))/i.exec(contentType);
    const label = charset?.[1] || charset?.[2] || charset?.[3] || "utf-8";
    let decoder: TextDecoder;
    try {
        decoder = new TextDecoder(label);
    } catch {
        decoder = new TextDecoder("utf-8");
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
    let workerBytes: Uint8Array | null = null;
    if (parallel) {
        // A worker may be unable to resolve a module URL using the host's
        // custom codec. Retry with the SAME Rust scanner locally (never OXC).
        workerBytes = new Uint8Array(await response.arrayBuffer());
        try {
            const rewritten = await parallel(workerBytes.slice(), options);
            if (typeof rewritten === "string") {
                return new TextEncoder().encode(rewritten);
            }
        } catch {
            // The local scanner will attempt the same source with full
            // URL metadata; unsupported syntax still fails closed.
        }
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
        // Decoder is initialized above using the response charset.
        if (workerBytes) {
            session.write(decoder.decode(workerBytes));
        } else if (response.body) {
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
        const rewritten = session.finish((specifier: string) =>
            rewriteUrl(specifier, context, meta, { isModule: true }));
        if (typeof rewritten !== "string") {
            throw new Error("Unsupported JavaScript syntax in " + (source ?? "(unknown)"));
        }
        return new TextEncoder().encode(rewritten);
    } finally {
        session.free?.();
    }
}
