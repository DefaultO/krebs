//! mirror: reference/engine/src/processor/global-intents/market.js
//! parity: FAITHFUL
//!
//! Statement-by-statement port over the global stage's live graphs ([`Scope`]). Where vanilla would throw
//! (a missing user or store), the item is skipped instead: a global-stage throw is TICK-ERROR, which chitin
//! never reproduces (COMPAT-CRASH-GLOBAL).

use crate::atom;
use crate::bulk::Target;
use crate::constants as C;
use crate::doc::Atom;
use crate::live::{is_equal, to_js_string, to_number, truthy, O, V};
use crate::processor::global::{obj, resource_key, Scope};
use crate::processor::{get_or_undef, gt, js_max, js_min, js_round, le, loose_eq, lt, or0, put};
use crate::utils::{calc_resources, calc_rooms_distance, calc_terminal_energy_cost};
use std::collections::HashMap;
use std::sync::Arc;

fn g(v: &V, k: Atom) -> V {
    get_or_undef(v, k)
}
fn n(v: &V) -> f64 {
    to_number(v)
}
fn ov(o: &O) -> V {
    V::Obj(o.clone())
}

fn is_intershard(rt: &V) -> bool {
    matches!(rt, V::Str(s) if C::INTERSHARD_RESOURCES.contains(&&**s))
}

/// `_.contains(C.RESOURCES_ALL, rt)`.
fn is_resource(rt: &V) -> bool {
    matches!(rt, V::Str(s) if C::RESOURCES_ALL.iter().any(|r| r.as_str() == &**s))
}

/// `store[rt]` (a key that is not a resource type reads undefined; such keys never reach the global stage).
fn store_get(o: &V, rt: &V) -> V {
    resource_key(rt).map_or(V::Undef, |k| g(&g(o, atom!("store")), k))
}

/// `{[rt]: v}`.
fn keyed(rt: &V, v: V) -> V {
    let o = O::new();
    if let Some(k) = resource_key(rt) {
        o.set(k, v);
    }
    V::Obj(o)
}

/// `_.find(t.effects, {power: PWR_OPERATE_TERMINAL})` with `effect.endTime > gameTime`:
/// `POWER_INFO[PWR_OPERATE_TERMINAL].effect[effect.level-1]` (NaN for a level outside 1..5).
fn operate_terminal(t: &V, game_time: f64) -> Option<f64> {
    let list = match g(t, atom!("effects")) {
        V::Arr(a) => a.to_vec(),
        V::Obj(o) => o.keys().into_iter().map(|k| o.get(k)).collect(),
        _ => Vec::new(),
    };
    let power = V::Num(C::PWR_OPERATE_TERMINAL as f64);
    let e = list.into_iter().find(|e| matches!(e, V::Obj(_)) && is_equal(&g(e, atom!("power")), &power))?;
    if !gt(&g(&e, atom!("endTime")), &V::Num(game_time)) {
        return None;
    }
    let i = n(&g(&e, atom!("level"))) - 1.0;
    let effect = C::power_info(C::PWR_OPERATE_TERMINAL).and_then(|p| p.effect).expect("PWR_OPERATE_TERMINAL has an effect");
    Some(if (0.0..5.0).contains(&i) && i.fract() == 0.0 { effect[i as usize] } else { f64::NAN })
}

fn cooldown_of(t: &V, game_time: f64) -> f64 {
    let cooldown = C::TERMINAL_COOLDOWN as f64;
    operate_terminal(t, game_time).map_or(cooldown, |m| js_round(cooldown * m))
}

