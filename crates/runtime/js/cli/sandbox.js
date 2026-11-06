// mirror: reference/backend/lib/cli/sandbox.js
// parity: FAITHFUL
// Verified against the official CLI by crates/server/tests/fixtures/oracle/cli (tools/oracle/cli.js).
//
// The connection's realm is the sandbox: its global gets `print`, `storage`, `map`, `bots`, `strongholds`,
// `system` and `help` (vm.createContext makes the sandbox object the global), and a line runs as a global-scope
// script (vm.runInContext). `cliSandbox` handlers of mods: none yet (JS mods' server slot).
const util = require('util');
const common = require('@screeps/common');

function createSandbox(outputCallback) {
    var sandbox = {
        print() {
            outputCallback(Array.prototype.slice.apply(arguments).map( i => util.inspect(i)).join(" "));
        },
        storage: common.storage,
        map: require('./map'),
        bots: require('./bots'),
        strongholds: require('./strongholds'),
        system: require('./system')
    };

    require('./help')(sandbox);

    return sandbox;
}

function create(outputCallback) {

    Object.assign(globalThis, createSandbox(outputCallback));
    const runInContext = (0, eval);

    return function(command) {
        try {
            var result = runInContext(command);
            if(result && result.then) {
                result.then(
                    data => {
                        if(data) {
                            outputCallback(util.inspect(data), true);
                        }
                    },
                    err => {
                        outputCallback("Error: "+(err.stack || err), true);
                    }
                );
            }
            else {
                outputCallback(""+result, true);
            }

        }
        catch(e) {
            outputCallback(e.toString(), true);
        }
    };
}

exports.create = create;
