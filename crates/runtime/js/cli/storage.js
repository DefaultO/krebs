// mirror: reference/common/lib/storage.js
// parity: FAITHFUL
// Verified against the official LokiJS 1.5.9 + storage db.js by tools/oracle/cli-storage.js (fixtures in
// crates/runtime/tests/fixtures/cli).
//
// The CLI's `storage` (CLI-STORAGE): common/lib/storage.js's wrappers, and behind them what the storage process
// runs for each request (reference/storage/lib/db.js dbRequest/dbUpdate/dbBulk/dbFindEx and its LokiJS
// collection calls), here in the CLI's own isolate. Arguments and results cross JSON as over the storage RPC.
// Documents come from the world (`natives.docs(collection)`, creation order = Loki's data order, every binary index
// lazy and rebuilt per query: STORE-ORDER-INDEXED); writes go back as whole documents (`__cli.write`), env and
// pubsub to the host. No `$loki`/`meta` fields (FMT-LOKI-ARTIFACTS).
// Not reproduced (STORE-ALIAS): Loki keeps the objects a request passed in, so a `$set` of one object on several
// docs shares it between them in memory until a restart; here every request copies.
// ponytail: indices behave as lazy ones everywhere; vanilla's runtime-created collections use adaptive indices,
// which order equal keys most-recently-written first (STORE-ORDER-ADAPTIVE).
'use strict';
var _ = require('lodash');
var natives = __cli;

var dbCollections = [
    'leaderboard.power',
    'leaderboard.seasons',
    'leaderboard.world',
    'users.intents',
    'market.orders',
    'market.stats',
    'rooms',
    'rooms.objects',
    'rooms.flags',
    'rooms.intents',
    'rooms.terrain',
    'transactions',
    'users',
    'users.code',
    'users.console',
    'users.messages',
    'users.money',
    'users.notifications',
    'users.resources',
    'users.power_creeps'
];

// db.js getOrAddCollection: the binary indices of each collection (STORE-INDEXES)
var INDICES = {
    'rooms.objects': ['room', 'user'],
    'rooms.intents': ['room'],
    'users': ['username'],
    'rooms.flags': ['room', 'user'],
    'rooms.terrain': ['room'],
    'transactions': ['user'],
    'users.code': ['user'],
    'users.console': ['user'],
    'users.money': ['user'],
    'users.notifications': ['user'],
    'users.resources': ['user'],
    'users.power_creeps': ['user']
};

var hasOwnProperty = Object.prototype.hasOwnProperty;

// ---- LokiJS 1.5.9 comparators and operators ----

function aeqHelper(prop1, prop2) {
    var cv1, cv2, t1, t2;
    if (prop1 === prop2) return true;
    if (!prop1 || !prop2 || prop1 === true || prop2 === true || prop1 !== prop1 || prop2 !== prop2) {
        switch (prop1) {
            case undefined: t1 = 1; break;
            case null: t1 = 1; break;
            case false: t1 = 3; break;
            case true: t1 = 4; break;
            case "": t1 = 5; break;
            default: t1 = (prop1 === prop1) ? 9 : 0; break;
        }
        switch (prop2) {
            case undefined: t2 = 1; break;
            case null: t2 = 1; break;
            case false: t2 = 3; break;
            case true: t2 = 4; break;
            case "": t2 = 5; break;
            default: t2 = (prop2 === prop2) ? 9 : 0; break;
        }
        if (t1 !== 9 || t2 !== 9) {
            return (t1 === t2);
        }
    }
    cv1 = Number(prop1);
    cv2 = Number(prop2);
    if (cv1 === cv1 || cv2 === cv2) {
        return (cv1 === cv2);
    }
    cv1 = prop1.toString();
    cv2 = prop2.toString();
    return (cv1 == cv2);
}

function ltHelper(prop1, prop2, equal) {
    var cv1, cv2, t1, t2;
    if (!prop1 || !prop2 || prop1 === true || prop2 === true || prop1 !== prop1 || prop2 !== prop2) {
        switch (prop1) {
            case undefined: t1 = 1; break;
            case null: t1 = 1; break;
            case false: t1 = 3; break;
            case true: t1 = 4; break;
            case "": t1 = 5; break;
            default: t1 = (prop1 === prop1) ? 9 : 0; break;
        }
        switch (prop2) {
            case undefined: t2 = 1; break;
            case null: t2 = 1; break;
            case false: t2 = 3; break;
            case true: t2 = 4; break;
            case "": t2 = 5; break;
            default: t2 = (prop2 === prop2) ? 9 : 0; break;
        }
        if (t1 !== 9 || t2 !== 9) {
            return (t1 === t2) ? equal : (t1 < t2);
        }
    }
    cv1 = Number(prop1);
    cv2 = Number(prop2);
    if (cv1 === cv1 && cv2 === cv2) {
        if (cv1 < cv2) return true;
        if (cv1 > cv2) return false;
        return equal;
    }
    if (cv1 === cv1 && cv2 !== cv2) {
        return true;
    }
    if (cv2 === cv2 && cv1 !== cv1) {
        return false;
    }
    if (prop1 < prop2) return true;
    if (prop1 > prop2) return false;
    if (prop1 == prop2) return equal;
    cv1 = prop1.toString();
    cv2 = prop2.toString();
    if (cv1 < cv2) {
        return true;
    }
    if (cv1 == cv2) {
        return equal;
    }
    return false;
}

