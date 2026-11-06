//! Market natives: marketOrders, marketOrder, marketHistory as per-version lazy JSON (ARCHITECTURE.md §6.7).
//! Owner: WP08 (wave 2).
//!
//! The JSON is vanilla's runtime data (driver/lib/runtime/data.js:31-75): active `market.orders` grouped by
//! `resourceType` and under `all`, each order's fields minus `_id` plus `id` (appended, as `i.id = ""+i._id;
//! delete i._id`); `market.stats` docs minus `_id` grouped into arrays the same way. Loki's `meta`/`$loki`
//! never exist here (05 MARKET-DATA-SOURCES: chitin does not emit them). Built once per `MarketState`
//! (a new one is installed whenever orders change), shared by every player, ASCII.

use crate::natives::{set_fn, with_run};
use engine::doc::{Obj, Val};
use engine::world::MarketState;
use std::collections::HashMap;
use std::sync::{Arc, OnceLock, RwLock};

/// Indexes of active orders and their lazily built `{id: order}` JSON.
type Group = (Vec<usize>, OnceLock<Arc<str>>);

/// One `MarketState` version's lazily built JSON.
struct Cache {
    market: Arc<MarketState>,
    /// Active orders' indexes per group key (`all` included), creation order.
    groups: HashMap<Box<str>, Group>,
    /// Active order id → index.
    by_id: HashMap<Box<str>, usize>,
    /// Per active order index: its JSON.
    order_json: Vec<OnceLock<Box<str>>>,
    history: OnceLock<HashMap<Box<str>, Arc<str>>>,
}

static CACHE: RwLock<Option<Arc<Cache>>> = RwLock::new(None);

fn cache() -> Option<Arc<Cache>> {
    let market = with_run(|c| c.world().market().clone())?;
    if let Some(c) = CACHE.read().unwrap_or_else(|e| e.into_inner()).as_ref().filter(|c| Arc::ptr_eq(&c.market, &market)) {
        return Some(c.clone());
    }
    let mut slot = CACHE.write().unwrap_or_else(|e| e.into_inner());
    if let Some(c) = slot.as_ref().filter(|c| Arc::ptr_eq(&c.market, &market)) {
        return Some(c.clone());
    }
    let mut groups: HashMap<Box<str>, Group> = HashMap::new();
    groups.insert("all".into(), (Vec::new(), OnceLock::new()));
    let mut by_id = HashMap::new();
    for (i, doc) in market.orders.iter().enumerate() {
        if !matches!(field(&doc.obj, "active"), Some(Val::Bool(true))) {
            continue;
        }
        let rt = match field(&doc.obj, "resourceType") {
            Some(Val::Str(s)) => s.to_string(),
            Some(v) => js_key(v),
            None => "undefined".into(),
        };
        groups.entry(rt.into()).or_default().0.push(i);
        groups.get_mut("all").unwrap().0.push(i);
        by_id.insert(doc.id().as_str().into(), i);
    }
    let order_json = market.orders.iter().map(|_| OnceLock::new()).collect();
    let c = Arc::new(Cache { market, groups, by_id, order_json, history: OnceLock::new() });
    *slot = Some(c.clone());
    Some(c)
}

fn field<'a>(o: &'a Obj, k: &str) -> Option<&'a Val> {
    o.iter().find(|(a, _)| a.as_str() == k).map(|(_, v)| v)
}

/// The property key a non-string `resourceType` becomes (`result[i.resourceType]`: ToString, so an
/// array joins with commas).
fn js_key(v: &Val) -> String {
    engine::live::to_js_string(&engine::live::from_val(v)).to_string()
}

/// `obj` minus `_id`, plus `id` when given (appended), as JSON.
fn doc_json(obj: &Obj, id: Option<&str>) -> String {
    let mut out = String::from("{");
    for (k, v) in obj.iter().filter(|(k, _)| k.as_str() != "_id") {
        if out.len() > 1 {
            out.push(',');
        }
        Val::Str(Arc::from(k.as_str())).write_json(&mut out);
        out.push(':');
        v.write_json(&mut out);
    }
    if let Some(id) = id {
        if out.len() > 1 {
            out.push(',');
        }
        out.push_str("\"id\":");
        Val::Str(Arc::from(id)).write_json(&mut out);
    }
    out.push('}');
    out
}

