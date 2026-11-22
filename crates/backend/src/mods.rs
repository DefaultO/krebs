//! Everything a mod can register (ARCHITECTURE.md §10.1, FROZEN). Owner: WP21.

use crate::socket::server::ChannelFn;
use crate::Route;

pub struct Manifest {
    pub name: &'static str,
    pub version: &'static str,
    pub dependencies: &'static [&'static str],
}

pub trait Mod: Sync {
    fn manifest(&self) -> Manifest;
    fn register(&self, r: &mut Registry);
}

#[derive(Default)]
pub struct Registry {
    pub engine: engine::hooks::EngineHooks,
    /// HTTP routes.
    pub routes: Vec<Route>,
    /// `config.backend.<path> = value` (vanilla mods' way): `("welcomeText", …)`, `("customObjectTypes.myobject",
    /// …)`, `("renderer.metadata.myobject", …)`, `("socketUpdateThrottle", …)`; /api/version's `serverData` and the
    /// socket throttle read the result.
    pub server_data: Vec<(&'static str, util::json::Json)>,
    /// (channel prefix, handler): offered every `subscribe` whose channel starts with the prefix, then called for
    /// each subscriber after every tick (`ChannelFn`).
    pub socket_channels: Vec<(&'static str, ChannelFn)>,
    /// Game-slot JS (§10.2), one `mods/<mod>.js` module each, evaluated in every player VM after the game API; its
    /// exports may carry `customObjectPrototypes` and `customIntentTypes`.
    pub js_game: Vec<&'static str>,
    /// Scripts that run in the game client, in its page and with its globals (`GameRenderer`, `RENDERER_METADATA`,
    /// `angular`, …): how a mod changes what the client does past what `server_data` reaches. They arrive in
    /// `/api/version` (`serverData.features`) and run, each in a function of its own, every time the client connects,
    /// so a script must leave alone what an earlier run already changed. A mod that replaces the client's menu, as
    /// `mods/bare` does, removes them.
    pub client_js: Vec<&'static str>,
    /// The server's data directory (`--data`), where a mod keeps its files.
    pub data_dir: std::path::PathBuf,
    /// The host's Steam Web API key. Only a server other people can reach has one (`steam_web`).
    pub steam_key: Option<String>,
}
