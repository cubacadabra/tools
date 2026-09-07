# Cubacadabra Tools

`cubacadabra` is the shared command-line toolbox for Cubacadabra projects.
It is intentionally small at the start and organized so new commands can be
added without putting build logic back into individual game repositories.

## Run

PYTHONPATH=src python3 -m cubacadabra --help

## Install

From this repository, install the CLI in an environment you control:

```sh
python3 -m pip install -e .
```

The command is then available as `cubacadabra`. Without installing, use
`PYTHONPATH=src python3 -m cubacadabra` from this repository.

## Commands

```text
cubacadabra [--version] COMMAND

Commands:
  build-game  Build a portable game package from a game project.
```

Build any compatible game repository from its project directory:

```sh
cubacadabra build-game ../first-game
cubacadabra build-game ../second-game --output ../second-game/build/package
cubacadabra build-game ../third-game --zip ../third-game/build/third-game.zip
```

By convention, a game project contains `manifest.json`, `src/main.luau`, and
an optional `assets/` directory. The manifest's `id` and SemVer `version`
become package metadata; legacy positive integer versions remain accepted.
Luau files can include other files with `-- @include "relative/path.luau"`
directives.

One-shot game audio is declared under `assets.audio` with an id, path, and
optional volume. The builder validates up to 64 package-local WAV files as
48 kHz, 16-bit PCM with one or two channels and a maximum size of 4 MiB each.

Run `cubacadabra build-game --help` for all options.

## Development

```sh
PYTHONPATH=src python3 -m unittest discover -s tests -v
PYTHONPATH=src python3 -m cubacadabra --help
```
