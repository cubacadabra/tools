## Overall assessment

**The biggest remaining weakness is consistency between the parts of the platform—not a shortage of features.** A developer can encounter different SDK code in their editor and their build, multiplayer actions that repeat or disappear, and packages that pass one stage but fail in another.

I found **10 areas worth prioritizing**, including four behaviors reproduced in isolated tests. My first fixes would be **the failing compatibility build, shared-state operation semantics, package integrity, and SDK resolution**.

### Code reviewed

These are the current snapshots I inspected through the GitHub connector, including changes **after** your linked `662c18a` fix:

| Repository    | Reviewed commit | Main areas inspected                                                                     |
| ------------- | --------------- | ---------------------------------------------------------------------------------------- |
| `tools`       | `65f737d`       | Module bundler, SDK, project creation, asset validation, compatibility workflow.         |
| `rust`        | `78c59fe`       | Dependencies, shared-runtime architecture, authority boundary and documentation.         |
| `web`         | `aa638ed`       | Package loading, local-game routing, game synchronization.                               |
| `studio`      | `87bef19`       | Project-loading entry point, shared-client integration, documented development workflow. |
| `ios_app`     | `33c6ccf`       | Package selection, caching, checksum verification, image loading and atlas construction. |
| `android_app` | `10c32a3`       | Package selection, caching, checksum verification and asset loading.                     |
| `first-game`  | `28f8813`       | Shared round state, reducers and interaction intents.                                    |
| `second-game` | `6079c33`       | Relay state, interaction intents and round transitions.                                  |
| `third-game`  | `c880cf7`       | Capability Probe, shared counter and lifecycle handlers.                                 |
| `examples`    | `db9d46b`       | Example-project organization, Wild West source and compatibility coverage.               |

**Scope limitation:** `cubacadabra/backend` returned **404 through the connector**. That means its implementation was not available for this review—not that it necessarily does not exist. I have not independently verified server-side authorization, publishing, payments or persistence.

This was a source-and-CI review, with small executable reproductions—not a full device or production-backend test. No repositories were modified.

---

## 1. The compatibility build is failing before it tests compatibility

**Priority: High. Observed in CI.**

The `tools` workflow run for the reviewed head—**run `34757982252`**—failed while building the web renderer. Both the actual package-compatibility check and the subsequent Studio tests were **skipped**. This is not merely a missing-test recommendation; the new protection is currently failing before it reaches its intended checks.

The job log reported a `wasm-bindgen` schema-version mismatch. The underlying configuration explains how that happens: the workflow installs CLI version `0.2.127`, while the latest Rust manifest permits `wasm-bindgen = "0.2"` and the repository explicitly ignores `Cargo.lock`. I checked the dependency configuration at the newer Rust head, not just the Rust revision used by the failed job.

**What a developer experiences:** “I followed the setup instructions on a clean machine, and your own build does not work.”

**What I would change:** Establish a reproducible platform toolchain: committed lockfiles for these application/workspace builds, a single source for the `wasm-bindgen` crate/CLI version, and `--locked` builds. Separately, record the exact cross-repository SHAs used for a supported platform release. Testing against floating `main` can remain an additional integration check, but should not define the only known-good configuration.

**Acceptance test:** A clean checkout of the recorded repository set successfully builds and reaches every compatibility-test stage.

---

## 2. Shared-state completion semantics are too fragile for ordinary game operations

**Priority: High. Reproduced in isolation.**

In `tools/src/cubacadabra/sdk/shared-state.luau`, an in-flight intent is considered completed when applying its reducer to the newly received state returns `nil`.

That is effectively asking:

> “Does this operation still appear necessary?”

It is **not** asking:

> “Was this particular operation accepted?”

Those are different questions.

This can work for a shared objective such as “make this charm collected.” It is much harder to use correctly for “add one visit,” “award ten coins,” or “consume one item.”

I reproduced two consequences using the queue methods with a simulated transport:

**An ordinary additive reducer repeats indefinitely.** With `score = state.score + 1`, one dispatched intent produced five acknowledged increments and a sixth proposal; the original intent remained queued.