function gtHelper(prop1, prop2, equal) {
    var cv1, cv2, t1, t2;
    if (!prop1 || !prop2 || prop1 === true || prop2 === true || prop1 !== prop1 || prop2 !== prop2) {
        switch (prop1) {
            case undefined: t1 = 1; break;
            case null: t1 = 1; break;
            case false: t1 = 3; break;
            case true: t1 = 4; break;
            case "": t1 = 5; break;
            default: t1 = (prop1 === prop1) ? 9 : 0; break;
        }
        switch (prop2) {
            case undefined: t2 = 1; break;
            case null: t2 = 1; break;
            case false: t2 = 3; break;
            case true: t2 = 4; break;
            case "": t2 = 5; break;
            default: t2 = (prop2 === prop2) ? 9 : 0; break;
        }
        if (t1 !== 9 || t2 !== 9) {
            return (t1 === t2) ? equal : (t1 > t2);
        }
    }
    cv1 = Number(prop1);
    cv2 = Number(prop2);
    if (cv1 === cv1 && cv2 === cv2) {
        if (cv1 > cv2) return true;
        if (cv1 < cv2) return false;
        return equal;
    }
    if (cv1 === cv1 && cv2 !== cv2) {
        return false;
    }
    if (cv2 === cv2 && cv1 !== cv1) {
        return true;
    }
    if (prop1 > prop2) return true;
    if (prop1 < prop2) return false;
    if (prop1 == prop2) return equal;
    cv1 = prop1.toString();
    cv2 = prop2.toString();
    if (cv1 > cv2) {
        return true;
    }
    if (cv1 == cv2) {
        return equal;
    }
    return false;
}

var Comparators = { aeq: aeqHelper, lt: ltHelper, gt: gtHelper };

function sortHelper(prop1, prop2, desc) {
    if (Comparators.aeq(prop1, prop2)) return 0;
    if (Comparators.lt(prop1, prop2, false)) {
        return (desc) ? (1) : (-1);
    }
    if (Comparators.gt(prop1, prop2, false)) {
        return (desc) ? (-1) : (1);
    }
    return 0;
}

function getIn(object, path, usingDotNotation) {
    if (object == null) {
        return undefined;
    }
    if (!usingDotNotation) {
        return object[path];
    }
    if (typeof (path) === "string") {
        path = path.split(".");
    }
    var index = 0, length = path.length;
    while (object != null && index < length) {
        object = object[path[index++]];
    }
    return (index && index == length) ? object : undefined;
}

function dotSubScan(root, paths, fun, value, poffset) {
    var pathOffset = poffset || 0;
    var path = paths[pathOffset];
    var valueFound = false;
    var element;
    if (typeof root === 'object' && path in root) {
        element = root[path];
    }
    if (pathOffset + 1 >= paths.length) {
        valueFound = fun(element, value);
    } else if (Array.isArray(element)) {
        for (var index = 0, len = element.length; index < len; index += 1) {
            valueFound = dotSubScan(element[index], paths, fun, value, pathOffset + 1);
            if (valueFound === true) {
                break;
            }
        }
    } else {
        valueFound = dotSubScan(element, paths, fun, value, pathOffset + 1);
    }
    return valueFound;
}

function containsCheckFn(a) {
    if (typeof a === 'string' || Array.isArray(a)) {
        return function (b) {
            return a.indexOf(b) !== -1;
        };
    } else if (typeof a === 'object' && a !== null) {
        return function (b) {
            return hasOwnProperty.call(a, b);
        };
    }
    return null;
}

function doQueryOp(val, op) {
    for (var p in op) {
        if (hasOwnProperty.call(op, p)) {
            return LokiOps[p](val, op[p]);
        }
    }
    return false;
}

