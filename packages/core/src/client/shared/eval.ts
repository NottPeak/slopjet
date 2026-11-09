import { rewriteJs } from "@rewriters/js";
import { ScramjetClient } from "@client/index";
import { Object_defineProperty, String } from "@/shared/snapshot";

export default function (client: ScramjetClient, self: Self) {
	// used for proxying *direct eval*
	// eval("...") -> eval($scramjet$rewrite("..."))
	Object_defineProperty(self, client.config.globals.rewritefn, {
		value: function (js: any, spread = false) {
			// A direct eval whose first argument is a spread expression must
			// remain a direct eval call. The scanner transforms eval(...value)
			// into eval(...rewritefn(value, true)), not an indirect call.
			// Native spread iterates exactly once and preserves a TypeError for
			// non-iterables. Convert only the first argument, as eval does.
			if (spread) {
				const args = [...js];
				if (args.length) {
					let first = args[0];
					if (client.box.instanceof(first, "TrustedScript")) first = String(first);
					if (typeof first === "string") {
						first = rewriteJs(first, "(direct eval proxy)", client.context, client.meta);
					}
					args[0] = first;
				}
				return args;
			}
			if (client.box.instanceof(js, "TrustedScript")) js = String(js);
			if (typeof js !== "string") return js;
			return rewriteJs(js, "(direct eval proxy)", client.context, client.meta);
		},
		writable: false,
		configurable: false,
	});
}

export function createIndirectEval(client: ScramjetClient) {
	const indirection = client.global.eval;
	const proxy = new Proxy(client.global.eval, {
		apply(_target, _thisArg, args) {
			let js = args[0];
			// > If the argument of eval() is not a string, eval() returns the argument unchanged
			// the one exception is TrustedScript, which can just be stringified and rewritten
			if (client.box.instanceof(js, "TrustedScript")) js = String(js);
			if (typeof js !== "string") return js;

			return indirection(
				rewriteJs(
					js,
					"(indirect eval proxy)",
					client.context,
					client.meta
				) as string
			);
		},
	});
	client.box.unproxy.set(proxy, client.global.eval);

	return proxy;
}