**The current Capability Probe avoids repetition by potentially losing distinct operations.** Its reducer records `expectedVisits` and returns `nil` whenever the shared count changes. When two clients independently visit a sensor while both observe zero, the first accepted update changes the count to one; the other client drops its pending visit. For a counter intended to count every visit, one valid increment is lost.

**What a developer experiences:** “Your networking helper works for the sample’s shared switches, but I cannot safely adapt it into a score or inventory system.”

**What I would change:** Explicitly distinguish **coalescing desired-state intents** from **distinct operations**. Distinct operations need stable operation IDs, acknowledgment/deduplication semantics and clear outcomes such as accepted, rejected or expired. Merely capturing the old counter value does not solve that problem.

**Acceptance test:** Two distinct increments eventually produce two increments; retransmitting the same operation produces only one. Include the case where acceptance succeeds but the acknowledgment is lost.

---

## 3. Package integrity checks stop before the assets

**Priority: High. Confirmed in both mobile loaders.**

The new package descriptor and checksums are a useful improvement, but the mobile implementations do not yet enforce the complete package identity.

Both iOS `verifyPackageDescriptor` and Android `verifyPackageDescriptor` check the hashes of **`manifest.json` and `game.luau`**. Their image-loading paths subsequently read or download image bytes without checking those bytes against the descriptor’s asset hashes.

There is a related consistency problem in the **cached/default-package path**: mobile caches retain the descriptor, manifest and script, but reload assets using the game’s remote base URL. The cached package does not preserve an immutable release-specific asset location. Thus, if that location is updated in place, an older cached script can receive newer assets.

**What a developer experiences:** “The game version is unchanged, but its textures changed,” or “the package passed verification even though an asset is wrong.”

**What I would change:** Treat a release as one immutable unit. Resolve a release ID or content hash first, load every file relative to that immutable release, verify every declared file, and promote a cache entry only when the required release contents are available and valid. A cached release should not silently source its assets from a moving “latest” location.

Checksums establish consistency with the descriptor; they should not be confused with authentication of the publisher.

**Acceptance test:** Cache version A, publish version B with different assets under the same logical filenames, then launch A. The client must load a coherent A or a coherent B—never a mixture.

---

## 4. The editor and builder resolve the SDK from different places

**Priority: High. Confirmed in source.**

`create_game()` copies the SDK into:

```text
<project>/.cubacadabra/sdk
```

It then writes `.luaurc` so the editor’s `@cubacadabra` alias resolves to that project-local copy.

But `_read_sdk_module()` in `game_builder.py` resolves SDK imports from:

```text
<installed CLI package>/sdk
```

It does not use the project’s copied SDK.

Consequently, upgrading the CLI can change what gets bundled while the editor continues to show the old implementation. Editing the project-local SDK can also change what the developer reads without changing what runs.

**What a developer experiences:** “Go to definition shows one implementation, but my game behaves like another.”

This is especially damaging when debugging a helper such as shared state: the developer may be investigating code that is not actually in their package.

**What I would change:** Make the editor and builder consume the **same resolved dependency snapshot**. A project lockfile could identify the exact SDK release and content hash, with an explicit update operation. Alternatively, stop copying executable SDK source and generate editor metadata from the same locked SDK resolution the builder uses.

The important requirement is not a particular package-manager design. It is:

> The source the developer inspects must correspond to the source being bundled.

**Acceptance test:** Change the globally installed CLI while retaining a project’s lock. The bundled SDK remains identical, or the build reports an explicit incompatibility—not a silent substitution.

---

## 5. Browser development still treats the built-in games as special cases

**Priority: High for onboarding. Confirmed in source.**

`web/src/game/loadGamePackage.js` contains a hardcoded `LOCAL_GAME_IDS` set:

```text
first-game
second-game
third-game
survival-101
adventure-101
```

Every other valid game ID goes through the backend catalog lookup. Therefore, building a new game into `public/games/my-game/` and opening `?game=my-game` does **not** make the loader use those local files.