var LokiOps = {
    $eq: function (a, b) { return a === b; },
    $aeq: function (a, b) { return a == b; },
    $ne: function (a, b) {
        if (b !== b) {
            return (a === a);
        }
        return a !== b;
    },
    $dteq: function (a, b) { return Comparators.aeq(a, b); },
    $gt: function (a, b) { return Comparators.gt(a, b, false); },
    $gte: function (a, b) { return Comparators.gt(a, b, true); },
    $lt: function (a, b) { return Comparators.lt(a, b, false); },
    $lte: function (a, b) { return Comparators.lt(a, b, true); },
    $jgt: function (a, b) { return a > b; },
    $jgte: function (a, b) { return a >= b; },
    $jlt: function (a, b) { return a < b; },
    $jlte: function (a, b) { return a <= b; },
    $between: function (a, vals) {
        if (a === undefined || a === null) return false;
        return (Comparators.gt(a, vals[0], true) && Comparators.lt(a, vals[1], true));
    },
    $jbetween: function (a, vals) {
        if (a === undefined || a === null) return false;
        return (a >= vals[0] && a <= vals[1]);
    },
    $in: function (a, b) { return b.indexOf(a) !== -1; },
    $nin: function (a, b) { return b.indexOf(a) === -1; },
    $keyin: function (a, b) { return a in b; },
    $nkeyin: function (a, b) { return !(a in b); },
    $definedin: function (a, b) { return b[a] !== undefined; },
    $undefinedin: function (a, b) { return b[a] === undefined; },
    $regex: function (a, b) { return b.test(a); },
    $containsString: function (a, b) { return (typeof a === 'string') && (a.indexOf(b) !== -1); },
    $containsNone: function (a, b) { return !LokiOps.$containsAny(a, b); },
    $containsAny: function (a, b) {
        var checkFn = containsCheckFn(a);
        if (checkFn !== null) {
            return (Array.isArray(b)) ? (b.some(checkFn)) : (checkFn(b));
        }
        return false;
    },
    $contains: function (a, b) {
        var checkFn = containsCheckFn(a);
        if (checkFn !== null) {
            return (Array.isArray(b)) ? (b.every(checkFn)) : (checkFn(b));
        }
        return false;
    },
    $elemMatch: function (a, b) {
        if (Array.isArray(a)) {
            return a.some(function (item) {
                return Object.keys(b).every(function (property) {
                    var filter = b[property];
                    if (!(typeof filter === 'object' && filter)) {
                        filter = { $eq: filter };
                    }
                    if (property.indexOf('.') !== -1) {
                        return dotSubScan(item, property.split('.'), doQueryOp, b[property]);
                    }
                    return doQueryOp(item[property], filter);
                });
            });
        }
        return false;
    },
    $type: function (a, b) {
        var type = typeof a;
        if (type === 'object') {
            if (Array.isArray(a)) {
                type = 'array';
            } else if (a instanceof Date) {
                type = 'date';
            }
        }
        return (typeof b !== 'object') ? (type === b) : doQueryOp(type, b);
    },
    $finite: function (a, b) { return (b === isFinite(a)); },
    $size: function (a, b) {
        if (Array.isArray(a)) {
            return (typeof b !== 'object') ? (a.length === b) : doQueryOp(a.length, b);
        }
        return false;
    },
    $len: function (a, b) {
        if (typeof a === 'string') {
            return (typeof b !== 'object') ? (a.length === b) : doQueryOp(a.length, b);
        }
        return false;
    },
    $where: function (a, b) { return b(a) === true; },
    $not: function (a, b) { return !doQueryOp(a, b); },
    $and: function (a, b) {
        for (var idx = 0, len = b.length; idx < len; idx += 1) {
            if (!doQueryOp(a, b[idx])) {
                return false;
            }
        }
        return true;
    },
    $or: function (a, b) {
        for (var idx = 0, len = b.length; idx < len; idx += 1) {
            if (doQueryOp(a, b[idx])) {
                return true;
            }
        }
        return false;
    },
    $exists: function (a, b) {
        if (b) {
            return a !== undefined;
        } else {
            return a === undefined;
        }
    }
};

var indexedOps = {
    $eq: LokiOps.$eq,
    $aeq: true,
    $dteq: true,
    $gt: true,
    $gte: true,
    $lt: true,
    $lte: true,
    $in: true,
    $between: true
};

function precompileQuery(operator, value) {
    if (operator === '$regex') {
        if (Array.isArray(value)) {
            value = new RegExp(value[0], value[1]);
        } else if (!(value instanceof RegExp)) {
            value = new RegExp(value);
        }
    } else if (typeof value === 'object') {
        for (var key in value) {
            if (key === '$regex' || typeof value[key] === 'object') {
                value[key] = precompileQuery(key, value[key]);
            }
        }
    }
    return value;
}

// ---- LokiJS Collection / Resultset over a snapshot of the docs ----

function Collection(name, data) {
    this.name = name;
    this.data = data;
    this.binaryIndices = {};
    var self = this;
    (INDICES[name] || []).forEach(function (p) {
        self.binaryIndices[p] = { name: p, dirty: true, values: [] };
    });
}

Collection.prototype.prepareFullDocIndex = function () {
    var len = this.data.length;
    var indexes = new Array(len);
    for (var i = 0; i < len; i += 1) {
        indexes[i] = i;
    }
    return indexes;
};

// non-adaptive ensureIndex: sort positions by the property (stable)
Collection.prototype.ensureIndex = function (property) {
    if (this.binaryIndices[property] && !this.binaryIndices[property].dirty) return;
    var index = { name: property, dirty: true, values: this.prepareFullDocIndex() };
    this.binaryIndices[property] = index;
    var data = this.data;
    var propPath = ~property.indexOf('.') ? property.split('.') : false;
    index.values.sort(function (a, b) {
        var val1, val2;
        if (propPath) {
            val1 = getIn(data[a], propPath, true);
            val2 = getIn(data[b], propPath, true);
        } else {
            val1 = data[a][property];
            val2 = data[b][property];
        }
        if (val1 !== val2) {
            if (Comparators.lt(val1, val2, false)) return -1;
            if (Comparators.gt(val1, val2, false)) return 1;
        }
        return 0;
    });
    index.dirty = false;
};

