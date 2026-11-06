//! Compile-time Rust mod list (the built-ins here, the crates in mods/ through modlist), JS mod discovery (`<data>/mods/<name>/mod.json`), one DFS load order
//! (ARCHITECTURE.md §10). Owner: WP21.

use backend::mods::Mod;

#[cfg(feature = "devtools")]
mod devtools;
#[cfg(feature = "history")]
mod history;

/// The built-in Rust mods (§10.1); the crates in mods/ are `modlist::MODS`.
pub static MODS: &[&dyn Mod] = &[
    #[cfg(feature = "history")]
    &history::MOD,
    #[cfg(feature = "devtools")]
    &devtools::MOD,
];

/// The enabled mods (`mods` in the config) in load order (§10.3: DFS post-order over dependencies, siblings by
/// name), registered into one `Registry`, and their `js_game` as the runtime's JS-mod game slots
/// (`mods/<mod>.js`, `mods/<mod>.2.js`, … in registration order). An unknown name, a missing dependency or a cycle
/// is a startup error.
// ponytail: Rust mods only; JS mods (`<data>/mods/<name>/mod.json`, §10.2) join this graph when the server isolate
// can run their server slot.
pub type GameJs = Vec<(Box<str>, Box<str>)>;

/// Whether this build has a mod of that name.
pub fn built_in(name: &str) -> bool {
    MODS.iter().chain(modlist::MODS).any(|m| m.manifest().name == name)
}

pub fn load(enabled: &[String], data_dir: &std::path::Path, steam_key: Option<String>) -> Result<(Vec<&'static str>, backend::mods::Registry, GameJs), String> {
    fn find(name: &str) -> Result<&'static dyn Mod, String> {
        let mut found = MODS.iter().chain(modlist::MODS).copied().filter(|m| m.manifest().name == name);
        let m = found.next().ok_or_else(|| format!("mod '{name}' is not available in this build"))?;
        if found.next().is_some() {
            return Err(format!("two mods in this build are named '{name}'"));
        }
        Ok(m)
    }
    fn visit(name: &str, stack: &mut Vec<String>, order: &mut Vec<&'static str>) -> Result<(), String> {
        let m = find(name)?;
        let me = m.manifest().name;
        if order.contains(&me) {
            return Ok(());
        }
        if stack.iter().any(|s| s == me) {
            return Err(format!("mod dependency cycle: {} -> {me}", stack.join(" -> ")));
        }
        stack.push(me.into());
        let mut deps: Vec<&str> = m.manifest().dependencies.to_vec();
        deps.sort_unstable();
        for d in deps {
            visit(d, stack, order)?;
        }
        stack.pop();
        order.push(me);
        Ok(())
    }
    let mut names: Vec<&str> = enabled.iter().map(String::as_str).collect();
    names.sort_unstable();
    let mut order = Vec::new();
    for n in names {
        visit(n, &mut Vec::new(), &mut order)?;
    }
    let mut r = backend::mods::Registry { data_dir: data_dir.to_path_buf(), steam_key, ..Default::default() };
    let mut game_js = GameJs::new();
    for name in &order {
        let before = r.js_game.len();
        find(name)?.register(&mut r);
        for (k, js) in r.js_game[before..].iter().enumerate() {
            let slot = if k == 0 { name.to_string() } else { format!("{name}.{}", k + 1) };
            game_js.push((slot.into(), (*js).into()));
        }
    }
    Ok((order, r, game_js))
}
