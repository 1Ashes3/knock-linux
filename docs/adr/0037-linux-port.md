# 0037: Linux port

- **Status:** Proposed
- **Date:** 2026-11-10
- **Spec sections:** §2, §3 (platform scope), §7.1, §9 (jump), §13.2 (tooling), §13.4
- **VERIFY item:** none (verified by use on one desktop, see Evidence)

## Context

Knock is specified as a Windows 11 app. The user runs KDE Plasma on Wayland and wants Knock on Linux, kept as a fork of the original so the Windows code stays intact and a pull request upstream stays possible.

## Decision

Add a Linux backend behind the existing `trait Platform` (`platform/linux.rs`), selected with `#[cfg(target_os = "linux")]`. Windows code is not changed in behaviour.

- **Strip:** on Wayland desktops with layer-shell (KDE, Hyprland, Sway) the strip is a layer-shell surface above the panel; on X11 an always-on-top window; on GNOME Wayland there is no strip (tray and notifications only, and Settings says so).
- **Click-through:** an input region set from the hit regions, not a cursor poll (Wayland gives no global cursor). The strip thread polls every 250 ms on Linux and 33 ms on Windows.
- **Jump:** the §9.4 process chain from `/proc`, then KWin script (KDE), `hyprctl` (Hyprland) or `swaymsg` (Sway). Other desktops show the toast only.
- **Files:** config in `$XDG_CONFIG_HOME/knock`, logs in `$XDG_STATE_HOME/knock/logs`, files private (0600/0700).
- **Packaging:** an AUR package `knock-claude-git` (the name `knock` is taken by an unrelated Arch package) and an AppImage. Build paths are remapped out of the binary.
- **Fixed port stays** (§8.1): chosen at first run and then fixed, with the existing Repair flow if it can't bind. This is a known limit: another program can take the port while Knock is not running.
- **New runtime dependencies** (Linux only unless noted): `gtk`, `gtk-layer-shell`, `zbus`, `libc`, `wayland-client`, `wayland-protocols`. None is named in §13.2, so each needs the user's approval.

## Consequences

- SPEC.md needs a platform section for Linux (not edited yet; needs the user's approval).
- Windows is only type-checked in this port (`cargo clippy --target x86_64-pc-windows-msvc`); nobody has run it there.
- Untested on Hyprland, Sway and X11 (code written only).
- Autostart uses `~/.config/autostart/knock.desktop`; the `Exec` path is escaped for quotes, `$`, backticks, backslashes and `%`.

## Evidence

KDE Plasma 6 on Wayland, CachyOS: strip above the panel, knock on a real permission prompt, click jumps to the Konsole window, end to end with real Claude Code sessions. AppImage built and run by the user on the same machine (2026-11-10). A read-only review of the release files by a second agent found no personal paths or leaks.