Collection.prototype.calculateRangeStart = function (prop, val, adaptive, usingDotNotation) {
    var rcd = this.data;
    var index = this.binaryIndices[prop].values;
    var min = 0;
    var max = index.length - 1;
    var mid = 0;
    if (index.length === 0) {
        return -1;
    }
    while (min < max) {
        mid = (min + max) >> 1;
        if (Comparators.lt(getIn(rcd[index[mid]], prop, usingDotNotation), val, false)) {
            min = mid + 1;
        } else {
            max = mid;
        }
    }
    var lbound = min;
    if (Comparators.aeq(val, getIn(rcd[index[lbound]], prop, usingDotNotation))) {
        return lbound;
    }
    if (Comparators.lt(val, getIn(rcd[index[lbound]], prop, usingDotNotation), false)) {
        return adaptive ? lbound : lbound - 1;
    }
    return adaptive ? lbound + 1 : lbound;
};

Collection.prototype.calculateRangeEnd = function (prop, val, usingDotNotation) {
    var rcd = this.data;
    var index = this.binaryIndices[prop].values;
    var min = 0;
    var max = index.length - 1;
    var mid = 0;
    if (index.length === 0) {
        return -1;
    }
    while (min < max) {
        mid = (min + max) >> 1;
        if (Comparators.lt(val, getIn(rcd[index[mid]], prop, usingDotNotation), false)) {
            max = mid;
        } else {
            min = mid + 1;
        }
    }
    var ubound = max;
    if (Comparators.aeq(val, getIn(rcd[index[ubound]], prop, usingDotNotation))) {
        return ubound;
    }
    if (Comparators.gt(val, getIn(rcd[index[ubound]], prop, usingDotNotation), false)) {
        return ubound + 1;
    }
    if (Comparators.aeq(val, getIn(rcd[index[ubound - 1]], prop, usingDotNotation))) {
        return ubound - 1;
    }
    return ubound;
};

Collection.prototype.calculateRange = function (op, prop, val) {
    var rcd = this.data;
    var index = this.binaryIndices[prop].values;
    var min = 0;
    var max = index.length - 1;
    var lbound, lval;
    var ubound;
    if (rcd.length === 0) {
        return [0, -1];
    }
    var usingDotNotation = (prop.indexOf('.') !== -1);
    var minVal = getIn(rcd[index[min]], prop, usingDotNotation);
    var maxVal = getIn(rcd[index[max]], prop, usingDotNotation);
    switch (op) {
        case '$eq':
        case '$aeq':
            if (Comparators.lt(val, minVal, false) || Comparators.gt(val, maxVal, false)) {
                return [0, -1];
            }
            break;
        case '$dteq':
            if (Comparators.lt(val, minVal, false) || Comparators.gt(val, maxVal, false)) {
                return [0, -1];
            }
            break;
        case '$gt':
            if (Comparators.gt(val, maxVal, true)) {
                return [0, -1];
            }
            if (Comparators.gt(minVal, val, false)) {
                return [min, max];
            }
            break;
        case '$gte':
            if (Comparators.gt(val, maxVal, false)) {
                return [0, -1];
            }
            if (Comparators.gt(minVal, val, true)) {
                return [min, max];
            }
            break;
        case '$lt':
            if (Comparators.lt(val, minVal, true)) {
                return [0, -1];
            }
            if (Comparators.lt(maxVal, val, false)) {
                return [min, max];
            }
            break;
        case '$lte':
            if (Comparators.lt(val, minVal, false)) {
                return [0, -1];
            }
            if (Comparators.lt(maxVal, val, true)) {
                return [min, max];
            }
            break;
        case '$between':
            if (Comparators.gt(val[0], maxVal, false)) {
                return [0, -1];
            }
            if (Comparators.lt(val[1], minVal, false)) {
                return [0, -1];
            }
            lbound = this.calculateRangeStart(prop, val[0], false, usingDotNotation);
            ubound = this.calculateRangeEnd(prop, val[1], usingDotNotation);
            if (lbound < 0) lbound++;
            if (ubound > max) ubound--;
            if (!Comparators.gt(getIn(rcd[index[lbound]], prop, usingDotNotation), val[0], true)) lbound++;
            if (!Comparators.lt(getIn(rcd[index[ubound]], prop, usingDotNotation), val[1], true)) ubound--;
            if (ubound < lbound) return [0, -1];
            return ([lbound, ubound]);
        case '$in':
            var idxset = [],
                segResult = [];
            for (var j = 0, len = val.length; j < len; j++) {
                var seg = this.calculateRange('$eq', prop, val[j]);
                for (var i = seg[0]; i <= seg[1]; i++) {
                    if (idxset[i] === undefined) {
                        idxset[i] = true;
                        segResult.push(i);
                    }
                }
            }
            return segResult;
    }
    switch (op) {
        case '$eq':
        case '$aeq':
        case '$dteq':
        case '$gte':
        case '$lt':
            lbound = this.calculateRangeStart(prop, val, false, usingDotNotation);
            lval = getIn(rcd[index[lbound]], prop, usingDotNotation);
            break;
        default: break;
    }
    switch (op) {
        case '$eq':
        case '$aeq':
        case '$dteq':
        case '$lte':
        case '$gt':
            ubound = this.calculateRangeEnd(prop, val, usingDotNotation);
            break;
        default: break;
    }
    switch (op) {
        case '$eq':
        case '$aeq':
        case '$dteq':
            if (!Comparators.aeq(lval, val)) {
                return [0, -1];
            }
            return [lbound, ubound];
        case '$gt':
            if (!Comparators.aeq(getIn(rcd[index[ubound]], prop, usingDotNotation), val)) {
                return [ubound, max];
            }
            return [ubound + 1, max];
        case '$gte':
            if (!Comparators.aeq(getIn(rcd[index[lbound]], prop, usingDotNotation), val)) {
                return [lbound + 1, max];
            }
            return [lbound, max];
        case '$lt':
            if (!Comparators.aeq(getIn(rcd[index[lbound]], prop, usingDotNotation), val)) {
                return [min, lbound];
            }
            return [min, lbound - 1];
        case '$lte':
            if (!Comparators.aeq(getIn(rcd[index[ubound]], prop, usingDotNotation), val)) {
                return [min, ubound - 1];
            }
            return [min, ubound];
        default:
            return [0, rcd.length - 1];
    }
};

