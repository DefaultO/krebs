// mirror: reference/storage/lib/db.js updateDocument (257-293) and dbBulk (480-513),
// over a plain array of docs instead of a Loki collection. Callers pass ops after a JSON
// round trip (the storage RPC). Loki behaviour kept: `by('_id', id)` matches String(id);
// `by('_id', undefined)` returns a function, which makes Loki's update/remove throw and
// aborts the rest of the bulk; so does inserting a second document with the same `_id`.
// Inserts get `_id` only through `opts.genId`.
'use strict';

const { reference } = require('./common');
const _ = require(reference('lodash-3.10.1/lodash.js'));

function updateDocument(doc, update) {
  if (update.$set) {
    _.extend(doc, update.$set);
  }
  if (update.$merge) {
    _.merge(doc, update.$merge);
  }
  if (update.$inc) {
    _.forEach(update.$inc, (val, key) => (doc[key] = (doc[key] || 0) + val));
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

function by(docs, id) {
  if (id === undefined) throw new Error('loki: by(_id, undefined) is not a document');
  return docs.find((d) => String(d._id) === String(id));
}

// Applies `ops` to `docs` in order. Returns null, or the error message that aborted the rest.
function dbBulk(docs, ops, opts = {}) {
  try {
    for (const i of ops) {
      switch (i.op) {
        case 'update': {
          const result = by(docs, i.id);
          if (result) updateDocument(result, i.update);
          break;
        }
        case 'insert': {
          if (opts.genId && !i.data._id) i.data._id = opts.genId(i.data);
          // Loki's unique `_id` index (UniqueIndex.set) refuses a second document.
          const id = i.data._id;
          if (id !== null && id !== undefined && by(docs, id)) throw new Error(`Duplicate key for property _id: ${id}`);
          docs.push(i.data);
          break;
        }
        case 'remove': {
          const result = by(docs, i.id);
          if (result) docs.splice(docs.indexOf(result), 1);
          break;
        }
      }
    }
    return null;
  } catch (e) {
    return e.message;
  }
}

module.exports = { updateDocument, dbBulk };