/// market.js:15 executeTransfer; `true` only when the transfer happened.
fn execute_transfer(s: &mut Scope, from: &O, to: &O, rt: &V, amount: V, fee_t: &O, additional: Vec<(&'static str, V)>) -> bool {
    let (fv, tv, feev) = (ov(from), ov(to), ov(fee_t));
    let mut amount = amount;
    if truthy(&g(&fv, atom!("user"))) {
        let store = g(&fv, atom!("store"));
        if !truthy(&store) || !truthy(&store_get(&fv, rt)) || lt(&store_get(&fv, rt), &amount) {
            return false;
        }
    }
    if truthy(&g(&tv, atom!("user"))) {
        let Ok(total) = calc_resources(&tv) else { return false };
        let free_space = js_max(0.0, n(&g(&tv, atom!("storeCapacity"))) - total);
        amount = V::Num(js_min(n(&amount), free_space));
    }
    if !gt(&amount, &V::Num(0.0)) {
        return false;
    }

    let range = calc_rooms_distance(&to_js_string(&g(&fv, atom!("room"))), &to_js_string(&g(&tv, atom!("room"))), true, s.world_size);
    let mut transfer_cost = calc_terminal_energy_cost(n(&amount), range);
    if let Some(m) = operate_terminal(&feev, s.game_time) {
        transfer_cost = (transfer_cost * m).ceil();
    }
    let cost = V::Num(transfer_cost);

    let energy_of = |v: &V| -> Option<V> {
        match g(v, atom!("store")) {
            V::Undef | V::Null => None, // vanilla reads `.energy` of undefined: throws
            st => Some(g(&st, atom!("energy"))),
        }
    };
    // (fee === from && (…)) || (fee === to && to.store.energy < cost)
    if fee_t.same(from) {
        let Some(e) = energy_of(&fv) else { return false };
        let is_energy = crate::processor::eq_str(rt, C::RESOURCE_ENERGY);
        if (!is_energy && lt(&e, &cost)) || (is_energy && lt(&e, &crate::processor::js_add(&amount, &cost))) {
            return false;
        }
    }
    if fee_t.same(to) {
        let Some(e) = energy_of(&tv) else { return false };
        if lt(&e, &cost) {
            return false;
        }
    }

    if truthy(&g(&tv, atom!("user"))) {
        if !truthy(&g(&tv, atom!("store"))) {
            to.set(atom!("store"), V::Obj(O::new()));
        }
        let next = crate::processor::js_add(&or0(store_get(&tv, rt)), &amount);
        if let Some(k) = resource_key(rt) {
            let _ = put(&g(&tv, atom!("store")), k, next.clone());
        }
        let _ = s.bulk_objects.update(Target::Obj(to), &obj(vec![("store", keyed(rt, next))]));
    }

    let left = V::Num(n(&store_get(&fv, rt)) - n(&amount));
    let _ = s.bulk_objects.update(Target::Obj(from), &obj(vec![("store", keyed(rt, left))]));
    let fee_energy = V::Num(n(&g(&g(&feev, atom!("store")), atom!("energy"))) - transfer_cost);
    let _ = s.bulk_objects.update(Target::Obj(fee_t), &obj(vec![("store", obj(vec![("energy", fee_energy)]))]));

    let user_str = |v: &V| {
        let u = g(v, atom!("user"));
        if truthy(&u) {
            V::Str(to_js_string(&u))
        } else {
            V::Undef
        }
    };
    let tx = obj(vec![
        ("time", V::Num(s.game_time)),
        ("sender", user_str(&fv)),
        ("recipient", user_str(&tv)),
        ("resourceType", rt.clone()),
        ("amount", amount),
        ("from", g(&fv, atom!("room"))),
        ("to", g(&tv, atom!("room"))),
    ]);
    if let V::Obj(t) = &tx {
        for (k, v) in additional {
            t.set(Atom::intern(k), v);
        }
    }
    s.bulk_transactions.insert(&tx, None);
    true
}

/// market.js:7.
pub(crate) fn run(s: &mut Scope) {
    let game_time = s.game_time;

    for terminal in s.terminals_with_send() {
        let tv = ov(&terminal);
        let intent = g(&tv, atom!("send"));
        let _ = s.bulk_objects.update(Target::Obj(&terminal), &obj(vec![("send", V::Null)]));
        if gt(&g(&tv, atom!("cooldownTime")), &V::Num(game_time)) {
            continue;
        }
        let Some(target) = s.terminal_in(&g(&intent, atom!("targetRoomName"))) else { continue };
        if !truthy(&g(&ov(&target), atom!("user"))) {
            continue;
        }
        let cooldown = cooldown_of(&tv, game_time);
        let description = g(&intent, atom!("description"));
        let description = if truthy(&description) { V::str(&to_js_string(&description).replace('<', "&lt;")) } else { V::Undef };
        let rt = g(&intent, atom!("resourceType"));
        if execute_transfer(s, &terminal, &target, &rt, g(&intent, atom!("amount")), &terminal, vec![("description", description)]) {
            let _ = s.bulk_objects.update(Target::Obj(&terminal), &obj(vec![("cooldownTime", V::Num(game_time + cooldown))]));
        }
    }

    let mut orders_by_id: HashMap<Arc<str>, O> = HashMap::new();
    for o in &s.orders {
        orders_by_id.insert(to_js_string(&o.get(atom!("_id"))), o.clone());
    }
    let order_of = |id: &V| orders_by_id.get(&*to_js_string(id)).cloned();
    let mut terminal_deals: Vec<V> = Vec::new();
    let mut direct_deals: Vec<V> = Vec::new();
    let now = s.now;
    let fee_rate = C::MARKET_FEE;

    for doc in s.user_intents.clone() {
        let uid = doc.get(atom!("user"));
        let user = s.users_by_id.get(&*to_js_string(&uid)).cloned();
        let intents = doc.get(atom!("intents"));
        let each = |name: Atom| match g(&intents, name) {
            V::Arr(a) => a.to_vec(),
            _ => Vec::new(),
        };

        for intent in each(atom!("createOrder")) {
            let (price, total) = (g(&intent, atom!("price")), g(&intent, atom!("totalAmount")));
            if !truthy(&price) || !truthy(&total) {
                continue;
            }
            let rt = g(&intent, atom!("resourceType"));
            if !is_resource(&rt) && !is_intershard(&rt) {
                continue;
            }
            if !is_intershard(&rt) {
                match s.terminal_in(&g(&intent, atom!("roomName"))) {
                    Some(t) if loose_eq(&g(&ov(&t), atom!("user")), &uid) => {}
                    _ => continue,
                }
            }
            if le(&price, &V::Num(0.0)) || le(&total, &V::Num(0.0)) {
                continue;
            }
            let fee = (n(&price) * n(&total) * fee_rate).ceil();
            let Some(user) = user.clone() else { continue };
            if lt(&user.get(atom!("money")), &V::Num(fee)) {
                continue;
            }
            s.bulk_users.inc(Target::Obj(&user), &[atom!("money")], -fee);

            let order = O::new();
            order.set(atom!("createdTimestamp"), V::Num(now));
            order.set(atom!("user"), uid.clone());
            order.set(atom!("active"), V::Bool(false));
            order.set(atom!("type"), V::str(if crate::processor::eq_str(&g(&intent, atom!("type")), C::ORDER_SELL) { C::ORDER_SELL } else { C::ORDER_BUY }));
            order.set(atom!("amount"), V::Num(0.0));
            order.set(atom!("remainingAmount"), total.clone());
            if let V::Obj(i) = &intent {
                crate::live::assign(&order, i);
            }
            if is_intershard(&rt) {
                s.bulk_market_intershard_orders.insert(&ov(&order), None);
            } else {
                order.set(atom!("created"), V::Num(game_time));
                s.bulk_market_orders.insert(&ov(&order), None);
            }
            let _ = put(&intent, atom!("price"), V::Num(n(&price) / 1000.0));
            let entry = money_entry(s, &uid, "market.fee", n(&user.get(atom!("money"))) / 1000.0, -fee / 1000.0, obj(vec![("order", intent.clone())]));
            s.bulk_users_money.insert(&entry, None);
        }

        for intent in each(atom!("changeOrderPrice")) {
            let Some(order) = order_of(&g(&intent, atom!("orderId"))) else { continue };
            if !loose_eq(&order.get(atom!("user")), &uid) {
                continue;
            }
            let new_price = g(&intent, atom!("newPrice"));
            if !truthy(&new_price) || le(&new_price, &V::Num(0.0)) {
                continue;
            }
            let price = order.get(atom!("price"));
            if !loose_eq(&new_price, &price) {
                order.set(Atom::intern("_skip"), V::Bool(true));
            }
            if gt(&new_price, &price) {
                let fee = ((n(&new_price) - n(&price)) * n(&order.get(atom!("remainingAmount"))) * fee_rate).ceil();
                let Some(user) = user.clone() else { continue };
                if lt(&user.get(atom!("money")), &V::Num(fee)) {
                    continue;
                }
                s.bulk_users.inc(Target::Obj(&user), &[atom!("money")], -fee);
                let change = obj(vec![("orderId", g(&intent, atom!("orderId"))), ("oldPrice", V::Num(n(&price) / 1000.0)), ("newPrice", V::Num(n(&new_price) / 1000.0))]);
                let entry = money_entry(s, &uid, "market.fee", n(&user.get(atom!("money"))) / 1000.0, -fee / 1000.0, obj(vec![("changeOrderPrice", change)]));
                s.bulk_users_money.insert(&entry, None);
            }
            let delta = n(&new_price) - n(&price);
            orders_bulk(s, &order).inc(Target::Obj(&order), &[atom!("price")], delta);
        }

        for intent in each(atom!("extendOrder")) {
            let Some(order) = order_of(&g(&intent, atom!("orderId"))) else { continue };
            if !loose_eq(&order.get(atom!("user")), &uid) {
                continue;
            }
            let add = g(&intent, atom!("addAmount"));
            if !truthy(&add) || le(&add, &V::Num(0.0)) {
                continue;
            }
            let fee = (n(&order.get(atom!("price"))) * n(&add) * fee_rate).ceil();
            let Some(user) = user.clone() else { continue };
            if lt(&user.get(atom!("money")), &V::Num(fee)) {
                continue;
            }
            s.bulk_users.inc(Target::Obj(&user), &[atom!("money")], -fee);
            let ext = obj(vec![("orderId", g(&intent, atom!("orderId"))), ("addAmount", add.clone())]);
            let entry = money_entry(s, &uid, "market.fee", n(&user.get(atom!("money"))) / 1000.0, -fee / 1000.0, obj(vec![("extendOrder", ext)]));
            s.bulk_users_money.insert(&entry, None);
            let bulk = orders_bulk(s, &order);
            bulk.inc(Target::Obj(&order), &[atom!("remainingAmount")], n(&add));
            bulk.inc(Target::Obj(&order), &[atom!("totalAmount")], n(&add));
        }

        for intent in each(atom!("cancelOrder")) {
            if let Some(order) = order_of(&g(&intent, atom!("orderId"))) {
                if loose_eq(&order.get(atom!("user")), &uid) {
                    order.set(atom!("remainingAmount"), V::Num(0.0));
                    order.set(Atom::intern("_cancelled"), V::Bool(true));
                }
            }
        }

        for intent in each(atom!("deal")) {
            let _ = put(&intent, atom!("user"), uid.clone());
            let Some(order) = order_of(&g(&intent, atom!("orderId"))) else { continue };
            if truthy(&order.get(Atom::intern("_skip"))) {
                continue;
            }
            if le(&g(&intent, atom!("amount")), &V::Num(0.0)) {
                continue;
            }
            if is_intershard(&order.get(atom!("resourceType"))) {
                direct_deals.push(intent);
                continue;
            }
            match s.terminal_in(&g(&intent, atom!("targetRoomName"))) {
                Some(t) if loose_eq(&g(&ov(&t), atom!("user")), &uid) => terminal_deals.push(intent),
                _ => {}
            }
        }
    }

    // stable sort by `distance(a) - distance(b)` (V8 TimSort; a NaN difference compares equal)
    let world_size = s.world_size;
    let dist = |d: &V| {
        let room = order_of(&g(d, atom!("orderId"))).map_or(V::Undef, |o| o.get(atom!("roomName")));
        calc_rooms_distance(&to_js_string(&g(d, atom!("targetRoomName"))), &to_js_string(&room), true, world_size)
    };
    let mut keyed_deals: Vec<(f64, V)> = terminal_deals.into_iter().map(|d| (dist(&d), d)).collect();
    keyed_deals.sort_by(|a, b| (a.0 - b.0).partial_cmp(&0.0).unwrap_or(std::cmp::Ordering::Equal));

    for (_, deal) in keyed_deals {
        let Some(order) = order_of(&g(&deal, atom!("orderId"))) else { continue };
        let (Some(order_terminal), Some(target_terminal)) = (s.terminal_in(&order.get(atom!("roomName"))), s.terminal_in(&g(&deal, atom!("targetRoomName")))) else {
            continue;
        };
        if gt(&g(&ov(&target_terminal), atom!("cooldownTime")), &V::Num(game_time)) {
            continue;
        }
        for t in [&order_terminal, &target_terminal] {
            if !truthy(&t.get(atom!("store"))) {
                t.set(atom!("store"), V::Obj(O::new()));
            }
        }
        let (buyer, seller) = if crate::processor::eq_str(&order.get(atom!("type")), C::ORDER_SELL) {
            (target_terminal.clone(), order_terminal.clone())
        } else {
            (order_terminal.clone(), target_terminal.clone())
        };
        let (bv, sv) = (ov(&buyer), ov(&seller));
        let rt = order.get(atom!("resourceType"));
        let price = n(&order.get(atom!("price")));

        let mut amount = js_min(n(&g(&deal, atom!("amount"))), n(&order.get(atom!("remainingAmount"))));
        if truthy(&g(&sv, atom!("user"))) {
            amount = js_min(amount, n(&or0(store_get(&sv, &rt))));
        }
        if truthy(&g(&bv, atom!("user"))) {
            let Ok(total) = calc_resources(&bv) else { continue };
            amount = js_min(amount, js_max(0.0, n(&g(&bv, atom!("storeCapacity"))) - total));
        }
        if !(amount > 0.0) {
            continue;
        }
        let mut deal_cost = amount * price;
        if truthy(&g(&bv, atom!("user"))) {
            let Some(bu) = s.users_by_id.get(&*to_js_string(&g(&bv, atom!("user")))).cloned() else { continue };
            deal_cost = js_min(deal_cost, n(&or0(bu.get(atom!("money")))));
            amount = (deal_cost / price).floor();
            deal_cost = amount * price;
            if amount == 0.0 || amount.is_nan() {
                continue;
            }
        }

        let order_info = obj(vec![("id", V::Str(to_js_string(&order.get(atom!("_id"))))), ("type", order.get(atom!("type"))), ("price", V::Num(price / 1000.0))]);
        if !execute_transfer(s, &seller, &buyer, &rt, V::Num(amount), &target_terminal, vec![("order", order_info)]) {
            continue;
        }
        let deal_market = |npc: bool| {
            obj(vec![
                ("resourceType", rt.clone()),
                ("roomName", order.get(atom!("roomName"))),
                ("targetRoomName", g(&deal, atom!("targetRoomName"))),
                ("price", V::Num(price / 1000.0)),
                ("npc", V::Bool(npc)),
                ("owner", order.get(atom!("user"))),
                ("dealer", g(&deal, atom!("user"))),
                ("amount", V::Num(amount)),
            ])
        };
        if truthy(&g(&sv, atom!("user"))) {
            let suid = g(&sv, atom!("user"));
            if let Some(su) = s.users_by_id.get(&*to_js_string(&suid)).cloned() {
                s.bulk_users.inc(Target::Obj(&su), &[atom!("money")], deal_cost);
                let entry = money_entry(s, &suid, "market.sell", n(&su.get(atom!("money"))) / 1000.0, deal_cost / 1000.0, deal_market(!truthy(&g(&bv, atom!("user")))));
                s.bulk_users_money.insert(&entry, None);
            }
        }
        if truthy(&g(&bv, atom!("user"))) {
            let buid = g(&bv, atom!("user"));
            if let Some(bu) = s.users_by_id.get(&*to_js_string(&buid)).cloned() {
                s.bulk_users.inc(Target::Obj(&bu), &[atom!("money")], -deal_cost);
                let entry = money_entry(s, &buid, "market.buy", n(&bu.get(atom!("money"))) / 1000.0, -deal_cost / 1000.0, deal_market(!truthy(&g(&sv, atom!("user")))));
                s.bulk_users_money.insert(&entry, None);
            }
        }
        let update = obj(vec![
            ("amount", V::Num(n(&order.get(atom!("amount"))) - amount)),
            ("remainingAmount", V::Num(n(&order.get(atom!("remainingAmount"))) - amount)),
        ]);
        let _ = s.bulk_market_orders.update(Target::Obj(&order), &update);
        let cooldown = cooldown_of(&ov(&target_terminal), game_time);
        let _ = s.bulk_objects.update(Target::Obj(&target_terminal), &obj(vec![("cooldownTime", V::Num(game_time + cooldown))]));
    }

    let shuffled = {
        let rng = s.rng();
        crate::policy::lodash_shuffle(rng, &direct_deals)
    };
    for deal in shuffled {
        let Some(order) = order_of(&g(&deal, atom!("orderId"))) else { continue };
        let user_of = |v: &V| s.users_by_id.get(&*to_js_string(v)).cloned();
        let (buyer, seller) = if crate::processor::eq_str(&order.get(atom!("type")), C::ORDER_SELL) {
            (user_of(&g(&deal, atom!("user"))), user_of(&order.get(atom!("user"))))
        } else {
            (user_of(&order.get(atom!("user"))), user_of(&g(&deal, atom!("user"))))
        };
        let (Some(buyer), Some(seller)) = (buyer, seller) else { continue };
        for u in [&seller, &buyer] {
            if !truthy(&u.get(atom!("resources"))) {
                u.set(atom!("resources"), V::Obj(O::new()));
            }
        }
        let rt = order.get(atom!("resourceType"));
        let rk = resource_key(&rt);
        let res = |u: &O| rk.map_or(V::Undef, |k| get_or_undef(&u.get(atom!("resources")), k));
        let amount = [n(&g(&deal, atom!("amount"))), n(&order.get(atom!("amount"))), n(&order.get(atom!("remainingAmount"))), n(&or0(res(&seller)))].into_iter().fold(f64::INFINITY, js_min);
        if amount == 0.0 || amount.is_nan() || amount < 0.0 {
            continue;
        }
        let price = n(&order.get(atom!("price")));
        let deal_cost = amount * price;
        if truthy(&buyer.get(atom!("user"))) && (!truthy(&buyer.get(atom!("money"))) || lt(&buyer.get(atom!("money")), &V::Num(deal_cost))) {
            continue;
        }
        let (sid, bid, oid) = (to_js_string(&seller.get(atom!("_id"))), to_js_string(&buyer.get(atom!("_id"))), to_js_string(&order.get(atom!("_id"))));

        s.bulk_users.inc(Target::Obj(&seller), &[atom!("money")], deal_cost);
        // vanilla's live `seller['resources.X']` is a junk key nobody reads; the nested value changes only below
        if let Some(k) = rk {
            s.bulk_users.inc(Target::Id(&sid), &[atom!("resources"), k], -amount);
            let _ = put(&seller.get(atom!("resources")), k, V::Num(n(&res(&seller)) - amount));
        }
        let market = obj(vec![("resourceType", rt.clone()), ("price", V::Num(price / 1000.0)), ("amount", V::Num(amount))]);
        let entry = money_entry(s, &V::Str(sid.clone()), "market.sell", n(&seller.get(atom!("money"))) / 1000.0, deal_cost / 1000.0, market.clone());
        s.bulk_users_money.insert(&entry, None);
        let res_entry = obj(vec![
            ("date", s.date()),
            ("resourceType", rt.clone()),
            ("user", V::Str(sid.clone())),
            ("change", V::Num(-amount)),
            ("balance", res(&seller)),
            ("marketOrderId", V::Str(oid.clone())),
            ("market", obj(vec![("orderId", V::Str(oid.clone())), ("anotherUser", V::Str(bid.clone()))])),
        ]);
        s.bulk_users_resources.insert(&res_entry, None);

        s.bulk_users.inc(Target::Obj(&buyer), &[atom!("money")], -deal_cost);
        if let Some(k) = rk {
            s.bulk_users.inc(Target::Id(&bid), &[atom!("resources"), k], amount);
        }
        // `buyer.money -= dealCost` after the live `$inc`: vanilla's in-memory balance drops twice
        buyer.set(atom!("money"), V::Num(n(&buyer.get(atom!("money"))) - deal_cost));
        let entry = money_entry(s, &V::Str(bid.clone()), "market.buy", n(&buyer.get(atom!("money"))) / 1000.0, -deal_cost / 1000.0, market);
        s.bulk_users_money.insert(&entry, None);
        let bulk = orders_bulk(s, &order);
        bulk.inc(Target::Obj(&order), &[atom!("amount")], -amount);
        bulk.inc(Target::Obj(&order), &[atom!("remainingAmount")], -amount);
        let res_entry = obj(vec![
            ("date", s.date()),
            ("resourceType", rt.clone()),
            ("user", V::Str(bid.clone())),
            ("change", V::Num(amount)),
            ("balance", res(&buyer)),
            ("market", obj(vec![("orderId", V::Str(oid.clone())), ("anotherUser", V::Str(sid.clone()))])),
        ]);
        s.bulk_users_resources.insert(&res_entry, None);
    }

    for order in s.orders.clone() {
        if truthy(&order.get(Atom::intern("_cancelled"))) {
            let id = to_js_string(&order.get(atom!("_id")));
            orders_bulk(s, &order).remove(&id);
            continue;
        }
        let owner = order.get(atom!("user"));
        if truthy(&owner) && now - n(&order.get(atom!("createdTimestamp"))) > C::MARKET_ORDER_LIFE_TIME as f64 {
            let remaining_fee = n(&order.get(atom!("remainingAmount"))) * n(&order.get(atom!("price"))) * fee_rate;
            if remaining_fee > 0.0 {
                if let Some(user) = s.users_by_id.get(&*to_js_string(&owner)).cloned() {
                    s.bulk_users.inc(Target::Obj(&user), &[atom!("money")], remaining_fee);
                    let info = obj(vec![
                        ("orderId", V::Str(to_js_string(&order.get(atom!("_id"))))),
                        ("type", order.get(atom!("type"))),
                        ("resourceType", order.get(atom!("resourceType"))),
                        ("price", V::Num(n(&order.get(atom!("price"))) / 1000.0)),
                        ("remainingAmount", order.get(atom!("remainingAmount"))),
                        ("roomName", order.get(atom!("roomName"))),
                    ]);
                    let uid = V::Str(to_js_string(&user.get(atom!("_id"))));
                    let entry = money_entry(s, &uid, "market.fee", n(&user.get(atom!("money"))) / 1000.0, remaining_fee / 1000.0, obj(vec![("order", info)]));
                    s.bulk_users_money.insert(&entry, None);
                }
            }
            let id = to_js_string(&order.get(atom!("_id")));
            orders_bulk(s, &order).remove(&id);
            continue;
        }
        if !truthy(&owner) {
            continue;
        }
        let rt = order.get(atom!("resourceType"));
        let terminal = s.terminal_in(&order.get(atom!("roomName")));
        let t_owned = |t: &Option<O>| t.as_ref().is_some_and(|t| loose_eq(&t.get(atom!("user")), &owner));

        if crate::processor::eq_str(&order.get(atom!("type")), C::ORDER_SELL) {
            let available = if is_intershard(&rt) {
                let u = s.users_by_id.get(&*to_js_string(&owner)).map_or(V::Undef, |u| u.get(atom!("resources")));
                n(&or0(resource_key(&rt).map_or(V::Undef, |k| get_or_undef(&or_obj(u), k))))
            } else if t_owned(&terminal) {
                n(&or0(store_get(&ov(terminal.as_ref().unwrap()), &rt)))
            } else {
                0.0
            };
            let available = js_min(available, n(&order.get(atom!("remainingAmount"))));
            if truthy(&order.get(atom!("active"))) {
                if available == 0.0 || available.is_nan() || available < 0.0 {
                    let _ = orders_bulk(s, &order).update(Target::Obj(&order), &obj(vec![("active", V::Bool(false)), ("amount", V::Num(0.0))]));
                    continue;
                }
                if !loose_eq(&order.get(atom!("amount")), &V::Num(available)) {
                    let _ = orders_bulk(s, &order).update(Target::Obj(&order), &obj(vec![("amount", V::Num(available))]));
                }
            } else if available > 0.0 {
                let _ = orders_bulk(s, &order).update(Target::Obj(&order), &obj(vec![("active", V::Bool(true)), ("amount", V::Num(available))]));
            }
        }

        if crate::processor::eq_str(&order.get(atom!("type")), C::ORDER_BUY) {
            let Some(user) = s.users_by_id.get(&*to_js_string(&owner)).cloned() else { continue };
            let user_money = n(&or0(user.get(atom!("money"))));
            let is_owner = is_intershard(&rt) || t_owned(&terminal);
            let mut new_amount = js_min((user_money / n(&order.get(atom!("price")))).floor(), n(&order.get(atom!("remainingAmount"))));
            if let Some(t) = &terminal {
                if truthy(&t.get(atom!("user"))) {
                    let Ok(total) = calc_resources(&ov(t)) else { continue };
                    new_amount = js_min(new_amount, js_max(0.0, n(&t.get(atom!("storeCapacity"))) - total));
                }
            }
            let new_active = is_owner && new_amount > 0.0;
            if !loose_eq(&order.get(atom!("amount")), &V::Num(new_amount)) || !loose_eq(&order.get(atom!("active")), &V::Bool(new_active)) {
                let _ = orders_bulk(s, &order).update(Target::Obj(&order), &obj(vec![("amount", V::Num(new_amount)), ("active", V::Bool(new_active))]));
            }
        }
    }
}

/// `x || {}` for a read.
fn or_obj(v: V) -> V {
    if truthy(&v) {
        v
    } else {
        V::Obj(O::new())
    }
}

/// `_.contains(C.INTERSHARD_RESOURCES, order.resourceType) ? bulkMarketIntershardOrders : bulkMarketOrders`.
fn orders_bulk<'a>(s: &'a mut Scope, order: &O) -> &'a mut crate::bulk::Bulk {
    if is_intershard(&order.get(atom!("resourceType"))) {
        &mut s.bulk_market_intershard_orders
    } else {
        &mut s.bulk_market_orders
    }
}

/// A users.money doc: `{date: new Date(), tick, user, type, balance, change, market}`.
fn money_entry(s: &Scope, user: &V, kind: &str, balance: f64, change: f64, market: V) -> V {
    obj(vec![
        ("date", s.date()),
        ("tick", V::Num(s.game_time)),
        ("user", user.clone()),
        ("type", V::str(kind)),
        ("balance", V::Num(balance)),
        ("change", V::Num(change)),
        ("market", market),
    ])
}
