import { isHtmlMimeType, isJavascriptMimeType, ScramjetHeaders } from "@/shared";
import { BareResponse } from "@mercuryworkshop/proxy-transports";
import { ScramjetFetchParsed } from ".";

export function normalizeContentType(
	parsed: ScramjetFetchParsed,
	headers: ScramjetHeaders
) {
	const ct = headers.get("content-type");
	if (isDocument(parsed)) {
		if (ct && isHtmlMimeType(ct))
			headers.set("content-type", "text/html; charset=utf-8");
		return;
	}

	// JS network rewriting emits UTF-8 bytes even if the source script used
	// a legacy charset. Keep the MIME type (module scripts validate it)
	// but update its declared encoding to match the transformed payload.
	if (
		parsed.destination === "script" ||
		parsed.destination === "worker" ||
		parsed.destination === "sharedworker"
	) {
		if (!ct || !isJavascriptMimeType(ct)) return;
		const mime = ct.split(";")[0].trim();
		headers.set("content-type", mime + "; charset=utf-8");
	}
}

export function isRedirect(response: BareResponse) {
	return response.status >= 300 && response.status < 400;
}

export function isDocument(parsed: ScramjetFetchParsed) {
	return parsed.destination === "document" || parsed.destination === "iframe";
}

export function createReferrerString(
	clientUrl: URL,
	resource: URL,
	policy: string | null
): string {
	policy ||= "strict-origin-when-cross-origin";
	const originIsHttps = clientUrl.protocol === "https:";
	const destIsHttps = resource.protocol === "https:";

	const isPotentialDowngrade = originIsHttps && !destIsHttps;

	const isSameOrigin =
		clientUrl.protocol === resource.protocol &&
		clientUrl.host === resource.host;

	const referrerOrigin = clientUrl.origin;

	const referrerUrl = new URL(clientUrl.href);
	referrerUrl.hash = "";
	const referrerUrlString = referrerUrl.href;

	switch (policy) {
		case "no-referrer":
			return "";

		case "no-referrer-when-downgrade":
			if (isPotentialDowngrade) return "";
			return referrerUrlString;

		case "same-origin":
			if (isSameOrigin) return referrerUrlString;
			return "";

		case "origin":
			return referrerOrigin === "null" ? "" : referrerOrigin + "/";

		case "strict-origin":
			if (isPotentialDowngrade) return "";
			return referrerOrigin === "null" ? "" : referrerOrigin + "/";

		case "origin-when-cross-origin":
			if (isSameOrigin) return referrerUrlString;
			return referrerOrigin === "null" ? "" : referrerOrigin + "/";

		case "strict-origin-when-cross-origin":
			if (isSameOrigin) return referrerUrlString;
			if (isPotentialDowngrade) return "";
			return referrerOrigin === "null" ? "" : referrerOrigin + "/";

		case "unsafe-url":
			return referrerUrlString;

		default:
			return "";
	}
}
