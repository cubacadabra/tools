# Cubacadabra Tools — Developer Preview 0.3

`cubacadabra` is the shared command-line toolbox for Cubacadabra projects.
It is intentionally small at the start and organized so new commands can be
added without putting build logic back into individual game repositories.

## Run

PYTHONPATH=src python3 -m cubacadabra --help

PYTHONPATH=src python3 -m cubacadabra create-game --title "The Wild West" --path /Users/aa/cubacadabra/examples

PYTHONPATH=src python3 -m cubacadabra build-game --source /Users/aa/cubacadabra/examples/the-wild-west --output /tmp/foo --zip ../the-wild-west.zip

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
  create-game Create a new starter game.
  build-game  Build a portable game package from a game project.
```

Create a new game from a title and a parent directory. The command creates a
directory named from the title, with a starter `manifest.json`, `src/main.luau`,
and empty `assets/audio/` and `assets/images/` directories:

```sh
cubacadabra --create-game --title "The Wild West" --path ~/games
# or: cubacadabra create-game --title "The Wild West" --path ~/games
```

Build any compatible game repository from its project directory:

```sh
cubacadabra build-game ../first-game
cubacadabra build-game ../second-game --output ../second-game/build/package
cubacadabra build-game ../third-game --zip ../third-game/build/third-game.zip
```

The starter created above follows the standard `src/main.luau` and `assets/`
layout. It can be built by pointing `--source` at the project directory; the
CLI selects `src/` and finds its `manifest.json` automatically:

```sh
cubacadabra build-game --source ~/games/the-wild-west --output ../the-wild-west \
  --zip ../the-wild-west.zip
```

By convention, a game project contains `manifest.json`, `src/main.luau`, and
an optional `assets/` directory. The manifest's `id` and SemVer `version`
become package metadata; legacy positive integer versions remain accepted.
Luau files can include other files with `-- @include "relative/path.luau"`
directives.

Portable SDK helpers use the same explicit syntax with a reserved namespace:

```luau
-- @include "@cubacadabra/shared-state-v1.luau"
-- @include "@cubacadabra/disclosure-v1.luau"
```

`CubaSharedState` v1 owns bounded intent queuing, compare-and-set retries,
conflict rebasing, and reconnect snapshots. Games provide their own initial
state, validator, reducer, and optional change callback, so neither the SDK nor
the backend needs to know game-specific rules.

See [Shared state SDK v1](docs/shared-state-v1.md) for the reducer contract and
lifecycle API. [Disclosure SDK v1](docs/disclosure-v1.md) provides a generic
tap-to-reveal controller while each game continues to own its HUD composition.

The complete preview contract is in the
[Cubacadabra Game Developer Guide](docs/cubacadabra-game-developer-guide-preview-0.3.md).

One-shot game audio is declared under `assets.audio` with an id, path, and
optional volume. The builder validates up to 64 package-local WAV files as
48 kHz, 16-bit PCM with one or two channels and a maximum size of 4 MiB each.

Large game-owned effect libraries may live in a separate source file:

```json
{
  "effects": { "source": "effects.json" }
}
```

The builder validates the relative path and inlines the effect library into the
portable package manifest. Runtime clients still receive one self-contained
manifest and do not need filesystem or include behavior.

Run `cubacadabra build-game --help` for all options.

## Development

```sh
PYTHONPATH=src python3 -m unittest discover -s tests -v
PYTHONPATH=src python3 -m cubacadabra --help
```
