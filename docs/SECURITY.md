# Security

What you take on when you run a Krebs server, and what a player takes on by joining one. It is written for hosts. The last
section is for whoever maintains the V8 library.

The short version: uploaded bots are code from other people, running on your PC, and only V8 stands between them and your
files. Run a server for people you trust, and do not run a public one.

## What a bot can reach

A bot is JavaScript, and WebAssembly if its author wants, running inside Krebs in a V8 virtual machine of its own.

It can use the game's API, its own Memory and segments, and whatever else the game gives every player. Krebs gives it no
function that opens a file, a network connection or a program.

What holds a bot to that is V8, and nothing else:

- **V8 is the only barrier.** Krebs does not run bots under a restricted account, in a container, or in any other kind of
  jail.
- **V8's own sandbox is off.** In Chrome, that sandbox is a second wall that contains a memory bug in V8. The library Krebs
  uses is built without it.
- **So a bug in V8 could mean code running as your Windows user.** If a bot can reach a memory bug in V8, it may be able to
  run programs on your PC with everything your account can touch: this folder, your Steam Web API key, your documents.
  Nobody has shown such a bug in the V8 Krebs uses, but V8 gets them regularly, and the risk grows as this library falls
  behind V8's fixes.

With the bare mod, players' bots run only in match servers. A match server is a process of its own, so a bot that crashes
or stalls one ends that match and not your server. It still runs as your Windows user, so this does not help against a bug
in V8.

If you want a wall that does not depend on V8, run Krebs under a separate Windows account that holds nothing else.

## What limits a bot

Each bot has a limit on its CPU per tick, a hard timeout of 5 seconds on a run, and a memory limit of 256 MB plus the
terrain, which covers its heap and its array buffers. Its output is capped at 32 MB a tick.

**WebAssembly memory is not counted.** V8 allocates it without going through the per-bot memory limit, and Rust and .NET
bots need it, so it cannot simply be removed. One bot was measured committing 2 GiB in one tick this way, and it can carry
on until the PC runs out, at which point the next failed allocation ends the whole process.

What bounds it is the process around the bot. A match server has a memory limit of its own, so a bot that takes too much
ends its own match. A server that runs bots in its main world, which means any server without the bare mod, has no such
limit: only the PC's memory stops it.

A very large output is also cheap for a bot. Handling one object of 1 million keys costs the server about 0.3 seconds a
tick, and none of it is billed to the bot. That cost grows with the size of the output, up to the 32 MB cap, so a bot can
slow a world this way but not freeze it.

## Known limit: new module names on a plain server

The server keeps every distinct property name it has seen until it restarts, so that memory cannot grow without end it takes
at most 131072 new names from outside. Once those are gone, an upload with a module name the server has not seen fails
with `this server cannot take more new module names until it restarts`. Bots that are running carry on, and uploads that use
names already seen still work. A restart clears it.

On a plain server, the kind with no bare mod, anyone who can reach it and make an account can use all of them: about 32
uploads of 4096 new names do it, and the sign-up route takes modules too, so as many sign-ups do it with no account to
begin with. A plain server that listens only on 127.0.0.1 can be reached that way only by programs on that PC.

With the bare mod this does not apply. Players cannot upload to the main world, and each match server starts empty with
far more room than its seats can use.

## Joining someone else's server

A player who joins a server trusts its host with their PC, not only with their sign-in.

- The Screeps client runs code that a server sends it when it connects, which is how Krebs shows its lobbies at all. A
  dishonest host could use that to run programs on the player's PC, as that player. Nothing a server does can prevent this,
  as it is how the stock client works. Only a patched client or launcher could.
- That code stays in the client after the player leaves, so a dishonest host can also see the sign-in the player uses for
  the next server.
- Nothing is encrypted, so anyone on the network path between the player and the server can do the same.
- The host sees everything on their own server: every player's bot code and Memory.

So: only join servers of people you trust, and tell people that before they join yours.

## For maintainers

### Following V8 security releases

Krebs pins V8 exactly: the crate `v8` `=152.2.0` in `crates/runtime/Cargo.toml`, which is V8 15.2.124.1
(`vendor/v8/v8/include/v8-version.h`), and a library pinned by tag and sha256 in `tools/v8-download.bat`. Nothing updates
these or says when they fall behind, so a person has to look.

1. Once a week, and before every release, read the Stable Channel Update posts on
   <https://chromereleases.googleblog.com/> and note every fix listed under V8.
2. Compare them with the pinned version. Chrome's posts name the Chrome version, and V8's major and minor numbers follow it,
   so V8 15.2 is Chrome 152.
3. See whether a newer `v8` crate has the fix, on <https://crates.io/crates/v8> and in the releases of
   <https://github.com/denoland/rusty_v8>.
