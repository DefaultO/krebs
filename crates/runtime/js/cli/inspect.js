// Derived from Node.js (MIT), see THIRD_PARTY_NOTICES.md.
// not a mirror: node's util.inspect (lib/internal/util/inspect.js, node 22) for the CLI's output (CLI-OUTPUT): the
// default options (depth 2, breakLength 80, compact 3, maxArrayLength 100, maxStringLength 10000, no colors),
// ported for the values a CLI session holds: primitives, plain and class objects, arrays (grouping included),
// functions, Date, RegExp, Error, Map, Set, Promise, boxed primitives, circular references. Checked against
// node's own util.inspect by tools/oracle/cli-inspect.js.
// ponytail: no Proxy/TypedArray/ArrayBuffer/iterator/getter special cases; non-ASCII widths use the non-ICU
// table (node with ICU measures some emoji differently when grouping arrays).
'use strict';

var kObjectType = 0;
var kArrayType = 1;
var kArrayExtrasType = 2;

var strEscapeSequencesRegExp = /[\x00-\x1f\x27\x5c\x7f-\x9f]|[\ud800-\udbff](?![\udc00-\udfff])|(?<![\ud800-\udbff])[\udc00-\udfff]/;
var strEscapeSequencesReplacer = /[\x00-\x1f\x27\x5c\x7f-\x9f]|[\ud800-\udbff](?![\udc00-\udfff])|(?<![\ud800-\udbff])[\udc00-\udfff]/g;
var strEscapeSequencesRegExpSingle = /[\x00-\x1f\x5c\x7f-\x9f]|[\ud800-\udbff](?![\udc00-\udfff])|(?<![\ud800-\udbff])[\udc00-\udfff]/;
var strEscapeSequencesReplacerSingle = /[\x00-\x1f\x5c\x7f-\x9f]|[\ud800-\udbff](?![\udc00-\udfff])|(?<![\ud800-\udbff])[\udc00-\udfff]/g;
var keyStrRegExp = /^[a-zA-Z_][a-zA-Z_0-9]*$/;
var numberRegExp = /^(0|[1-9][0-9]*)$/;
var classRegExp = /^(\s+[^(]*?)\s*{/;
var stripCommentsRegExp = /(\/\/.*?\n)|(\/\*(.|\n)*?\*\/)/g;
var kMinLineLength = 16;

var meta = [
    '\\x00', '\\x01', '\\x02', '\\x03', '\\x04', '\\x05', '\\x06', '\\x07',
    '\\b', '\\t', '\\n', '\\x0B', '\\f', '\\r', '\\x0E', '\\x0F',
    '\\x10', '\\x11', '\\x12', '\\x13', '\\x14', '\\x15', '\\x16', '\\x17',
    '\\x18', '\\x19', '\\x1A', '\\x1B', '\\x1C', '\\x1D', '\\x1E', '\\x1F',
    '', '', '', '', '', '', '', "\\'", '', '', '', '', '', '', '', '',
    '', '', '', '', '', '', '', '', '', '', '', '', '', '', '', '',
    '', '', '', '', '', '', '', '', '', '', '', '', '', '', '', '',
    '', '', '', '', '', '', '', '', '', '', '', '', '\\\\', '', '', '',
    '', '', '', '', '', '', '', '', '', '', '', '', '', '', '', '',
    '', '', '', '', '', '', '', '', '', '', '', '', '', '', '', '\\x7F',
    '\\x80', '\\x81', '\\x82', '\\x83', '\\x84', '\\x85', '\\x86', '\\x87',
    '\\x88', '\\x89', '\\x8A', '\\x8B', '\\x8C', '\\x8D', '\\x8E', '\\x8F',
    '\\x90', '\\x91', '\\x92', '\\x93', '\\x94', '\\x95', '\\x96', '\\x97',
    '\\x98', '\\x99', '\\x9A', '\\x9B', '\\x9C', '\\x9D', '\\x9E', '\\x9F',
];

var toStr = Object.prototype.toString;
function isDate(v) { return toStr.call(v) === '[object Date]'; }
function isRegExp(v) { return toStr.call(v) === '[object RegExp]'; }
function isError(v) { return v instanceof Error || toStr.call(v) === '[object Error]'; }
function isMap(v) { return v instanceof Map; }
function isSet(v) { return v instanceof Set; }
function isPromise(v) { return v instanceof Promise; }
function isArguments(v) { return toStr.call(v) === '[object Arguments]'; }
function isBoxedPrimitive(v) {
    return v instanceof Number || v instanceof String || v instanceof Boolean ||
        (typeof Symbol === 'function' && v instanceof Symbol) || (typeof BigInt === 'function' && v instanceof BigInt);
}

function inspect(value) {
    var ctx = {
        budget: {},
        indentationLvl: 0,
        seen: [],
        currentDepth: 0,
        depth: 2,
        maxArrayLength: 100,
        maxStringLength: 10000,
        breakLength: 80,
        compact: 3,
    };
    return formatValue(ctx, value, 0);
}

function addQuotes(str, quotes) {
    if (quotes === -1) {
        return '"' + str + '"';
    }
    if (quotes === -2) {
        return '`' + str + '`';
    }
    return "'" + str + "'";
}

function escapeFn(str) {
    var charCode = str.charCodeAt(0);
    return meta.length > charCode ? meta[charCode] : '\\u' + charCode.toString(16);
}

function strEscape(str) {
    var escapeTest = strEscapeSequencesRegExp;
    var escapeReplace = strEscapeSequencesReplacer;
    var singleQuote = 39;
    if (str.includes("'")) {
        if (!str.includes('"')) {
            singleQuote = -1;
        } else if (!str.includes('`') && !str.includes('${')) {
            singleQuote = -2;
        }
        if (singleQuote !== 39) {
            escapeTest = strEscapeSequencesRegExpSingle;
            escapeReplace = strEscapeSequencesReplacerSingle;
        }
    }
    if (str.length < 5000 && escapeTest.exec(str) === null)
        return addQuotes(str, singleQuote);
    if (str.length > 100) {
        str = str.replace(escapeReplace, escapeFn);
        return addQuotes(str, singleQuote);
    }
    var result = '';
    var last = 0;
    for (var i = 0; i < str.length; i++) {
        var point = str.charCodeAt(i);
        if (point === singleQuote || point === 92 || point < 32 || (point > 126 && point < 160)) {
            if (last === i) {
                result += meta[point];
            } else {
                result += str.slice(last, i) + meta[point];
            }
            last = i + 1;
        } else if (point >= 0xd800 && point <= 0xdfff) {
            if (point <= 0xdbff && i + 1 < str.length) {
                var next = str.charCodeAt(i + 1);
                if (next >= 0xdc00 && next <= 0xdfff) {
                    i++;
                    continue;
                }
            }
            result += str.slice(last, i) + '\\u' + point.toString(16);
            last = i + 1;
        }
    }
    if (last !== str.length) {
        result += str.slice(last);
    }
    return addQuotes(result, singleQuote);
}

function isFullWidthCodePoint(code) {
    return code >= 0x1100 && (
        code <= 0x115f || code === 0x2329 || code === 0x232a ||
        (code >= 0x2e80 && code <= 0x3247 && code !== 0x303f) ||
        (code >= 0x3250 && code <= 0x4dbf) ||
        (code >= 0x4e00 && code <= 0xa4c6) ||
        (code >= 0xa960 && code <= 0xa97c) ||
        (code >= 0xac00 && code <= 0xd7a3) ||
        (code >= 0xf900 && code <= 0xfaff) ||
        (code >= 0xfe10 && code <= 0xfe19) ||
        (code >= 0xfe30 && code <= 0xfe6b) ||
        (code >= 0xff01 && code <= 0xff60) ||
        (code >= 0xffe0 && code <= 0xffe6) ||
        (code >= 0x1b000 && code <= 0x1b001) ||
        (code >= 0x1f200 && code <= 0x1f251) ||
        (code >= 0x1f300 && code <= 0x1f64f) ||
        (code >= 0x20000 && code <= 0x3fffd));
}

function isZeroWidthCodePoint(code) {
    return code <= 0x1F || (code >= 0x7F && code <= 0x9F) || (code >= 0x300 && code <= 0x36F) ||
        (code >= 0x200B && code <= 0x200F) || (code >= 0x20D0 && code <= 0x20FF) ||
        (code >= 0xFE00 && code <= 0xFE0F) || (code >= 0xFE20 && code <= 0xFE2F) ||
        (code >= 0xE0100 && code <= 0xE01EF);
}

function getStringWidth(str) {
    var width = 0;
    for (var i = 0; i < str.length; i++) {
        var code = str.charCodeAt(i);
        if (code >= 127) {
            for (var ch of str.slice(i).normalize('NFC')) {
                var cp = ch.codePointAt(0);
                if (isFullWidthCodePoint(cp)) {
                    width += 2;
                } else if (!isZeroWidthCodePoint(cp)) {
                    width++;
                }
            }
            break;
        }
        width += code >= 32 ? 1 : 0;
    }
    return width;
}

function getConstructorName(obj) {
    var tmp = obj;
    var firstProto;
    while (obj) {
        var descriptor = Object.getOwnPropertyDescriptor(obj, 'constructor');
        if (descriptor !== undefined && typeof descriptor.value === 'function' && descriptor.value.name !== '' &&
            tmp instanceof descriptor.value) {
            return String(descriptor.value.name);
        }
        obj = Object.getPrototypeOf(obj);
        if (firstProto === undefined) {
            firstProto = obj;
        }
    }
    if (firstProto === null) {
        return null;
    }
    // ponytail: node names "Foo <Bar>" from V8's class name here; such objects are rare in a CLI
    return 'Object';
}

function getPrefix(constructor, tag, fallback, size) {
    size = size || '';
    if (constructor === null) {
        if (tag !== '' && fallback !== tag) {
            return '[' + fallback + size + ': null prototype] [' + tag + '] ';
        }
        return '[' + fallback + size + ': null prototype] ';
    }
    var result = constructor + size + ' ';
    if (tag !== '') {
        var position = constructor.indexOf(tag);
        if (position === -1) {
            result += '[' + tag + '] ';
        } else {
            var endPos = position + tag.length;
            if (endPos !== constructor.length && constructor[endPos] === constructor[endPos].toLowerCase()) {
                result += '[' + tag + '] ';
            }
        }
    }
    return result;
}

function getKeys(value) {
    var keys = Object.keys(value);
    var symbols = Object.getOwnPropertySymbols(value);
    if (symbols.length !== 0) {
        symbols.forEach(function (s) {
            if (Object.prototype.propertyIsEnumerable.call(value, s)) keys.push(s);
        });
    }
    return keys;
}

function getCtxStyle(value, constructor, tag) {
    var fallback = '';
    if (constructor === null) {
        fallback = toStr.call(value).slice(8, -1);
        if (fallback === tag) {
            fallback = 'Object';
        }
    }
    return getPrefix(constructor, tag, fallback);
}

function formatValue(ctx, value, recurseTimes) {
    if (typeof value !== 'object' && typeof value !== 'function') {
        return formatPrimitive(value, ctx);
    }
    if (value === null) {
        return 'null';
    }
    if (ctx.seen.includes(value)) {
        var index = 1;
        if (ctx.circular === undefined) {
            ctx.circular = new Map();
            ctx.circular.set(value, index);
        } else {
            index = ctx.circular.get(value);
            if (index === undefined) {
                index = ctx.circular.size + 1;
                ctx.circular.set(value, index);
            }
        }
        return '[Circular *' + index + ']';
    }
    return formatRaw(ctx, value, recurseTimes);
}

function remainingText(remaining) {
    return '... ' + remaining + ' more item' + (remaining > 1 ? 's' : '');
}

function formatRaw(ctx, value, recurseTimes) {
    var keys;
    var constructor = getConstructorName(value);
    var tag = value[Symbol.toStringTag];
    if (typeof tag !== 'string' || (tag !== '' && Object.prototype.propertyIsEnumerable.call(value, Symbol.toStringTag))) {
        tag = '';
    }
    var base = '';
    var formatter = function () { return []; };
    var braces;
    var noIterator = true;
    var extrasType = kObjectType;
    if (Symbol.iterator in value || constructor === null) {
        noIterator = false;
        if (Array.isArray(value)) {
            var prefix = (constructor !== 'Array' || tag !== '') ? getPrefix(constructor, tag, 'Array', '(' + value.length + ')') : '';
            keys = Object.keys(value).filter(function (k) { return !numberRegExp.test(k) || +k >= 2 ** 32 - 1; });
            braces = [prefix + '[', ']'];
            if (value.length === 0 && keys.length === 0)
                return braces[0] + ']';
            extrasType = kArrayExtrasType;
            formatter = formatArray;
        } else if (isSet(value)) {
            var sizeS = value.size;
            var prefixS = getPrefix(constructor, tag, 'Set', '(' + sizeS + ')');
            keys = getKeys(value);
            formatter = formatSet.bind(null, value);
            if (sizeS === 0 && keys.length === 0)
                return prefixS + '{}';
            braces = [prefixS + '{', '}'];
        } else if (isMap(value)) {
            var sizeM = value.size;
            var prefixM = getPrefix(constructor, tag, 'Map', '(' + sizeM + ')');
            keys = getKeys(value);
            formatter = formatMap.bind(null, value);
            if (sizeM === 0 && keys.length === 0)
                return prefixM + '{}';
            braces = [prefixM + '{', '}'];
        } else {
            noIterator = true;
        }
    }
    if (noIterator) {
        keys = getKeys(value);
        braces = ['{', '}'];
        if (typeof value === 'function') {
            base = getFunctionBase(value, constructor, tag);
            if (keys.length === 0)
                return base;
        } else if (constructor === 'Object') {
            if (isArguments(value)) {
                braces[0] = '[Arguments] {';
            } else if (tag !== '') {
                braces[0] = getPrefix(constructor, tag, 'Object') + '{';
            }
            if (keys.length === 0) {
                return braces[0] + '}';
            }
        } else if (isRegExp(value)) {
            base = RegExp.prototype.toString.call(constructor !== null ? value : new RegExp(value));
            var prefixR = getPrefix(constructor, tag, 'RegExp');
            if (prefixR !== 'RegExp ')
                base = prefixR + base;
            if (keys.length === 0 || (recurseTimes > ctx.depth && ctx.depth !== null)) {
                return base;
            }
        } else if (isDate(value)) {
            base = Number.isNaN(Date.prototype.getTime.call(value)) ? Date.prototype.toString.call(value) : Date.prototype.toISOString.call(value);
            var prefixD = getPrefix(constructor, tag, 'Date');
            if (prefixD !== 'Date ')
                base = prefixD + base;
            if (keys.length === 0) {
                return base;
            }
        } else if (isError(value)) {
            base = formatError(value, constructor, tag, ctx, keys);
            if (keys.length === 0)
                return base;
        } else if (isPromise(value)) {
            braces[0] = getPrefix(constructor, tag, 'Promise') + '{';
            // ponytail: V8 does not tell plain script a promise's state; every promise shows as pending
            formatter = function () { return ['<pending>']; };
        } else if (isBoxedPrimitive(value)) {
            base = getBoxedBase(value, ctx, keys, constructor, tag);
            if (keys.length === 0) {
                return base;
            }
        } else {
            if (keys.length === 0) {
                return getCtxStyle(value, constructor, tag) + '{}';
            }
            braces[0] = getCtxStyle(value, constructor, tag) + '{';
        }
    }
    if (recurseTimes > ctx.depth && ctx.depth !== null) {
        var constructorName = getCtxStyle(value, constructor, tag).slice(0, -1);
        if (constructor !== null)
            constructorName = '[' + constructorName + ']';
        return constructorName;
    }
    recurseTimes += 1;
    ctx.seen.push(value);
    ctx.currentDepth = recurseTimes;
    var output = formatter(ctx, value, recurseTimes);
    for (var i = 0; i < keys.length; i++) {
        output.push(formatProperty(ctx, value, recurseTimes, keys[i], extrasType));
    }
    if (ctx.circular !== undefined) {
        var idx = ctx.circular.get(value);
        if (idx !== undefined) {
            var reference = '<ref *' + idx + '>';
            base = base === '' ? reference : reference + ' ' + base;
        }
    }
    ctx.seen.pop();
    var res = reduceToSingleString(ctx, output, base, braces, extrasType, recurseTimes, value);
    var budget = ctx.budget[ctx.indentationLvl] || 0;
    var newLength = budget + res.length;
    ctx.budget[ctx.indentationLvl] = newLength;
    if (newLength > 2 ** 27) {
        ctx.depth = -1;
    }
    return res;
}

function getBoxedBase(value, ctx, keys, constructor, tag) {
    var fn, type;
    if (value instanceof Number) {
        fn = Number.prototype.valueOf;
        type = 'Number';
    } else if (value instanceof String) {
        fn = String.prototype.valueOf;
        type = 'String';
        keys.splice(0, value.length);
    } else if (value instanceof Boolean) {
        fn = Boolean.prototype.valueOf;
        type = 'Boolean';
    } else if (typeof BigInt === 'function' && value instanceof BigInt) {
        fn = BigInt.prototype.valueOf;
        type = 'BigInt';
    } else {
        fn = Symbol.prototype.valueOf;
        type = 'Symbol';
    }
    var base = '[' + type;
    if (type !== constructor) {
        if (constructor === null) {
            base += ' (null prototype)';
        } else {
            base += ' (' + constructor + ')';
        }
    }
    base += ': ' + formatPrimitive(fn.call(value), ctx) + ']';
    if (tag !== '' && tag !== constructor) {
        base += ' [' + tag + ']';
    }
    return base;
}

function getClassBase(value, constructor, tag) {
    var hasName = Object.prototype.hasOwnProperty.call(value, 'name');
    var name = (hasName && value.name) || '(anonymous)';
    var base = 'class ' + name;
    if (constructor !== 'Function' && constructor !== null) {
        base += ' [' + constructor + ']';
    }
    if (tag !== '' && constructor !== tag) {
        base += ' [' + tag + ']';
    }
    if (constructor !== null) {
        var superName = Object.getPrototypeOf(value).name;
        if (superName) {
            base += ' extends ' + superName;
        }
    } else {
        base += ' extends [null prototype]';
    }
    return '[' + base + ']';
}

function getFunctionBase(value, constructor, tag) {
    var stringified = Function.prototype.toString.call(value);
    if (stringified.startsWith('class') && stringified[stringified.length - 1] === '}') {
        var slice = stringified.slice(5, -1);
        var bracketIndex = slice.indexOf('{');
        if (bracketIndex !== -1 &&
            (!slice.slice(0, bracketIndex).includes('(') ||
            classRegExp.exec(slice.replace(stripCommentsRegExp, '')) !== null)) {
            return getClassBase(value, constructor, tag);
        }
    }
    var type = 'Function';
    var kind = toStr.call(value);
    if (kind === '[object GeneratorFunction]' || kind === '[object AsyncGeneratorFunction]') {
        type = 'Generator' + type;
    }
    if (kind === '[object AsyncFunction]' || kind === '[object AsyncGeneratorFunction]') {
        type = 'Async' + type;
    }
    var base = '[' + type;
    if (constructor === null) {
        base += ' (null prototype)';
    }
    if (value.name === '') {
        base += ' (anonymous)';
    } else {
        base += ': ' + (typeof value.name === 'string' ? value.name : formatValue({ seen: [], depth: 2, indentationLvl: 0, budget: {}, currentDepth: 0, maxArrayLength: 100, maxStringLength: 10000, breakLength: 80, compact: 3 }, value.name, 0));
    }
    base += ']';
    if (constructor !== type && constructor !== null) {
        base += ' ' + constructor;
    }
    if (tag !== '' && constructor !== tag) {
        base += ' [' + tag + ']';
    }
    return base;
}

function formatError(err, constructor, tag, ctx, keys) {
    var name = err.name != null ? err.name : 'Error';
    var stack = err.stack ? (typeof err.stack === 'string' ? err.stack : Error.prototype.toString.call(err)) : Error.prototype.toString.call(err);
    // removeDuplicateErrorKeys
    ['name', 'message', 'stack'].forEach(function (k) {
        var i = keys.indexOf(k);
        if (i !== -1 && stack.includes(err[k])) {
            keys.splice(i, 1);
        }
    });
    if ('cause' in err && (keys.length === 0 || !keys.includes('cause'))) {
        keys.push('cause');
    }
    var pos = (err.message && stack.indexOf(err.message)) || -1;
    if (pos !== -1)
        pos += err.message.length;
    var stackStart = stack.indexOf('\n    at', pos);
    if (stackStart === -1) {
        stack = '[' + stack + ']';
    }
    if (ctx.indentationLvl !== 0) {
        var indentation = ' '.repeat(ctx.indentationLvl);
        stack = stack.split('\n').join('\n' + indentation);
    }
    void name;
    return stack;
}

function groupArrayElements(ctx, output, value) {
    var totalLength = 0;
    var maxLength = 0;
    var i = 0;
    var outputLength = output.length;
    if (ctx.maxArrayLength < output.length) {
        outputLength--;
    }
    var separatorSpace = 2;
    var dataLen = new Array(outputLength);
    for (; i < outputLength; i++) {
        var len = getStringWidth(output[i]);
        dataLen[i] = len;
        totalLength += len + separatorSpace;
        if (maxLength < len)
            maxLength = len;
    }
    var actualMax = maxLength + separatorSpace;
    if (actualMax * 3 + ctx.indentationLvl < ctx.breakLength &&
        (totalLength / actualMax > 5 || maxLength <= 6)) {
        var approxCharHeights = 2.5;
        var averageBias = Math.sqrt(actualMax - totalLength / output.length);
        var biasedMax = Math.max(actualMax - 3 - averageBias, 1);
        var columns = Math.min(
            Math.round(Math.sqrt(approxCharHeights * biasedMax * outputLength) / biasedMax),
            Math.floor((ctx.breakLength - ctx.indentationLvl) / actualMax),
            ctx.compact * 4,
            15);
        if (columns <= 1) {
            return output;
        }
        var tmp = [];
        var maxLineLength = [];
        for (var c = 0; c < columns; c++) {
            var lineMaxLength = 0;
            for (var j = c; j < output.length; j += columns) {
                if (dataLen[j] > lineMaxLength)
                    lineMaxLength = dataLen[j];
            }
            lineMaxLength += separatorSpace;
            maxLineLength[c] = lineMaxLength;
        }
        var padStart = true;
        if (value !== undefined) {
            for (var k = 0; k < output.length; k++) {
                if (typeof value[k] !== 'number' && typeof value[k] !== 'bigint') {
                    padStart = false;
                    break;
                }
            }
        }
        for (var r = 0; r < outputLength; r += columns) {
            var max = Math.min(r + columns, outputLength);
            var str = '';
            var m = r;
            for (; m < max - 1; m++) {
                var padding = maxLineLength[m - r] + output[m].length - dataLen[m];
                str += padStart ? (output[m] + ', ').padStart(padding, ' ') : (output[m] + ', ').padEnd(padding, ' ');
            }
            if (padStart) {
                var paddingLast = maxLineLength[m - r] + output[m].length - dataLen[m] - separatorSpace;
                str += output[m].padStart(paddingLast, ' ');
            } else {
                str += output[m];
            }
            tmp.push(str);
        }
        if (ctx.maxArrayLength < output.length) {
            tmp.push(output[outputLength]);
        }
        output = tmp;
    }
    return output;
}

function formatNumber(number) {
    if (Object.is(number, -0)) {
        return '-0';
    }
    return '' + number;
}

function formatPrimitive(value, ctx) {
    if (typeof value === 'string') {
        var trailer = '';
        if (value.length > ctx.maxStringLength) {
            var remaining = value.length - ctx.maxStringLength;
            value = value.slice(0, ctx.maxStringLength);
            trailer = '... ' + remaining + ' more character' + (remaining > 1 ? 's' : '');
        }
        if (value.length > kMinLineLength && value.length > ctx.breakLength - ctx.indentationLvl - 4) {
            return value.split(/(?<=\n)/).map(function (line) { return strEscape(line); })
                .join(' +\n' + ' '.repeat(ctx.indentationLvl + 2)) + trailer;
        }
        return strEscape(value) + trailer;
    }
    if (typeof value === 'number')
        return formatNumber(value);
    if (typeof value === 'bigint')
        return String(value) + 'n';
    if (typeof value === 'boolean')
        return '' + value;
    if (typeof value === 'undefined')
        return 'undefined';
    return Symbol.prototype.toString.call(value);
}

function formatSpecialArray(ctx, value, recurseTimes, maxLength, output, i) {
    var keys = Object.keys(value);
    var index = i;
    for (; i < keys.length && output.length < maxLength; i++) {
        var key = keys[i];
        var tmp = +key;
        if (tmp > 2 ** 32 - 2) {
            break;
        }
        if ('' + index !== key) {
            if (numberRegExp.exec(key) === null) {
                break;
            }
            var emptyItems = tmp - index;
            var ending = emptyItems > 1 ? 's' : '';
            output.push('<' + emptyItems + ' empty item' + ending + '>');
            index = tmp;
            if (output.length === maxLength) {
                break;
            }
        }
        output.push(formatProperty(ctx, value, recurseTimes, key, kArrayType));
        index++;
    }
    var remaining = value.length - index;
    if (output.length !== maxLength) {
        if (remaining > 0) {
            output.push('<' + remaining + ' empty item' + (remaining > 1 ? 's' : '') + '>');
        }
    } else if (remaining > 0) {
        output.push(remainingText(remaining));
    }
    return output;
}

function formatArray(ctx, value, recurseTimes) {
    var valLen = value.length;
    var len = Math.min(Math.max(0, ctx.maxArrayLength), valLen);
    var remaining = valLen - len;
    var output = [];
    for (var i = 0; i < len; i++) {
        var desc = Object.getOwnPropertyDescriptor(value, i);
        if (desc === undefined) {
            return formatSpecialArray(ctx, value, recurseTimes, len, output, i);
        }
        output.push(formatProperty(ctx, value, recurseTimes, i, kArrayType, desc));
    }
    if (remaining > 0) {
        output.push(remainingText(remaining));
    }
    return output;
}

function formatSet(value, ctx, ignored, recurseTimes) {
    var length = value.size;
    var maxLength = Math.min(Math.max(0, ctx.maxArrayLength), length);
    var remaining = length - maxLength;
    var output = [];
    ctx.indentationLvl += 2;
    var i = 0;
    for (var v of value) {
        if (i >= maxLength) break;
        output.push(formatValue(ctx, v, recurseTimes));
        i++;
    }
    if (remaining > 0) {
        output.push(remainingText(remaining));
    }
    ctx.indentationLvl -= 2;
    return output;
}

function formatMap(value, ctx, ignored, recurseTimes) {
    var length = value.size;
    var maxLength = Math.min(Math.max(0, ctx.maxArrayLength), length);
    var remaining = length - maxLength;
    var output = [];
    ctx.indentationLvl += 2;
    var i = 0;
    for (var entry of value) {
        if (i >= maxLength) break;
        output.push(formatValue(ctx, entry[0], recurseTimes) + ' => ' + formatValue(ctx, entry[1], recurseTimes));
        i++;
    }
    if (remaining > 0) {
        output.push(remainingText(remaining));
    }
    ctx.indentationLvl -= 2;
    return output;
}

function formatProperty(ctx, value, recurseTimes, key, type, desc) {
    var name, str;
    var extra = ' ';
    desc = desc || Object.getOwnPropertyDescriptor(value, key) || { value: value[key], enumerable: true };
    if (desc.value !== undefined) {
        ctx.indentationLvl += 2;
        str = formatValue(ctx, desc.value, recurseTimes);
        ctx.indentationLvl -= 2;
    } else if (desc.get !== undefined) {
        str = desc.set !== undefined ? '[Getter/Setter]' : '[Getter]';
    } else if (desc.set !== undefined) {
        str = '[Setter]';
    } else {
        str = 'undefined';
    }
    if (type === kArrayType) {
        return str;
    }
    if (typeof key === 'symbol') {
        name = '[' + Symbol.prototype.toString.call(key).replace(strEscapeSequencesReplacer, escapeFn) + ']';
    } else if (key === '__proto__') {
        name = "['__proto__']";
    } else if (desc.enumerable === false) {
        name = '[' + key.replace(strEscapeSequencesReplacer, escapeFn) + ']';
    } else if (keyStrRegExp.exec(key) !== null) {
        name = key;
    } else {
        name = strEscape(key);
    }
    return name + ':' + extra + str;
}

function isBelowBreakLength(ctx, output, start, base) {
    var totalLength = output.length + start;
    if (totalLength + output.length > ctx.breakLength)
        return false;
    for (var i = 0; i < output.length; i++) {
        totalLength += output[i].length;
        if (totalLength > ctx.breakLength) {
            return false;
        }
    }
    return base === '' || !base.includes('\n');
}

function reduceToSingleString(ctx, output, base, braces, extrasType, recurseTimes, value) {
    var entries = output.length;
    if (extrasType === kArrayExtrasType && entries > 6) {
        output = groupArrayElements(ctx, output, value);
    }
    if (ctx.currentDepth - recurseTimes < ctx.compact && entries === output.length) {
        var start = output.length + ctx.indentationLvl + braces[0].length + base.length + 10;
        if (isBelowBreakLength(ctx, output, start, base)) {
            var joinedOutput = output.join(', ');
            if (!joinedOutput.includes('\n')) {
                return (base ? base + ' ' : '') + braces[0] + ' ' + joinedOutput + ' ' + braces[1];
            }
        }
    }
    var indentation = '\n' + ' '.repeat(ctx.indentationLvl);
    return (base ? base + ' ' : '') + braces[0] + indentation + '  ' + output.join(',' + indentation + '  ') + indentation + braces[1];
}

exports.inspect = inspect;