function Resultset(collection) {
    this.collection = collection;
    this.filteredrows = [];
    this.filterInitialized = false;
}

Resultset.prototype.count = function () {
    if (this.filterInitialized) {
        return this.filteredrows.length;
    }
    return this.collection.data.length;
};

Resultset.prototype.branch = function () {
    var result = new Resultset(this.collection);
    if (this.filteredrows.length > 0) {
        result.filteredrows = this.filteredrows.slice();
    }
    result.filterInitialized = this.filterInitialized;
    return result;
};

Resultset.prototype.findOr = function (expressionArray) {
    var fr = null, fri = 0, frlen = 0, docset = [], idxset = [], idx = 0;
    for (var ei = 0, elen = expressionArray.length; ei < elen; ei++) {
        fr = this.branch().find(expressionArray[ei]).filteredrows;
        frlen = fr.length;
        for (fri = 0; fri < frlen; fri++) {
            idx = fr[fri];
            if (idxset[idx] === undefined) {
                idxset[idx] = true;
                docset.push(idx);
            }
        }
    }
    this.filteredrows = docset;
    this.filterInitialized = true;
    return this;
};
Resultset.prototype.$or = Resultset.prototype.findOr;

Resultset.prototype.findAnd = function (expressionArray) {
    for (var i = 0, len = expressionArray.length; i < len; i++) {
        if (this.count() === 0) {
            return this;
        }
        this.find(expressionArray[i]);
    }
    return this;
};
Resultset.prototype.$and = Resultset.prototype.findAnd;

Resultset.prototype.find = function (query, firstOnly) {
    if (this.collection.data.length === 0) {
        this.filteredrows = [];
        this.filterInitialized = true;
        return this;
    }
    var queryObject = query || 'getAll', p, property, queryObjectOp, obj, operator, value, key,
        searchByIndex = false, result = [], filters = [], index = null;
    firstOnly = firstOnly || false;
    if (typeof queryObject === 'object') {
        for (p in queryObject) {
            obj = {};
            obj[p] = queryObject[p];
            filters.push(obj);
            if (hasOwnProperty.call(queryObject, p)) {
                property = p;
                queryObjectOp = queryObject[p];
            }
        }
        if (filters.length > 1) {
            return this.find({ '$and': filters }, firstOnly);
        }
    }
    if (!property || queryObject === 'getAll') {
        if (firstOnly) {
            if (this.filterInitialized) {
                this.filteredrows = this.filteredrows.slice(0, 1);
            } else {
                this.filteredrows = (this.collection.data.length > 0) ? [0] : [];
                this.filterInitialized = true;
            }
        }
        return this;
    }
    if (property === '$and' || property === '$or') {
        this[property](queryObjectOp);
        if (firstOnly && this.filteredrows.length > 1) {
            this.filteredrows = this.filteredrows.slice(0, 1);
        }
        return this;
    }
    if (queryObjectOp === null || (typeof queryObjectOp !== 'object' || queryObjectOp instanceof Date)) {
        operator = '$eq';
        value = queryObjectOp;
    } else if (typeof queryObjectOp === 'object') {
        for (key in queryObjectOp) {
            if (hasOwnProperty.call(queryObjectOp, key)) {
                operator = key;
                value = queryObjectOp[key];
                break;
            }
        }
    } else {
        throw new Error('Do not know what you want to do.');
    }
    if (operator === '$regex' || typeof value === 'object') {
        value = precompileQuery(operator, value);
    }
    var usingDotNotation = (property.indexOf('.') !== -1);
    var doIndexCheck = !this.filterInitialized;
    if (doIndexCheck && this.collection.binaryIndices[property] && indexedOps[operator]) {
        this.collection.ensureIndex(property);
        searchByIndex = true;
        index = this.collection.binaryIndices[property];
    }
    var fun = LokiOps[operator];
    var t = this.collection.data;
    var i = 0, len = 0;
    var filter, rowIdx = 0;
    if (this.filterInitialized) {
        filter = this.filteredrows;
        len = filter.length;
        if (usingDotNotation) {
            property = property.split('.');
            for (i = 0; i < len; i++) {
                rowIdx = filter[i];
                if (dotSubScan(t[rowIdx], property, fun, value)) {
                    result.push(rowIdx);
                    if (firstOnly) {
                        this.filteredrows = result;
                        return this;
                    }
                }
            }
        } else {
            for (i = 0; i < len; i++) {
                rowIdx = filter[i];
                if (fun(t[rowIdx][property], value)) {
                    result.push(rowIdx);
                    if (firstOnly) {
                        this.filteredrows = result;
                        return this;
                    }
                }
            }
        }
    } else {
        if (!searchByIndex) {
            len = t.length;
            if (usingDotNotation) {
                property = property.split('.');
                for (i = 0; i < len; i++) {
                    if (dotSubScan(t[i], property, fun, value)) {
                        result.push(i);
                        if (firstOnly) {
                            this.filteredrows = result;
                            this.filterInitialized = true;
                            return this;
                        }
                    }
                }
            } else {
                for (i = 0; i < len; i++) {
                    if (fun(t[i][property], value)) {
                        result.push(i);
                        if (firstOnly) {
                            this.filteredrows = result;
                            this.filterInitialized = true;
                            return this;
                        }
                    }
                }
            }
        } else {
            var segm = this.collection.calculateRange(operator, property, value);
            if (operator !== '$in') {
                for (i = segm[0]; i <= segm[1]; i++) {
                    if (indexedOps[operator] !== true) {
                        if (indexedOps[operator](getIn(t[index.values[i]], property, usingDotNotation), value)) {
                            result.push(index.values[i]);
                            if (firstOnly) {
                                this.filteredrows = result;
                                this.filterInitialized = true;
                                return this;
                            }
                        }
                    } else {
                        result.push(index.values[i]);
                        if (firstOnly) {
                            this.filteredrows = result;
                            this.filterInitialized = true;
                            return this;
                        }
                    }
                }
            } else {
                for (i = 0, len = segm.length; i < len; i++) {
                    result.push(index.values[segm[i]]);
                    if (firstOnly) {
                        this.filteredrows = result;
                        this.filterInitialized = true;
                        return this;
                    }
                }
            }
        }
    }
    this.filteredrows = result;
    this.filterInitialized = true;
    return this;
};

