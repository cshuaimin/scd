# SCD

A tiny file manager focused on shell integration.

[![asciicast](https://asciinema.org/a/325485.svg)](https://asciinema.org/a/325485)

## Motivation

Have you ever typed `ls` after every `cd` command? In some "your most used command" surveys([reddit], [v2ex]), the `ls` command occupies a considerable amount.

[reddit]: https://www.reddit.com/r/linux/comments/6y98dm/what_are_your_most_used_command_line_utilities/
[v2ex]: https://www.v2ex.com/t/38674

Obviously you need a terminal file manager! But wait, it seems still inconvenient to switch from `ls` to some file managers after entering the directory ...

So here comes `scd`! `scd` is docked as a sidebar of your terminal so you don't have to open it every time.

Most importantly, the current directory of `scd` is synchronized with the shell. This means that it will update its file list when you `cd` in your shell, and if you enter another directory in `scd`, the shell will also automatically `cd` to it.

Moreover, it seems that it is too wasteful to display only the files in the current directory, so, as you think, I added some resource monitoring functions in `scd`. Hope you like it!

## Installation

### Cargo

```bash
cargo install scd
```

## Usage

`scd` is designed for sidebar, so you need `tmux` or `kitty` terminal to split an area for it.

In sidebar, run `scd` to open the main window. 

In your shell, you need to setup some hooks to send `scd` shell events:
```bash
scd fish-init | source
```
Currently only `fish` shell is supported. If you know how to setup same hooks in other shells, any contributions is welcomed!

## Keybinds

### Quit

`q`

### Move

- Up: `Up`/`k`/`Ctrl+p`
- Down: `Down`/`j`/`Ctrl+n`
- First: `Home`/`g`
- Last: `End`/`G`

### File operation

- Enter directory or open file: `Enter`/`l`
- Go to parent directory: `Esc`/`h`
- Delete file/directory: `d`
- Rename file/directory: `r`
- Mark files for copy/move: `Space`
- Copy marked files here: `p`
- Move marked files here: `m`

### Filter

- Toggle hidden files: `.`
- Enter filter mode: `/`

### Filter mode key bindings

- Move cursor: `Left`/`Ctrl+b`, `Right`/`Ctrl+f`, `Home`/`C-a`, `End`/`Ctrl+e`
- Move selection: `Ctrl+p`/`Ctrl+n`
- Edit: `Ctrl+u` to clear, `Backspace`/`Ctrl+h`, `Delete`/`Ctrl+d`,
- Exit filter mode: `Esc`/`Enter`

## Configuration how to open files

By default, `scd` opens file via `xdg-open`. It's recommended to configure some cli utilities to open file in the shell.

The configuration is a `YAML` file located at `~/.config/scd/open.yml`

A sample configure file:

```yaml
rs, py, go, js, html, css, c, cc, cpp, sh, fish: bat
toml, yaml, yml, json, ron, ini, conf, txt, md: bat
pdf: pdftotext {} - | less --quit-if-one-screen
```