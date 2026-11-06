# Rust mods

Every crate in this folder is compiled into the server and registered automatically. A world uses a mod only when
it is enabled: `"mods": ["example"]` in the config, or `krebs serve --mods example`.

Each mod is a repository of its own, added here as a git submodule, and the core builds and runs without any of them.
`git clone --recursive` gets them all, and `git submodule update --init` gets them in a clone that has none.

| Folder | Repository | What it is |
| --- | --- | --- |
| `mods/bare` | krebs-bare, on [GitHub](https://github.com/DefaultO/krebs-bare) and [Gitea](https://gitea.defaul.to/DefaultO/krebs-bare) | What players see: the stripped client, lobbies, Battle Royale. `run.bat` enables it. |
| `mods/matchrun` | krebs-matchrun, on [GitHub](https://github.com/DefaultO/krebs-matchrun) and [Gitea](https://gitea.defaul.to/DefaultO/krebs-matchrun) | Runs inside a match's own server. `mods/bare` needs it. |
| `mods/matchrules` | krebs-matchrules, on [GitHub](https://github.com/DefaultO/krebs-matchrules) and [Gitea](https://gitea.defaul.to/DefaultO/krebs-matchrules) | Match rules and stats. Nothing enables it yet. |
| `mods/example` | krebs-example, on [GitHub](https://github.com/DefaultO/krebs-example) and [Gitea](https://gitea.defaul.to/DefaultO/krebs-example) | Shows every kind of registration. Copy it to start a mod. |

A clone without submodules leaves empty folders here, which is fine: the workspace does not list `mods/*`, and the
block in `crates/modlist/Cargo.toml` is empty in the repository. A mod becomes a workspace member through the line
`build.rs` writes there.

`run.bat` marks that file and `Cargo.lock` skip-worktree, because both then list the mods on your machine and neither
list belongs in a commit. Before changing either on purpose: `git update-index --no-skip-worktree <file>`, and put the
block back to empty before committing.

## Adding a mod

1. Copy `example/` to `mods/<name>/`, delete the `.git` file in the copy, which still points at krebs-example, and rename the
   package in its `Cargo.toml` (e.g. `mod-<name>`).
2. In `src/lib.rs`, export `pub static MOD` implementing `backend::mods::Mod`: a `Manifest` (the name used in
   `mods`, a version, the names of mods it needs loaded first) and `register`, which adds what the mod does to the
   `Registry`.
3. Build twice. The first build finds the new crate, adds it to `crates/modlist/Cargo.toml` and stops with
   `The mods folder changed`, because Cargo reads manifests before build scripts run. The second build compiles it
   in. `run.bat` does both.

To remove a mod, delete its folder and its line in the generated block of `crates/modlist/Cargo.toml` (Cargo
cannot load a manifest whose dependency folder is gone, so build.rs never gets to do it).

## What a mod can register

`backend::mods::Registry` (ARCHITECTURE.md §10.1):

* `engine`: engine hooks. Processor hooks (custom intents, object ticks, room and global stages), crons,
  world-generation hooks, main-loop stage events. Hooks are plain `fn`s; state kept between calls lives in statics.
* `routes`: HTTP routes, `fn(&Request, &ApiCtx) -> Response`. `ctx.published.get()` is the last published world;
  `backend::call(&ctx.commands, |w| …)` runs a change on the tick thread before the next tick.
* `server_data`: `config.backend` assignments as vanilla mods make them, `(path, value)` meaning
  `config.backend.<path> = value`: `welcomeText`, `customObjectTypes.<type>` (the client's side panel),
  `renderer.resources.<name>` / `renderer.metadata.<type>` (how the client draws a type), `socketUpdateThrottle`
  (ms between socket pushes). `/api/version` reports the result as `serverData`.
* `socket_channels`: `(prefix, fn(user, channel, &WorldView) -> Option<Json>)`. On `subscribe <channel>` with that
  prefix, `Some(data)` accepts (and sends `data` unless it is null), `None` declines; after every tick each
  subscriber gets the new result. `user:<id>…` channels reach a mod only from that user's own connection.
* `js_game`: game-slot JS, evaluated in every player VM after the game API as module `mods/<mod>.js`. Its exports
  may carry `customObjectPrototypes` (`[[objectType, name, opts], …]`, vanilla's `registerCustomObjectPrototype`
  arguments) and `customIntentTypes`.
* `data_dir`: the server's data directory, for files the mod keeps.

`mods/example` uses each of these. `krebs bench --mods <names>` runs a benchmark with mods enabled.

A mod's routes run before core's, so one can also replace or remove a core route, as `mods/bare` does. `mods/bare`, which
`run.bat` enables, strips the Steam client down to a Krebs landing page with lobbies and Battle Royale: no menu, top bar or other
pages, one `Leave server` button you hold down, and none of the endpoints those pages used. Edit `mods/bare/landing.html`, `landing.css` or
`bare.css` and reconnect to change how that looks. The world map and room view are off, not deleted: set `WORLD_VIEW` to true in `mods/bare/src/lib.rs` for them.

A match's room pick, spawn placement and live view are one map, `mods/bare/worldmap.js`: one camera over every room, drag to move and
scroll to zoom. Its camera comes from shardreplay, the frontend at screeps.defaul.to/replay. Its ground is chart work: flat terrain cut into
tiles, held rooms printed in their owner's colour (`sheets()` in a screen's rules names any others). `render.js` draws every object by the
game's own recipe with the game's sprites in `game-assets/`, with no glow, shadow, additive blend or idle pulsing, and below 48 px a room
each object is a point. The match's overview says who holds and who has reserved every room, so prints and hatches show at any zoom.
New sprites go in `SPRITES` in `src/lib.rs`, and colours handed to the map are `#rrggbb`. It only draws when
something changed, because a match usually runs on the machine watching it. `/api/lobbies/map` lists a mineral with its type as a fourth
field, so a room nobody has seen shows its letter.

`landing.html` is one design in four size bands, chosen by both window axes: a left rail, a stage that holds the map, and a panel with one
step in one fixed slot. With `KREBS_PAGES_DIR` set, as `run.bat` sets it, page files reload from that folder; without it the built-in copies are used. Fonts and routes need a rebuild. `MARK` in `landing.js` picks the logo in the rail; every logo and its generator is kept in `bare/logos/`.
The client's server card shows two things a server sets, and `bare` fills both in `/api/version`: `users` is who has a page open now, and `packageVersion` carries the open lobbies and running matches, on hover. The name, look and layout of the card are the client's.
Display, text and mono are Big Shoulders, Inter and JetBrains Mono, all SIL OFL (`mods/bare/licences/`), and any name the display files cannot set, Vietnamese among them, sets in Inter.

`mods/matchrun` runs only inside the world `mods/bare` starts for a match: a bot's CPU limit (20, plus 10 per GCL level, up to 300, on a
full bucket), no 5 MB limit on its code (`mods/bare` takes a whole folder, up to 10 MB), and `/api/matchrun/view`, which the landing page draws while a run plays.

`mods/matchrules` is meant for a match's private world, reading `<data>/rules.json`:
safe-mode rules and match stats. `mods/bare` does not enable it yet.

Mods may depend on the workspace crates only (`util`, `engine`, `backend`, `runtime`), like the rest of Krebs.
`cargo test -p mod-<name>` runs a mod's own tests.
