# DOOM in the terminal

This example plays DOOM in your terminal, with PHP as the host. The game is [doom.wasm](https://github.com/jacobenget/doom.wasm), a build of DOOM as a single WebAssembly module with the shareware episode embedded. PHP runs its game loop, feeds it the keyboard and draws every frame with coloured half block characters.

## Running it

Install the extension with [PIE](https://github.com/php/pie). Until the first release is on Packagist, point PIE at the repository, which builds from source and needs Rust from [rustup.rs](https://rustup.rs):

```sh
pie repository:add vcs https://github.com/veewee/ext-wasm
pie install veewee/ext-wasm:dev-main
```

Download doom.wasm into `examples/doom/dist` and start the game:

```sh
examples/doom/download.sh
php examples/doom/doom.php
```

A large terminal window with a small font gives the sharpest picture. The picture scales to the window, also when you resize it.

## Controls

| Key | Action |
|---|---|
| Arrow keys | Walk and turn |
| W, S | Walk forward and back |
| A, D | Strafe |
| Space | Fire |
| E | Use: open doors, press switches |
| Enter, Esc | Menu |
| Tab | Map |
| 1 to 7 | Weapons |
| Ctrl+C | Quit |

A terminal normally only tells a program that a key was pressed, not that it was released. The example therefore counts a key as released once its auto-repeat stops, which makes a short tap last about half a second. Terminals that support the [kitty keyboard protocol](https://sw.kovidgoyal.net/kitty/keyboard-protocol/), such as kitty, WezTerm, Ghostty and foot, do report key releases, and the example uses them when available. The status line shows which mode is active.

Save games are written to `examples/doom/dist/saves`. There is no sound, because doom.wasm does not implement it.

## Without a terminal

`--ticks` runs the game without input or drawing, and `--screenshot` writes the last frame as a PPM image:

```sh
php examples/doom/doom.php --ticks=200 --screenshot=title.ppm
```

## Inner workings

`Game.php` holds the whole contract with the wasm module: ten imported functions for the clock, console messages, the framebuffer, WAD loading and save games, and the exports `initGame`, `tickGame`, `reportKeyDown` and `reportKeyUp`. The module draws into a 640x400 BGRA framebuffer in its own memory and calls `ui.drawFrame` with a pointer to it, which PHP reads with `Memory::read`.

`doom.php` calls `tickGame` 35 times per second, the rate DOOM was designed for. On a recent Mac a tick takes about 20 ms, and drawing a frame in a 160x50 terminal about 1.5 ms, because `Renderer.php` only writes the cells that changed since the previous frame. When the terminal falls behind, drawing skips frames, so the game itself keeps its speed.

## License

doom.wasm is GPL-2.0 and contains the DOOM shareware data, which id Software allows to be shared free of charge. For that reason it is downloaded rather than included in this repository. The PHP code of this example is MIT like the rest of ext-wasm.
