// mirror: reference/backend/lib/cli/server.js
// parity: FAITHFUL
// Verified against the official CLI by crates/server/tests/fixtures/oracle/cli (tools/oracle/cli.js).
//
// The JS half of server.js: the greeting and the output writer of a connection. The TCP listener and the
// readline loop are backend/src/cli.rs.

exports.greeting = 'Screeps server {build} running on port 21025.\r\nThis CLI interface contains a virtual JavaScript machine which you can use to invoke internal server commands. Any valid JavaScript code is allowed. Type "help()" to learn more about commands.\r\n';

// server.js:15: `socket.write` of one output callback
exports.writer = function(write) {
    return (data, isResult) => {
        if(data === 'undefined') {
            if(isResult) {
                write("< ");
            }
            return;
        }
        write((isResult ? "< " : "") + data + "\r\n");
    };
};