impl Cache {
    fn order(&self, i: usize) -> &str {
        self.order_json[i].get_or_init(|| {
            let doc = &self.market.orders[i];
            doc_json(&doc.obj, Some(doc.id().as_str())).into()
        })
    }

    fn group(&self, key: &str) -> Option<&Arc<str>> {
        let (list, json) = self.groups.get(key)?;
        Some(json.get_or_init(|| {
            let mut out = String::from("{");
            for &i in list {
                if out.len() > 1 {
                    out.push(',');
                }
                Val::Str(Arc::from(self.market.orders[i].id().as_str())).write_json(&mut out);
                out.push(':');
                out.push_str(self.order(i));
            }
            out.push('}');
            out.into()
        }))
    }

    fn history(&self) -> &HashMap<Box<str>, Arc<str>> {
        self.history.get_or_init(|| {
            let mut groups: Vec<(String, Vec<String>)> = vec![("all".into(), Vec::new())];
            for stat in self.market.stats.iter() {
                let json = doc_json(stat, None);
                let rt = field(stat, "resourceType").map_or("undefined".into(), js_key);
                match groups.iter_mut().find(|(k, _)| *k == rt) {
                    Some((_, l)) => l.push(json.clone()),
                    None => groups.push((rt, vec![json.clone()])),
                }
                groups[0].1.push(json);
            }
            groups.into_iter().map(|(k, l)| (k.into(), format!("[{}]", l.join(",")).into())).collect()
        })
    }
}

pub(crate) fn install(scope: &mut v8::PinScope, natives: v8::Local<v8::Object>) {
    set_fn(scope, natives, "marketOrders", market_orders);
    set_fn(scope, natives, "marketOrder", market_order);
    set_fn(scope, natives, "marketHistory", market_history);
}

fn arg(scope: &mut v8::PinScope, args: &v8::FunctionCallbackArguments) -> String {
    args.get(0).to_rust_string_lossy(scope)
}

fn ret(scope: &mut v8::PinScope, rv: &mut v8::ReturnValue<v8::Value>, json: &Arc<str>) {
    rv.set(crate::marshal::text(scope, json).into());
}

/// `marketOrders(resourceType)` → `JSON.stringify(runtimeData.market.orders[resourceType])`: `{id: order}`
/// (`all` for every active order), undefined for a resource without active orders.
fn market_orders(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue<v8::Value>) {
    let key = arg(scope, &args);
    let Some(c) = cache() else { return };
    if let Some(json) = c.group(&key) {
        ret(scope, &mut rv, json);
    }
}

/// `marketOrder(id)` → `JSON.stringify(runtimeData.market.orders.all[id])` | undefined.
fn market_order(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue<v8::Value>) {
    let id = arg(scope, &args);
    let Some(c) = cache() else { return };
    if let Some(&i) = c.by_id.get(&*id) {
        let json: Arc<str> = c.order(i).into();
        ret(scope, &mut rv, &json);
    }
}

/// `marketHistory(resourceType)` → `JSON.stringify(runtimeData.market.history[resourceType] || {})`.
fn market_history(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue<v8::Value>) {
    let key = arg(scope, &args);
    let Some(c) = cache() else { return };
    let empty: Arc<str> = Arc::from("{}");
    ret(scope, &mut rv, c.history().get(&*key).unwrap_or(&empty));
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine::doc::{Atom, Doc};

    fn obj(fields: &[(&str, Val)]) -> Obj {
        let mut o = Obj::default();
        for (k, v) in fields {
            o.set(Atom::intern(k), v.clone());
        }
        o
    }

    #[test]
    fn order_json_like_data_js() {
        let s = |v: &str| Val::Str(Arc::from(v));
        let o = obj(&[("_id", s("abc")), ("type", s("sell")), ("price", Val::Num(1500.0)), ("active", Val::Bool(true)), ("roomName", s("W1N1\u{e9}"))]);
        let bs = char::from(92u8); // a backslash: the text must stay ASCII
        assert_eq!(doc_json(&o, Some("abc")), format!(r#"{{"type":"sell","price":1500,"active":true,"roomName":"W1N1{bs}u00e9","id":"abc"}}"#));
        let _ = Doc::new(1, o);
        assert_eq!(js_key(&Val::Num(1.5)), "1.5");
        assert_eq!(js_key(&Val::Arr(vec![s("H"), Val::Null, Val::Num(2.0)])), "H,,2");
        assert_eq!(js_key(&Val::Obj(Obj::default())), "[object Object]");
    }
}
