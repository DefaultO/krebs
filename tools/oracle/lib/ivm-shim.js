// The isolated-vm 6.1.2 surface the official runtime uses (driver/lib/runtime/{runtime,user-vm,make}.js), over a
// node `vm` context (ARCHITECTURE.md §7.3). One "isolate" = one context. Deterministic: `cpuTime` advances
// 0.01 ms per read (the `Fixed` meter), heap statistics are constant.
'use strict';
const vm = require('vm');

const TIMEOUT_TEXT = 'Script execution timed out.';

// Deep copy of a host value into `ctx`'s realm (ExternalCopy#copyInto: structured clone, shared references and
// key order kept, undefined/NaN/Infinity kept). Functions and symbols throw like V8's ValueSerializer.
const clones = new WeakMap();   // per context; never a context global (it would be player-visible)
function intoContext(ctx, value) {
    if (!clones.has(ctx)) {
        clones.set(ctx, {
            // builtins captured before any player code can patch them (ExternalCopy is native in ivm)
            value: vm.runInContext(`(function (keys, isArray, isView, U8, A, E) {
                return function clone(v, seen) {
                    if (v === null || typeof v !== 'object') {
                        if (typeof v === 'function' || typeof v === 'symbol') throw new E('DataCloneError: ' + typeof v + ' could not be cloned.');
                        return v;
                    }
                    if (seen.has(v)) return seen.get(v);
                    var out, i, k;
                    if (isView(v)) {
                        out = new U8(v.length);
                        for (i = 0; i < v.length; i++) out[i] = v[i];
                        seen.set(v, out);
                        return out;
                    }
                    out = isArray(v) ? new A(v.length) : {};
                    seen.set(v, out);
                    k = keys(v);
                    for (i = 0; i < k.length; i++) out[k[i]] = clone(v[k[i]], seen);
                    return out;
                };
            })(Object.keys, Array.isArray, ArrayBuffer.isView, Uint8Array, Array, Error)`, ctx),
        });
    }
    return clones.get(ctx).value(value, new Map());
}

// ExternalCopy into the host realm: structuredClone (DataCloneError on functions, as ivm).
const intoHost = (value) => structuredClone(value);

function heapStatistics() {
    return {
        total_heap_size: 33554432, total_heap_size_executable: 1048576, total_physical_size: 33554432,
        total_available_size: 268435456, used_heap_size: 16777216, heap_size_limit: 285212672,
        malloced_memory: 262144, peak_malloced_memory: 524288, does_zap_garbage: 0, externally_allocated_size: 0,
    };
}

// `opts.onHalt()` is the `_halt` reference body (user-vm.js:68-71).
function makeIsolate(ctx, opts = {}) {
    let cpuNs = 0n;
    let disposed = false;

    class Script {
        constructor(code, options = {}) {
            this.script = new vm.Script(code, { filename: options.filename || '<isolated-vm>' });
        }
        runSync(context, options = {}) {
            if (disposed) throw new Error('Isolate is disposed');
            try {
                return this.script.runInContext(ctx, options.timeout ? { timeout: options.timeout, displayErrors: false } : { displayErrors: false });
            } catch (e) {
                // node's text differs; ivm throws its own Error (run_with_timeout.h:166)
                if (e && e.code === 'ERR_SCRIPT_EXECUTION_TIMEOUT') throw new (vm.runInContext('Error', ctx))(TIMEOUT_TEXT);
                throw e;
            }
        }
    }

    const isolate = {
        get cpuTime() {
            cpuNs += 10000n;
            return cpuNs;
        },
        get isDisposed() {
            return disposed;
        },
        compileScriptSync: (code, options) => new Script(code, options),
        getHeapStatisticsSync: () => intoContext(ctx, heapStatistics()),
        dispose() {
            disposed = true;
        },
    };

    class Reference {
        constructor(value) {
            this.value = value;
        }
        deref() {
            return this.value;
        }
        derefInto() {
            return this.value;
        }
        applySync(recv, args) {
            return this.value.apply(recv, args || []);
        }
        release() {}
    }

    class ExternalCopy {
        constructor(value) {
            this.value = intoHost(value);
        }
        copy() {
            return intoHost(this.value);
        }
        copyInto() {
            return intoContext(ctx, this.value);
        }
        release() {}
    }

    const ivm = { Reference, ExternalCopy };
    const halt = new Reference(() => {
        if (opts.onHalt) opts.onHalt();
        isolate.dispose();
    });
    return { ivm, isolate, halt, heapStatistics };
}

module.exports = { makeIsolate, intoContext, intoHost, heapStatistics, TIMEOUT_TEXT };
