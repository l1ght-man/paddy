---
name: grind
description: Autonomous build mode for paddy. Keep working through the milestones without stopping to ask permission, until everything in the spec is done or a real blocker hits. Use when asked to grind, keep going, finish it, or work until done.
---
Spec: `~/.claude/projects/-home-motaz/memory/project_paddy.md` (milestones 1-5). Work out what's already done from the code and tests, not from memory or this file.

## Loop
1. Find the first unfinished milestone. Build it fully: code, tests, `cargo clippy`, `cargo test`.
2. Actually run it. Core: tests pass. UI: launch it (see `run` skill) and confirm it opens and behaves; don't call UI work done off a green compile.
3. Fix what you find. Small refactors needed to make the milestone work are in scope.
4. Update the `run`/`release` skills if the build layout changed.
5. One-line status, then go straight into the next milestone. No "want me to continue?", no "say the word".

## Don't stop for
- Naming, crate choices, layout, code style, test design, or anything the spec or a sensible default settles. Pick, note it in one line, move on.
- A milestone finishing. That's a checkpoint, not an exit.
- Warnings, flaky first builds, missing crates. Fix them.

## Do stop (and say why in a few blunt lines) when
- Every milestone in the spec is done and verified.
- A decision is genuinely the user's: it changes scope, contradicts the spec or saved preferences, or is a real product tradeoff. Ask once, with a recommendation, then wait.
- You need something only they can do (sudo apt install, logging in, a GUI they must look at). Give the exact command.
- The same failure survives 3 different real fixes. Report what you tried.
- The permission layer or auto-mode classifier blocks an action. Don't rewrite the action to dodge it and don't try side channels. Say what was blocked and carry on with anything that doesn't need it.

## Honesty
- Report failures and skipped steps as they are. "Compiles" isn't "works". If something wasn't run, say it wasn't.
- No inflated claims, keep messages short and casual.
- No git commits, no publishing, no deleting existing user files unless asked.
