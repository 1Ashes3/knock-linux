# 0036: Beta installers are built by a manual GitHub workflow

- **Status:** Approved by user on 2026-11-07
- **Date:** 2026-11-07
- **Spec sections:** §13.4 (CI), §13.5, §11.9, §11.10, §14 (Phase 6)
- **VERIFY item:** none

## Context

§13.4 plans `release.yml` for Phase 6: on a `v*` tag it builds the installer, signs the update package and uploads to a draft release. For the public beta (ADR 0035) the user asked to have GitHub build the installer now instead of building it on their own PC. The updater doesn't exist until Phase 6.

## Decision

Add `.github/workflows/release.yml` now, as a manual workflow:

- It runs only when the user clicks **Run workflow**, and only from `main`.
- It builds the NSIS installer on a GitHub Windows runner, from the pinned toolchains and lockfiles.
- It creates a **draft pre-release** for the version in the app's `Cargo.toml` (tag `v<version>`, created when the user publishes). The installer and its SHA-256 checksum are attached, and the notes come from `docs/releases/v<version>.md`.
- It never publishes. The user reviews the draft and publishes by hand (§13.4).
- It uses no secrets beyond GitHub's built-in token. Nothing is signed: there is no updater key yet (§11.9).
- Two jobs. The build job has read-only access. A separate release job is the only one that can write: it checks out no code, takes the installer from the build job's artifact, and only runs the GitHub CLI. This adds one first-party action, `actions/download-artifact`, pinned by SHA (lead approval).

Phase 6 extends this workflow with the `v*` tag trigger, updater signing and `latest.json`.

## Consequences

- SPEC.md §13.4 `release.yml` row: until Phase 6, a manual run from `main` builds the installer into a draft pre-release (ADR 0036).
- Release notes live in `docs/releases/`, written by the lead.
- The user no longer builds installers locally.

## Evidence

none