`sync_games.sh` likewise enumerates the three standalone games and two examples. The Wild West is included in the compatibility runner’s supported projects, but not in this local browser synchronization path.

**What a developer experiences:** “Your examples run, but my newly created game needs platform changes or publishing before I can use the same browser workflow.”

This is a developer-experience problem more than a missing entry in a list. Adding Wild West to the allowlist fixes one symptom; it does not make the workflow generic.

**What I would change:** Add an explicit arbitrary-project development mode. For example, a **proposed** command such as `cubacadabra dev ./my-game --web` could build the project, expose its package to the local host and provide a launch URL. Local versus published loading should be an explicit source choice, not inferred from whether an ID is one of your demos.

**Acceptance test:** Create a project with a previously unseen name, edit it and run it in the browser without modifying any platform repository or uploading it.

---

## 6. The new `require()` bundler still has a language-understanding gap

**Priority: Medium–high. Dependency-discovery failure reproduced.**

The move away from comment-based imports is the right direction. However, `_require_specifiers()` is a handwritten scanner that treats an entire backtick-delimited string as non-executable text. Luau interpolation can contain executable expressions, including function calls.  ([Luau][1])

For example:

```luau
local title = `Welcome {require("./config").name}`
```

The current scanner finds **no dependency** in that expression.

I ran the extracted scanner against an ordinary require and an interpolated require:

```text
ordinary require:      ('./config',)
interpolation require: ()
```

That means valid Luau can fall outside dependency discovery without receiving a useful build-time error.

**What a developer experiences:** “I used normal Luau syntax and a static require, but your tooling did not understand it.”

**What I would change:** Use a Luau-aware parser or sufficiently complete tokenization for dependency extraction. Where the platform intentionally supports a subset, reject unsupported constructs clearly at build time rather than silently overlooking them.

I would also make original module/line diagnostics part of the bundler’s design, so developers debug their source modules rather than only the generated aggregate.

**Acceptance test:** Include static requires inside interpolation, nested expressions, comments, long strings and shadowed identifiers. Dependency extraction should either resolve them correctly or produce an intentional, documented build error.

---

## 7. Pending actions are not consistently scoped to a round

**Priority: Medium–high. Reproduced in isolation.**

This is separate from acknowledgment handling: even an operation that is retried correctly may no longer be appropriate after the game advances.

In the first game, learning and casting intents do not carry the round they belong to. In Signal Run, capture intents likewise do not bind the action to a round. Reset intents do have a round condition, so this protection exists for one action type but not the others.

I reproduced the following queue scenario:

```text
A capture is pending in round 1.
The store receives a newer snapshot for round 2.
The old capture is proposed again against round 2.
```

The reducer sees “running, next node 1” and accepts the old intent because it does not check which round originally produced it.

**What a developer experiences:** “After a delayed update, an action from the previous round affects the new round.”

**What I would change:** Bind gameplay commands to the relevant round/session generation, and explicitly expire commands when that generation changes. Apply this consistently, not just to reset commands. Return an expired/cancelled outcome so UI code can resolve a pending indicator.

**Acceptance test:** Inject a round change while an interaction is pending. No old-round action may alter the new round.

---

## 8. Asset validation does not establish that the package can actually render

**Priority: Medium–high. Confirmed in source.**

The Python image validator checks IDs, paths, existence, image count and compressed file size. It does **not** decode the image or verify that the complete set fits the runtime’s image atlas. A small invalid file named `.png` can therefore pass that image-validation stage.

Meanwhile, the iOS atlas builder uses a maximum atlas dimension of **2048 × 2048** and a maximum per-image upload dimension of **1020**. It throws when the images do not fit.

A concrete boundary case is **five valid 1020 × 1020 images**, each comfortably below the file-size limit. Five is below the declared count limit of sixteen, yet this set cannot fit the atlas used by that loader.

**What a developer experiences:** “The builder accepted my assets, but adding one more texture makes the game fail on startup.”