4. If a bug a bot can reach is public and the pinned version does not have the fix, rebuild as below. Until the new library
   is out, tell hosts to run only bots they trust.

Last looked on 2026-11-13. The newest `v8` crate was still 152.2.0, so there was nothing to move to. From the first update
after Chrome 152 went stable (152.0.7977.75, 2026-09-01) to 157.0.8103.78 (2026-11-11), Chrome's Stable Channel Update
posts for desktop list 92 fixes in V8, 73 of them rated High, and none marked as used in attacks. Which of them the pinned
V8 needs was not worked out: some are for code newer than it. Take it that some apply, and keep to bots you trust.

### Rebuilding and republishing the library

These steps come from `tools/v8-ptrcomp.ps1`, `tools/v8-download.bat` and `vendor/v8`.

1. **Pick the version.** Change `v8 = "=152.2.0"` in `crates/runtime/Cargo.toml`.
2. **Update `vendor/v8`.** It is that crate without its V8 tree. Carry over Krebs's changes, which are marked `chitin:` in
   `vendor/v8/build.rs` and `vendor/v8/src/binding.cc`: a cage per isolate, and a version string of its own. A third, in
   `vendor/v8/src/isolate.rs`, only renames a call that newer Rust warns about, and can go once the crate has done the
   same. Compare it with the published crate of the old version first, in case anything else has changed.
3. **Update the pins in `tools/v8-ptrcomp.ps1`.** The crate's address and sha256; the `icudtl.dat` commit and sha256, and the
   `chromium_crates_io` commit and digest, both at the commits the new `v8/DEPS` pins. Check that `tools/v8-ptrcomp.patch`
   still applies. A changed patch makes the script extract the sources again.
4. **Record a new builtins profile.** `tools/v8-ptrcomp.profile` is tied to one V8 and its GN args, and a changed builtin
   aborts the build. `docs/ARCHITECTURE.md` section 2.9 has the recipe. Set `v8_enable_builtins_profiling` in `GN_ARGS` while
   recording, and the script then leaves the old profile out.
5. **Record a new C++ profile.** `tools\v8-ptrcomp.profdata` holds the counts V8's C++ is compiled with, and is worth about
   a tenth of a tick. Old counts do not stop a build: the compiler drops those that no longer fit and says nothing, so the
   gain goes quietly. `docs/ARCHITECTURE.md` section 2.9 has the recipe: a build with `GN_ARGS=chrome_pgo_phase=1` in a
   target folder of its own, a recording, and a merge into `tools\v8-ptrcomp.profdata`. This is a second 40 minute build.
   When there is no time for it, as with a security fix, build with the old file and record later.
6. **Build.** Run `tools\v8-ptrcomp.ps1`, as its first lines describe. It takes about 40 minutes at `-j 4` the first time,
   plus about 3 GB of toolchains that the build downloads. It uses the `tools\v8-ptrcomp.profdata` that is there, and
   warns that it is not the published one, which is right after a new recording. The library comes out at
   `target\ptrcomp\release\gn_out\obj\rusty_v8.lib` and its bindings at `target\ptrcomp\release\gn_out\src_binding.rs`.
7. **Keep the bindings.** Copy `src_binding.rs` over `vendor/v8/gen/src_binding_ptrcomp_release_x86_64-pc-windows-msvc.rs`,
   which is what a build uses when it downloads the library.
8. **Test.** Set `RUSTY_V8_ARCHIVE` and `RUSTY_V8_SRC_BINDING_PATH` to those two files and run
   `cargo test --workspace --release --features runtime/ptrcomp -j 4 --no-fail-fast`. A new V8 can also add a new way to make
   memory outside the allocator, as resizable buffers did, and these tests only know the old ways. Read the release notes
   for anything new on `ArrayBuffer` or `WebAssembly.Memory`, and add it to `LOCK_BUFFERS` in `crates/runtime/src/host.rs`.
9. **Publish.** Gzip the library as `rusty_v8.lib.gz` and take its sha256: the pin is the hash of the `.gz`. Gzip the
   profile as `v8-ptrcomp.profdata.gz`, whose pin is the sha256 of the file before it was gzipped. Put those same two files
   in a release with a new tag on both GitHub and Gitea, at `DefaultO/krebs/releases/download/<tag>/`.
10. **Point the build at it.** Change `V8_TAG` and `V8_SHA256` in `tools/v8-download.bat`, `$countsTag` and `$countsSha` in
    `tools/v8-ptrcomp.ps1`, and the tag in the test commands and under "The fast V8" in `README.md`.
11. **Release Krebs,** and tell hosts to update.