// simplesort as dbFindEx uses it (no options object; the index-intersect path orders by the index, which equals a
// stable sort by the property for lazy indices)
Resultset.prototype.simplesort = function (propname, desc) {
    var options = desc ? { desc: true } : { desc: false };
    var frl = this.filteredrows.length;
    var hasBinaryIndex = hasOwnProperty.call(this.collection.binaryIndices, propname);
    if (frl === 0) {
        if (this.filterInitialized) {
            return this;
        }
        if (hasBinaryIndex) {
            this.collection.ensureIndex(propname);
            this.filteredrows = this.collection.binaryIndices[propname].values.slice(0);
            if (options.desc) {
                this.filteredrows.reverse();
            }
            return this;
        }
        this.filteredrows = this.collection.prepareFullDocIndex();
    } else if (hasBinaryIndex) {
        var eff = this.collection.data.length / frl;
        if (eff <= 10) {
            var io = {}, fr = this.filteredrows;
            for (var idx = 0; idx < frl; idx++) {
                io[fr[idx]] = true;
            }
            this.collection.ensureIndex(propname);
            var pv = this.collection.binaryIndices[propname].values;
            this.filteredrows = pv.filter(function (n) { return io[n]; });
            if (options.desc) {
                this.filteredrows.reverse();
            }
            return this;
        }
    }
    var usingDotNotation = (propname.indexOf('.') !== -1);
    var data = this.collection.data;
    this.filteredrows.sort(function (a, b) {
        var val1, val2;
        if (usingDotNotation) {
            val1 = getIn(data[a], propname, true);
            val2 = getIn(data[b], propname, true);
        } else {
            val1 = data[a][propname];
            val2 = data[b][propname];
        }
        return sortHelper(val1, val2, options.desc);
    });
    return this;
};

Resultset.prototype.offset = function (pos) {
    if (!this.filterInitialized && this.filteredrows.length === 0) {
        this.filteredrows = this.collection.prepareFullDocIndex();
    }
    var rscopy = new Resultset(this.collection);
    rscopy.filteredrows = this.filteredrows.slice(pos);
    rscopy.filterInitialized = true;
    return rscopy;
};

Resultset.prototype.limit = function (qty) {
    if (!this.filterInitialized && this.filteredrows.length === 0) {
        this.filteredrows = this.collection.prepareFullDocIndex();
    }
    var rscopy = new Resultset(this.collection);
    rscopy.filteredrows = this.filteredrows.slice(0, qty);
    rscopy.filterInitialized = true;
    return rscopy;
};

Resultset.prototype.data = function () {
    if (!this.filterInitialized) {
        if (this.filteredrows.length === 0) {
            return this.collection.data.slice();
        }
        this.filterInitialized = true;
    }
    var data = this.collection.data, fr = this.filteredrows, result = [];
    for (var i = 0; i < fr.length; i++) {
        result.push(data[fr[i]]);
    }
    return result;
};