**What I would change:** Validate decodability and the target’s complete resource budget during packaging. Ideally, use the same packing logic—or a shared offline packer—for build-time validation and runtime loading. Report the specific constraint and the offending assets, rather than leaving the failure to device startup.

**Acceptance test:** Corrupt images and atlas-overflow cases fail during the developer’s build/check step, with an actionable explanation. Packages advertised as portable should satisfy the portable target’s budgets.

---

## 9. The compatibility gate does not exercise the host code where several mismatches live

**Priority: Medium–high. Confirmed coverage gap.**

The compatibility runner is a useful foundation. It builds six game projects, loads packages through native and WASM validators, compares a conformance trace and invokes Studio validation.

But the reusable workflow does not include the iOS or Android repositories. The runner also does not execute the real browser package-loading workflow discussed above. It therefore cannot establish that Swift, Kotlin and browser host behavior agrees on caching, package loading or asset handling.

**What a developer experiences:** “You say this package is compatible, but it only works through the validator or on one host.”

This is distinct from finding #1: fixing the build will make the current tests run, but will not expand what they prove.

**What I would change:** Add a shared corpus of package-contract fixtures and run it against each host’s real validation/loading boundary. Start with inexpensive tests: missing assets, altered hashes, descriptor mismatches, invalid images, cache selection and failed refreshes. Full device automation can follow.

Longer term, move more pure package validation into shared Rust code, while leaving platform-specific fetching and presentation in each host.

**Acceptance test:** A deliberately invalid package is rejected consistently by every supported host, with an equivalent reason—not merely rejected by the CLI or native validator.

---

## 10. Trusted game authority is documented and prototyped, but not yet integrated

**Priority: High before competitive progression or rewards. Explicit architectural gap, not a newly discovered hidden bug.**

The latest authority map correctly distinguishes server-owned ordering from trusted game outcomes. It says that fields such as scores and captured objectives remain client-authored claims. It also explicitly says the new Rust authority prototype is **not wired to `World`, WebSockets or `second-game`**.

That distinction matters. The current Signal Run flow lets the client decide that an interaction occurred and propose the resulting state; the documentation itself identifies this as the first authority candidate. Because the backend was unavailable, I am reporting the current documented boundary, not claiming an independently demonstrated production exploit.

**What a developer asks:** “Where do I put the trusted rule that decides whether this player earned the reward?”

**What I would change:** Complete one narrow vertical slice: a client submits an interaction command; a trusted handler validates identity and position; game-owned rules produce the accepted transition; clients consume that result.

Crucially, adding a trusted command path is insufficient if clients can still overwrite the supposedly protected result through a generic state-write path.

Until then, make **cooperative synchronization** versus **trusted competitive state** unmistakable in the public SDK and examples.

**Acceptance test:** A client cannot advance the protected objective without a valid accepted interaction, including by using another exposed state-mutation route.

---

## What I would do first

I would organize the next work around three outcomes rather than another batch of unrelated features.

**First, make a clean developer checkout dependable:** repair the compatibility build, lock the supported repository/toolchain combination, and let an arbitrary new game use the browser development workflow.

**Second, make game state dependable:** define distinct-operation semantics, test concurrent updates and lost acknowledgments, and expire stale round-bound actions. The isolated reproductions show why these deserve attention before developers copy the patterns into inventories or progression systems.

**Third, make a release one coherent object:** the same SDK in the editor and bundle, the same package contract on every host, and verified assets belonging to the same immutable release.

My overall judgment is that **the individual components are progressing faster than the guarantees between them**. Your next major improvement should be making those guarantees explicit and executable: what code runs, what a successful build proves, what an accepted action means, and exactly which files constitute a release.

[Download the diagnostic reproductions and full reviewed commit IDs](sandbox:/mnt/data/cubacadabra-review-reproductions.zip)

The bundle contains the three queue cases and the require-scanner case. They use extracted methods and test doubles; they are diagnostic reproductions, not a full platform test suite or a proposed patch.

[1]: https://luau.org/syntax/?utm_source=chatgpt.com "Luau syntax by example | Luau"

