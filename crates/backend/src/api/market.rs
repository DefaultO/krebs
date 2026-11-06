//! mirror: reference/backend/lib/game/api/market.js
//! parity: FAITHFUL
//!
//! Checked against the official router in tests/fixtures/api (server.rs oracle). Prices are stored ×1000
//! (`market.orders`) and divided here, on copies.
#![allow(unused_variables)]

use crate::http::{obj, Request, Response};
use crate::{field, str_field, ApiCtx};
use engine::doc::{Obj, Val};
use util::json::Json;

macro_rules! auth {
    ($ctx:expr) => {
        match $ctx.user {
            Some(u) => u,
            None => return Response::unauthorized(),
        }
    };
}

/// The order after `i.price /= 1000`: ToNumber of the stored value (NaN → null); an absent price is created last.
fn order_json(o: &Obj) -> Json {
    let price = |v: Option<&Val>| {
        let p = crate::api::user::to_number(v.map(Val::to_json).as_ref()) / 1000.0;
        if p.is_finite() { Json::Num(p) } else { Json::Null }
    };
    let mut m: Vec<(String, Json)> = o.iter().map(|(k, v)| (k.as_str().to_string(), if k.as_str() == "price" { price(Some(v)) } else { v.to_json() })).collect();
    if field(o, "price").is_none() {
        m.push(("price".into(), price(None)));
    }
    Json::Obj(m)
}

/// GET /api/game/market/orders-index
pub fn orders_index(req: &Request, ctx: &ApiCtx) -> Response {
    let _ = auth!(ctx);
    let view = ctx.published.get();
    // list[resourceType] in first-appearance order (Object.values)
    // (String(resourceType), the first order's raw resourceType for `_id`, [count, buying, selling])
    let mut list: Vec<(String, Option<Json>, [f64; 3])> = Vec::new();
    for d in view.market.orders.iter().filter(|d| field(&d.obj, "active") == Some(&Val::Bool(true))) {
        let raw = field(&d.obj, "resourceType").map(Val::to_json);
        let rt = crate::socket::user::js_to_string(raw.as_ref());
        let i = match list.iter().position(|(r, _, _)| *r == rt) {
            Some(i) => i,
            None => {
                list.push((rt, raw, [0.0; 3]));
                list.len() - 1
            }
        };
        match str_field(&d.obj, "type") {
            Some("buy") => list[i].2[1] += 1.0,
            Some("sell") => list[i].2[2] += 1.0,
            _ => {}
        }
        list[i].2[0] += 1.0;
    }
    // integer-like resource types would be enumerated first by Object.values
    let mut keyed: Vec<(String, String)> = Vec::new();
    for (rt, raw, [count, buying, selling]) in list {
        // `_id: resourceType` (an undefined one is dropped by JSON)
        let id = raw.map(|r| ("_id".to_string(), r));
        let entry = Json::Obj(id.into_iter().chain([("count".to_string(), Json::Num(count)), ("buying".to_string(), Json::Num(buying)), ("selling".to_string(), Json::Num(selling))]).collect());
        crate::socket::rooms::js_push(&mut keyed, rt, entry.stringify());
    }
    let values: Vec<Json> = keyed.into_iter().map(|(_, v)| Json::parse(&v).expect("own json")).collect();
    Response::ok(obj(vec![("list", Json::Arr(values))]))
}

/// GET /api/game/market/orders
pub fn orders(req: &Request, ctx: &ApiCtx) -> Response {
    let _ = auth!(ctx);
    let view = ctx.published.get();
    let rt = req.param("resourceType").map(|r| Json::Str(r.into()));
    let list = view
        .market
        .orders
        .iter()
        .filter(|d| field(&d.obj, "active") == Some(&Val::Bool(true)) && crate::api::user::clause(&d.obj, "resourceType", rt.as_ref()))
        .map(|d| order_json(&d.obj))
        .collect();
    Response::ok(obj(vec![("list", Json::Arr(list))]))
}

/// GET /api/game/market/my-orders
pub fn my_orders(req: &Request, ctx: &ApiCtx) -> Response {
    let u = auth!(ctx);
    let view = ctx.published.get();
    let id = view.users[u as usize].id;
    let list = view.market.orders.iter().filter(|d| str_field(&d.obj, "user") == Some(id.as_str())).map(|d| order_json(&d.obj)).collect();
    Response::ok(obj(vec![("list", Json::Arr(list))]))
}

/// GET /api/game/market/stats
pub fn stats(req: &Request, ctx: &ApiCtx) -> Response {
    let _ = auth!(ctx);
    let view = ctx.published.get();
    let rt = req.param("resourceType").map(|r| Json::Str(r.into()));
    let mut list: Vec<&std::sync::Arc<Obj>> = view.market.stats.iter().filter(|d| crate::api::user::clause(d, "resourceType", rt.as_ref())).collect();
    // findEx sort {date: -1}: Loki simplesort descending, stable for equal dates
    list.sort_by(|a, b| {
        let (x, y) = (field(b, "date").map(Val::to_json), field(a, "date").map(Val::to_json));
        match (&x, &y) {
            (Some(Json::Num(p)), Some(Json::Num(q))) => p.partial_cmp(q).unwrap_or(std::cmp::Ordering::Equal),
            _ => crate::socket::user::js_to_string(x.as_ref()).cmp(&crate::socket::user::js_to_string(y.as_ref())),
        }
    });
    Response::ok(obj(vec![("stats", Json::Arr(list.into_iter().map(|d| Val::Obj((**d).clone()).to_json()).collect()))]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fake;
    use engine::doc::Doc;
    use engine::world::MarketState;
    use std::sync::Arc;

    /// `i.price /= 1000` on odd stored prices and `_id: resourceType` kept raw (node semantics).
    #[test]
    fn js_price_and_index_ids() {
        let (ctx, _h) = fake::ctx(Some(fake::ALICE));
        crate::write(&ctx, |w| {
            let orders = [
                r#"{"_id":"o1","active":true,"type":"sell","resourceType":"energy","price":"1500","user":"a11ce0000000001"}"#,
                r#"{"_id":"o2","active":true,"type":"buy","resourceType":"energy","user":"a11ce0000000001"}"#,
                r#"{"_id":"o3","active":true,"type":"buy","resourceType":7,"price":true,"user":"a11ce0000000001"}"#,
                r#"{"_id":"o4","active":true,"type":"buy","price":{},"user":"a11ce0000000001"}"#,
            ];
            let docs: Vec<engine::doc::DocRef> = orders.iter().enumerate().map(|(i, o)| Doc::new(i as u64, fake::obj(o))).collect();
            w.market = Arc::new(MarketState::new(docs.into(), Arc::from(Vec::new()), Arc::from(Vec::new()), Arc::from(Vec::new()), Arc::from(Vec::new())));
            Ok(())
        })
        .unwrap();
        let (_, j) = fake::run(my_orders, &ctx, "GET", "/x", "");
        let prices: Vec<String> = j.get("list").unwrap().as_array().unwrap().iter().map(|o| o.get("price").unwrap().stringify()).collect();
        assert_eq!(prices, ["1.5", "null", "0.001", "null"]);
        let o2: Vec<&str> = j.get("list").unwrap().at(1).unwrap().as_object().unwrap().iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(o2.last(), Some(&"price"), "created last");
        let (_, idx) = fake::run(orders_index, &ctx, "GET", "/x", "");
        assert_eq!(idx.get("list").unwrap().stringify(), r#"[{"_id":7,"count":1,"buying":1,"selling":0},{"_id":"energy","count":2,"buying":1,"selling":1},{"count":1,"buying":1,"selling":0}]"#);
    }
}
