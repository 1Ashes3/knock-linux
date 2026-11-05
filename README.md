# Knock

**A little animal knocks on your taskbar when Claude Code needs you.**

Knock shows each running [Claude Code](https://docs.claude.com/en/docs/claude-code/overview) session as a small animal sitting on top of your Windows taskbar, near the clock. Calm sessions stay still. When a session needs you, for example with a permission prompt or a question, its animal steps up and knocks on the glass, and Windows shows one notification. Click the animal to jump to that session's window.

![Knock's strip on a dark taskbar: a sleeping fox, a working cat, an owl that stopped with an error, a cat that needs permission, and a rabbit and a fox holding up "done" cards](docs/images/strip.png)

> **Beta.** This is v0.3, a public test version. It works, but expect rough edges, and please [tell us how it went](#feedback).

## What it does

- **One animal per session**: a fox, rabbit, cat or owl. Or switch to plain colored labels in Settings.
- **Knocks only when it matters.** When a session asks for permission or has a question, its animal steps up and knocks and you get **one** notification. If you still haven't answered after 5 minutes, you get one reminder. A session that stopped with an error shows a red mark and notifies you once. One that seems stuck paces, and notifies you only if you turn that on. Finished sessions hold up a "done" card and never interrupt you.
- **Hover** an animal to see the project, what it's waiting for and for how long.
- **Click** an animal to bring its terminal or editor window to the front.
- **More than 5 sessions?** The extras wait in a little den that shows how many are in each state. A session that needs you steps out.
- **While you were away.** Back after 10 minutes or more? A card lists what finished, failed or needs you.
- **Pause and quiet hours** for notifications, from the tray icon or Settings. The animals still show the state.
- Respects Windows **Animation effects**, works with Narrator, and handles 100-200% scaling, auto-hiding taskbars and multiple monitors.

![The "While you were away" card above the strip](docs/images/away.png)

## Requirements

- **Windows 11.**
- **The Claude Code CLI running natively on Windows**, in PowerShell, Command Prompt or Git Bash, in any terminal or editor (Windows Terminal, VS Code's terminal, Cursor, JetBrains and others).
- **Not supported:** Claude Code inside WSL, the Claude Code panel in VS Code, and the Claude desktop app.

## Install

1. Download `Knock_0.3.0-beta.1_x64-setup.exe` from the [Releases page](https://github.com/bandaranayake/knock/releases).
2. Run it. It installs for your user only and never asks for administrator rights.
3. Follow the short setup. It takes under two minutes, with no command line and no file editing.

### The installer is unsigned

Knock is a free open-source project and its installer isn't code-signed yet. So:

- **"Windows protected your PC"** may appear. Click **More info**, then **Run anyway**.
- **Smart App Control blocks unsigned apps entirely.** Check it first: open **Windows Security → App & browser control → Smart App Control settings**. If it says **On**, Windows won't let Knock run. We don't suggest turning it off just for Knock. Read [Microsoft's explanation](https://support.microsoft.com/en-us/topic/what-is-smart-app-control-285ea03d-fa88-4d56-882e-6698afdb7003) first.

If you'd rather not run an unsigned installer, you can [build Knock yourself](#build-from-source).

## What setup changes

Knock learns about your sessions through Claude Code **hooks**: small rules in Claude Code's settings file that send an event to Knock when something happens.

- Setup **shows you exactly what it will add** to `%USERPROFILE%\.claude\settings.json` (or the folder in `CLAUDE_CONFIG_DIR`) and writes nothing until you click **Confirm**.
- It **backs up** the file first, to `%APPDATA%\Knock\backups`, and keeps every other setting you have.
- Sessions that are already open connect within about 10 seconds. No restart needed.
- **Settings → Remove Knock from Claude Code** takes the hooks out again with one click.

If Knock isn't running, Claude Code carries on normally. The hooks fail quickly and silently.

## Privacy

- Knock **never** reads, shows, stores or logs your conversations, prompts, code, tool inputs or outputs. It keeps only what it needs to show state: session id, project folder name, event type, tool name, times, and which window to jump to.
- **No telemetry. No accounts. No internet connection.** Knock only listens on your own computer (`127.0.0.1`), protected by a random token.
- Logs live in `%LOCALAPPDATA%\Knock\logs`, are kept for 7 days, and never contain message content or tokens.

## Uninstall

1. Open Knock's **Settings** (tray icon) and click **Remove Knock from Claude Code**.
2. Uninstall **Knock** from **Windows Settings → Apps → Installed apps**.

Do step 1 first. If you skip it, Knock's hooks stay in Claude Code's settings. They're harmless (they fail silently), but they're clutter.

## Known limitations in this beta

- **No auto-update yet.** To get a newer beta, download and install it over the old one. Your settings are kept.
- In Windows Terminal, a click brings the Windows Terminal window forward but can't pick the exact tab.
- The installer is unsigned (see above).

## Feedback

This beta is here to find out whether people who didn't build Knock can install and use it without help. Please:

- **[Send a setup report](https://github.com/bandaranayake/knock/issues/new/choose)**, even if everything went fine. It takes a minute.
- **[Report a bug](https://github.com/bandaranayake/knock/issues/new/choose)** if something looks wrong.

**Never paste Claude Code conversations, prompts, code or tokens into an issue.**

## Build from source

You need Rust (the version in `rust-toolchain.toml`), Node.js and pnpm (versions in `package.json`), and the [Tauri prerequisites for Windows](https://v2.tauri.app/start/prerequisites/).

```
pnpm install --frozen-lockfile
pnpm build
```

The installer appears in `target\release\bundle\nsis\`. For development, `pnpm dev` runs the app, and `pnpm check` runs the formatters, linters and tests.

## How it's built

Knock is a [Tauri 2](https://v2.tauri.app/) app: a Rust core with a small local server and a pure state engine, plus a TypeScript overlay drawn with PixiJS. It's built entirely with AI: Claude Code wrote all the code and drew all the animals, and a human made the decisions and did the testing. [`SPEC.md`](SPEC.md) is the full specification, and [`docs/adr/`](docs/adr/) records every decision along the way.

## License

[MIT](LICENSE)