Collection.prototype.chain = function () {
    return new Resultset(this);
};
Collection.prototype.find = function (query) {
    return this.chain().find(query).data();
};
Collection.prototype.findOne = function (query) {
    query = query || {};
    var result = this.chain().find(query, true).data();
    if (Array.isArray(result) && result.length === 0) {
        return null;
    }
    return result[0];
};
Collection.prototype.count = function (query) {
    if (!query) {
        return this.data.length;
    }
    return this.chain().find(query).filteredrows.length;
};
// the unique `_id` index: a plain-object key map
Collection.prototype.by = function (field, value) {
    if (value === undefined) {
        var self = this;
        return function (value) {
            return self.by(field, value);
        };
    }
    if (field !== '_id') {
        throw new TypeError("Cannot read properties of undefined (reading 'get')");
    }
    var key = String(value);
    for (var i = this.data.length - 1; i >= 0; i--) {
        if (this.data[i]._id !== undefined && this.data[i]._id !== null && String(this.data[i]._id) === key) {
            return this.data[i];
        }
    }
    return undefined;
};

// ---- the storage process (db.js) ----

function updateDocument(doc, update) {
    if (update.$set) {
        _.extend(doc, update.$set);
    }
    if (update.$merge) {
        _.merge(doc, update.$merge);
    }
    if (update.$inc) {
        _.forEach(update.$inc, (val, key) => doc[key] = (doc[key] || 0) + val);
    }
    if (update.$unset) {
        for (var j in update.$unset) {
            delete doc[j];
        }
    }
    if (update.$addToSet) {
        for (let i in update.$addToSet) {
            if (!doc[i]) {
                doc[i] = [];
            }
            if (doc[i].indexOf(update.$addToSet[i]) == -1) {
                doc[i].push(update.$addToSet[i]);
            }
        }
    }
    if (update.$pull) {
        for (let i in update.$pull) {
            if (!doc[i]) {
                continue;
            }
            var idx = doc[i].indexOf(update.$pull[i]);
            if (idx != -1) {
                doc[i].splice(idx, 1);
            }
        }
    }
}

function recursReplaceNeNull(val) {
    if (!_.isObject(val)) {
        return;
    }
    for (var i in val) {
        if (_.isEqual(val[i], { $ne: null }) && !val.$and) {
            val.$and = [{ [i]: { $ne: null } }, { [i]: { $ne: undefined } }];
            delete val[i];
        }
        if (_.isEqual(val[i], { $eq: null }) && !val.$or) {
            val.$or = [{ [i]: { $eq: null } }, { [i]: { $eq: undefined } }];
            delete val[i];
        }
        recursReplaceNeNull(val[i]);
    }
}

// One storage request: the collection as the world holds it now, the writes it makes, the result.
function open(name) {
    var text = natives.docs(name);
    return new Collection(name, text === undefined ? [] : JSON.parse(text));
}

function commit(name, writes) {
    if (!writes.length) {
        return [];
    }
    return JSON.parse(natives.write(name, JSON.stringify(writes)));
}

// The RPC hop: arguments and results as JSON, an error as its message (a rejection).
function rpc(fn) {
    return function () {
        var args;
        try {
            args = JSON.parse(JSON.stringify(Array.prototype.slice.call(arguments)));
            var text = JSON.stringify(fn.apply(null, args));
            return Promise.resolve(text === undefined ? undefined : JSON.parse(text));
        } catch (e) {
            return Promise.reject(e && e.message !== undefined ? e.message : e);
        }
    };
}

function dbRequest(collectionName, method, argsArray) {
    var collection = open(collectionName);
    if (method == 'find' || method == 'findOne' || method == 'count' || method == 'removeWhere') {
        recursReplaceNeNull(argsArray[0]);
    }
    if (method == 'insert') {
        // insertOne per doc: the unique `_id` index refuses a key it holds, after the docs before it went in
        var docs = _.isArray(argsArray[0]) ? argsArray[0] : [argsArray[0]];
        var ids = {}, ok = [], dup = null;
        collection.data.forEach(d => { if (d._id !== undefined && d._id !== null) ids[d._id] = true; });
        for (var k = 0; k < docs.length && dup === null; k++) {
            if (typeof docs[k] !== 'object' || docs[k] === null) {
                commit(collectionName, ok.map(d => ({ op: 'insert', doc: d })));
                throw new TypeError('Document needs to be an object');
            }
            if (docs[k]._id !== undefined && docs[k]._id !== null && ids[docs[k]._id]) {
                dup = docs[k]._id;
            } else {
                if (docs[k]._id !== undefined && docs[k]._id !== null) ids[docs[k]._id] = true;
                ok.push(docs[k]);
            }
        }
        var inserted = commit(collectionName, ok.map(d => ({ op: 'insert', doc: d })));
        if (dup !== null) {
            throw new Error('Duplicate key for property _id: ' + dup);
        }
        return _.isArray(argsArray[0]) ? (inserted.length === 1 ? inserted[0] : inserted) : inserted[0];
    }
    if (method == 'removeWhere') {
        var found = collection.find(argsArray[0]);
        commit(collectionName, found.map(d => ({ op: 'remove', id: d._id })));
        return undefined;
    }
    if (method == 'clear') {
        commit(collectionName, collection.data.map(d => ({ op: 'remove', id: d._id })));
        return undefined;
    }
    if (method == 'ensureIndex') {
        return undefined;
    }
    return collection[method].apply(collection, argsArray);
}

