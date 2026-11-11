# Fork routine

This repository is a fork of [Zed](https://github.com/zed-industries/zed). A
downstream project uses its GPUI crates (`gpui`, `gpui_platform`, `gpui_wgpu`,
`gpui_web`, `gpui_linux`) as git dependencies. Only those crates are built
downstream; the rest of Zed is carried unchanged.

## Branches

- `main` mirrors upstream Zed: it only ever fast-forwards to an
  `upstream/main` commit and carries no commit of its own.
- `lunatic` is `main` plus the patch series below, in order, as one linear
  run of commits. It is rebuilt by rebasing, never by merging, so the series
  stays a reviewable list. Every patch is written to be offered upstream: one
  purpose, Zed's commit style (`crate: Capitalized summary`), no
  downstream names.
- Work on the series happens on a topic branch cut from `lunatic` and replaces
  `lunatic` once it is built and verified.
- The last two commits on `lunatic` are fork-only and never offered upstream:
  this file's commit, then the notice commit, which adds `NOTICE` and marks
  every file the series changes with the line
  `// Modified in the 620nm/zed fork; see NOTICE.` as its first line.
- When a sync or a new patch touches a file that does not carry the line
  yet, the line is added in the notice commit; a file no patch touches any
  more loses it there. Rebasing puts the notice commit last again.

## The series

| # | Purpose | Crates |
|---|---|---|
| 1 | Style the safe-area probes through CSSOM, so a page whose CSP refuses inline style attributes still reads the insets | `gpui_web` |
| 2 | `PaintSurface` carries a `SurfaceSource`; `SurfaceSource::Custom(CustomSurface)` is an opaque, renderer-specific producer; `surface()` on every platform | `gpui`, `gpui_apple` |
| 3 | `WgpuSurfaceProducer`: the wgpu renderer asks it for a texture each frame and draws it in paint order; RGBA/sRGB kind in `fs_surface`; `SurfaceParams` padded to 48 bytes for WebGL2 | `gpui_wgpu` |
| 4 | Custom surfaces honor the element opacity they are painted with | `gpui`, `gpui_wgpu`, `gpui_apple` (test) |
| 5 | The producer gets full bounds, visible bounds and a texture size fitted to the device limit; nothing visible, nothing produced | `gpui_wgpu` |
| 6 | The producer records into its own encoder inside a validation error scope, submitted before GPUI's frame; a failing producer loses only its surface and stops being called after three failing frames | `gpui_wgpu` |
| 7 | Headless tests: order, clipping, sRGB bytes, visible-only production, opacity, failure isolation, target fitting | `gpui_wgpu` |
| 8 | `PhysicalKey` (W3C `KeyboardEvent.code` names) and `physical_key` on `KeyDownEvent`/`KeyUpEvent`; existing constructors pass `None` | `gpui`, `gpui_linux`, `gpui_macos`, `gpui_windows`, `gpui_web`, Zed crates with key-event literals |
| 9 | Linux backends report physical keys from the XKB keycode; X11 uses detectable auto-repeat and reports repeats as `is_held` | `gpui_linux` |
| 10 | The browser backend reports physical keys from `KeyboardEvent.code` | `gpui_web` |
| 11 | `Window::is_key_pressed` / `Window::pressed_keys`, emptied on deactivation | `gpui` |
| 12 | The browser backend releases a key let go of after focus moved elsewhere in the page | `gpui_web` |
| 13 | Gate the `DisplayId` import to test and `test-support` builds | `gpui` |

`git log --oneline main..lunatic` lists them; this table is updated in the
same change as the series.

## Syncing with upstream

1. `git fetch upstream`, then fast-forward `main` to the chosen
   `upstream/main` commit (`git merge --ff-only <commit>`).
2. On a topic branch from `lunatic`:
   `git rebase --onto main <old main> <topic branch>`, and resolve conflicts patch by patch, keeping each patch's purpose.
3. Run `cargo fmt --all` at each patch that conflicted, and keep every patch
   building on its own (see below).

Where conflicts are likely:

- Patch 8 touches every `KeyDownEvent`/`KeyUpEvent` literal in Zed. A new
  literal upstream fails to compile rather than conflicting: add
  `physical_key: None` to it in patch 8.
- Patches 3-6 sit in `gpui_wgpu/src/wgpu_renderer.rs` around `record_frame`
  and the `PrimitiveBatch::Surfaces` arm, and in `shaders.wgsl`'s surface
  section. Upstream renderer refactors land here.
- Patch 2 changes `PaintSurface`; upstream changes to the macOS surface path
  (`gpui/src/elements/surface.rs`, `gpui_apple/src/metal_renderer.rs`)
  conflict here.
- Patches 10 and 12 sit in `gpui_web/src/events.rs`'s key handlers.
- Patch 9's evdev table is self-contained; the X11 key handlers around it
  change upstream from time to time.

If upstream removes the surface slot or the wgpu renderer, stop and decide
before porting the series.

## Verifying a sync

Run each command through the machine's build queue, from the fork root:

```sh
cargo fmt --all --check
cargo check -p gpui -p gpui_wgpu -p gpui_linux -p gpui_platform --tests
cargo test -p gpui_wgpu --lib
cargo test -p gpui --lib -- pressed_keys code_names_round_trip
```

A pass: no diff from rustfmt, no warnings, every test green (the
`gpui_wgpu` headless tests need a Vulkan or GL adapter). Check every patch on
its own with the `cargo check` line above, for example with
`git rebase --exec '<that command>' main`.

Then build the consumer against the fork checkout (path dependencies on
`crates/gpui*`), natively and for the browser:

```sh
cargo build --release -p <consumer>
RUSTFLAGS=<consumer's wasm flags> cargo build --release -p <consumer> \
  --target wasm32-unknown-unknown   # single-threaded; then wasm-bindgen --target web
```

A pass: both build with no warnings from the fork's crates; the consumer's
own lanes (key input on Wayland, X11 and the browser; its custom surface on
WebGL2 and WebGPU) behave as before; `cargo tree -d` shows one `wgpu` and
one `naga`.

macOS and Windows backends cannot be built on Linux; a sync that touches
`gpui_apple`, `gpui_macos` or `gpui_windows` needs a check on those machines.

## Known limits

- A key held when focus moves into an embedded frame of the page is released
  only when the window deactivates: the frame receives the `keyup`, and the
  top window does not see it.
- On browser WebGPU the validation result of a producer's frame arrives
  later. Until it does, the texture the producer returned is drawn, and an
  invalid one can fail GPUI's own frames for those few frames.
- The Metal renderer skips custom sources and does not apply surface
  opacity; the DirectX renderer draws no surfaces.
- The macOS and Windows backends are not built from Linux; patches 2, 4 and
  8 touch them mechanically.
- On context or device loss, a producer must compare the device it is
  handed and rebuild its resources; GPUI's own recovery is unchanged by the
  series.

## Moving the consumer's pin

The consumer depends on this repository by git with `branch = "lunatic"`; its
`Cargo.lock` records the exact commit. To move it, push the new `lunatic` and
run `cargo update -p gpui` in the consumer, so the lock names the new commit.

GPUI and the consumer must share one `wgpu`: the consumer takes `wgpu` from
the same git revision that this repository's root `Cargo.toml` declares. A
sync that moves that revision moves the consumer's `wgpu` (and its `naga`
patch) in the same change as the pin.
