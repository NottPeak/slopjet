
function emitBinding(name, payload) {
	var fn = globalThis[name];
	if (typeof fn === 'function') {
		fn(JSON.stringify(payload));
	}
}
function pass(message, details) {
	emitBinding('__testPass', { message: message, details: details });
}
function fail(message, details) {
	emitBinding('__testFail', { message: message, details: details });
}
function assertConsistent(label, value) {
	if (typeof value === 'undefined') {
		value = label;
		label = 'default';
	}
	emitBinding('__testConsistent', { label: label, value: value });
	return Promise.resolve();
}
function assert(condition, message) {
	if (!condition) { fail(message || 'Assertion failed'); throw new Error(message || 'Assertion failed'); }
}
function assertEqual(actual, expected, message) {
	if (actual !== expected) {
		var msg = message || ('Expected ' + JSON.stringify(expected) + ', got ' + JSON.stringify(actual));
		fail(msg, { actual: actual, expected: expected });
		throw new Error(msg);
	}
}
var assertEquals = assertEqual;
function waitForTestFunctions(timeout) {
	timeout = timeout || 5000;
	return new Promise(function(resolve, reject) {
		var start = Date.now();
		function check() {
			if (typeof __testPass === 'function' && typeof __testFail === 'function') resolve();
			else if (Date.now() - start > timeout) reject(new Error('Timed out'));
			else setTimeout(check, 10);
		}
		check();
	});
}
async function runTest(testFn) {
	try { await waitForTestFunctions(); await testFn(); pass(); } catch (err) { fail(err.message); }
}