function dbUpdate(collectionName, query, update, params) {
    recursReplaceNeNull(query);
    var collection = open(collectionName);
    var result = [];
    if (Object.keys(query).length == 1 && query._id && _.isString(query._id)) {
        var found = collection.by('_id', query._id);
        if (found) {
            result = [found];
        }
    } else {
        result = collection.find(query);
    }
    if (result.length) {
        // Loki's docs are the stored objects: what updateDocument did before a throw stays
        var writes = [];
        try {
            result.forEach(doc => {
                writes.push({ op: 'replace', id: doc._id, doc });
                updateDocument(doc, update);
            });
        } finally {
            commit(collectionName, writes);
        }
        return { modified: result.length };
    } else if (params && params.upsert) {
        var item = {};
        if (query.$and) {
            query.$and.forEach(i => _.extend(item, i));
        } else {
            _.extend(item, query);
        }
        updateDocument(item, update);
        commit(collectionName, [{ op: 'insert', doc: item }]);
        return { inserted: 1 };
    } else {
        return {};
    }
}

function dbBulk(collectionName, bulk) {
    var collection = open(collectionName), result, writes = [];
    try {
    bulk.forEach(i => {
        switch (i.op) {
            case 'update': {
                result = collection.by('_id', i.id);
                if (result) {
                    writes.push({ op: 'replace', id: result._id, doc: result });
                    updateDocument(result, i.update);
                }
                break;
            }
            case 'insert': {
                writes.push({ op: 'insert', doc: i.data });
                break;
            }
            case 'remove': {
                result = collection.by('_id', i.id);
                if (result) {
                    writes.push({ op: 'remove', id: result._id });
                }
                break;
            }
        }
    });
    } finally {
        commit(collectionName, writes);
    }
}

function dbFindEx(collectionName, query, opts) {
    recursReplaceNeNull(query);
    var collection = open(collectionName);
    var chain = collection.chain().find(query);
    if (opts.sort) {
        for (var field in opts.sort) {
            chain = chain.simplesort(field, opts.sort[field] == -1);
        }
    }
    if (opts.offset) {
        chain = chain.offset(opts.offset);
    }
    if (opts.limit) {
        chain = chain.limit(opts.limit);
    }
    return chain.data();
}

// ---- common/lib/storage.js ----

exports.db = {};
exports.queue = {};
exports.env = {
    keys: {
        ACCESSIBLE_ROOMS: 'accessibleRooms',
        ROOM_STATUS_DATA: 'roomStatusData',
        MEMORY: 'memory:',
        GAMETIME: 'gameTime',
        MAP_VIEW: 'mapView:',
        TERRAIN_DATA: 'terrainData',
        SCRIPT_CACHED_DATA: 'scriptCachedData:',
        USER_ONLINE: 'userOnline:',
        MAIN_LOOP_PAUSED: 'mainLoopPaused',
        ROOM_HISTORY: 'roomHistory:',
        ROOM_VISUAL: 'roomVisual:',
        MEMORY_SEGMENTS: 'memorySegments:',
        PUBLIC_MEMORY_SEGMENTS: 'publicMemorySegments:',
        ROOM_EVENT_LOG: 'roomEventLog:',
        ACTIVE_ROOMS: 'activeRooms',
        MAIN_LOOP_MIN_DURATION: 'tickRate'
    }
};
exports.pubsub = {
    keys: {
        QUEUE_DONE: 'queueDone:',
        RUNTIME_RESTART: 'runtimeRestart',
        TICK_STARTED: "tickStarted",
        ROOMS_DONE: "roomsDone"
    }
};

function wrapCollection(collectionName) {
    var wrap = {};
    ['find', 'findOne', 'by', 'clear', 'count', 'ensureIndex', 'removeWhere', 'insert'].forEach(method => {
        wrap[method] = function () {
            return rpc(dbRequest).call(null, collectionName, method, Array.prototype.slice.call(arguments));
        };
    });
    wrap.update = function (query, update, params) {
        return rpc(dbUpdate).call(null, collectionName, query, update, params);
    };
    wrap.bulk = function (bulk) {
        return rpc(dbBulk).call(null, collectionName, bulk);
    };
    wrap.findEx = function (query, opts) {
        return rpc(dbFindEx).call(null, collectionName, query, opts);
    };
    return wrap;
}

dbCollections.forEach(i => exports.db[i] = wrapCollection(i));

exports.resetAllData = () => rpc(() => JSON.parse(natives.call('resetAllData', '[]')))();

// env and pubsub: the host's (world keys apply to the world, others live in the server's memory)
['get', 'mget', 'set', 'setex', 'expire', 'ttl', 'del', 'hmget', 'hmset', 'hget', 'hset', 'sadd', 'smembers'].forEach(op => {
    exports.env[op] = function () {
        return rpc(function () {
            var out = natives.env(op, JSON.stringify(Array.prototype.slice.call(arguments)));
            return out === undefined ? undefined : JSON.parse(out);
        }).apply(null, arguments);
    };
});

Object.assign(exports.pubsub, {
    publish: rpc(function (channel, data) {
        natives.call('publish', JSON.stringify([channel, data]));
    }),
    // ponytail: messages from the server are not delivered to CLI subscribers (no event loop in a CLI session)
    subscribe(channel, cb) {}
});

// what the oracle and tests drive directly
exports._loki = { Collection: Collection, updateDocument: updateDocument, recursReplaceNeNull: recursReplaceNeNull };
