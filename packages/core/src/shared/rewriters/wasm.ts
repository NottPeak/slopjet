// i am a cat. i like to be petted. i like to be fed. i like to be
import { initSync, Rewriter } from "../../../rewriter/wasm/out/wasm.js";
import * as WasmExports from "../../../rewriter/wasm/out/wasm.js";
import type { JsRewriterOutput } from "../../../rewriter/wasm/out/wasm.js";
import { flagEnabled, ScramjetContext } from "@/shared";

export type { JsRewriterOutput, Rewriter };

import { URLMeta } from "@rewriters/url";
import { Error, TextDecoder_decode } from "@/shared/snapshot";

let wasm_u8: Uint8Array;
let initializedBytes: Uint8Array | undefined;
export function setWasm(u8: Uint8Array | ArrayBuffer) {
	wasm_u8 = u8 instanceof Uint8Array ? u8 : new Uint8Array(u8);
}

const MAGIC = "\0asm".split("").map((x) => x.charCodeAt(0));

function initWasm() {
	// Do not recompile the identical module on every intercepted JS request.
	if (!(wasm_u8 instanceof Uint8Array))
		throw new Error("rewriter wasm not found (was setWasm called?)");
	if (initializedBytes === wasm_u8) return;

	if (![...wasm_u8.slice(0, 4)].every((x, i) => x === MAGIC[i]))
		throw new Error(
			"rewriter wasm does not have wasm magic (was it fetched correctly?)\nrewriter wasm contents: " +
				TextDecoder_decode(wasm_u8)
		);

	initSync({
		module: new WebAssembly.Module(wasm_u8 as unknown as BufferSource),
	});
	initializedBytes = wasm_u8;
}

type RewriterBox = { rewriter: Rewriter; inUse: boolean };
const rewriters: RewriterBox[] = [];
export function getRewriter(
	context: ScramjetContext,
	meta: URLMeta
): [Rewriter, () => void] {
	initWasm();

	let obj: RewriterBox;
	const index = rewriters.findIndex((x) => !x.inUse);
	const len = rewriters.length;

	if (index === -1) {
		if (flagEnabled("rewriterLogs", context, meta.base))
			dbg.log(`creating new rewriter, ${len} rewriters made already`);

		const rewriter = new Rewriter();
		obj = { rewriter, inUse: false };
		rewriters.push(obj);
	} else {
		obj = rewriters[index];
	}
	obj.inUse = true;

	return [obj.rewriter, () => (obj.inUse = false)];
}


/**
 * Experimental WASM chunked rewriting API. Only available when the WASM
 * rewriter is compiled with Cargo feature "perftest". Ordinary builds
 * return null and continue using OXC without any behavior change.
 */
export type PerftestRewriteSessionLike = {
    write(chunk: string): void;
    finish(rewriteUrl?: (specifier: string) => string): string | null;
    free?(): void;
};
export function createPerftestRewriteSession(
    wrapfn: string,
    propertybase: string,
    propertyfn: string,
    disableComputedWrap: boolean,
    importfn: string,
    rewritefn: string,
    postmessagefn: string,
    base: string,
    metafn: string,
    module: boolean,
    trysetfn: string
): PerftestRewriteSessionLike | null {
    const exports = WasmExports as unknown as {
        PerftestRewriteSession?: new (
            wrapfn: string,
            propertybase: string,
            propertyfn: string,
            disableComputedWrap: boolean,
            importfn: string,
            rewritefn: string,
            postmessagefn: string,
            base: string,
            metafn: string,
            module: boolean,
            trysetfn: string
        ) => PerftestRewriteSessionLike
    };
    const Constructor = exports.PerftestRewriteSession;
    if (typeof Constructor !== "function") return null;
    initWasm();
    return new Constructor(wrapfn, propertybase, propertyfn, disableComputedWrap, importfn, rewritefn, postmessagefn, base, metafn, module, trysetfn);
}
