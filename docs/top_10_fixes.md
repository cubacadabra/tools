**I found several concrete problems—not just “you need more documentation” or “the platform needs more features.”** The first three deserve immediate attention.

I surveyed the **11 public repositories**, with the deepest inspection on the builder, Studio’s project loader, Rust scripting, browser package loading, and iOS package loading. I also checked the example projects, Android integration instructions, and deployed-site structure. This was a source-level review, not an end-to-end execution of every client. Some GitHub responses were cached, so cross-repository findings should be checked against a pinned set of commits; the destructive-build issue below is visible in **the exact commit you linked**. ([GitHub][1])

My overall impression: **the core separation is sensible, but several contracts have multiple implementations that can disagree.** That is the recurring weakness I would address—not the choice of Rust, Luau, or native application shells.

## 1. The builder can delete the project it is building

**Priority: immediate. Code defect.**
**Location:** `tools/src/cubacadabra/game_builder.py`, `_validate_output()` and `build_game()`.

The output guard rejects an output directory **inside** `src/`, but does not reject an output directory that **contains** `src/`. The builder subsequently recursively deletes an existing output directory. Those operations are present in commit `662c18a`. ([GitHub][2])

For example, with source at `/work/my-game/src`, choosing `/work/my-game` as output passes that guard—even though deleting the output deletes the source project.

There is a related transactional problem: the previous output is deleted **before module bundling succeeds**. An invalid `require()` can therefore destroy the last successful build. ([GitHub][2])

**What I would change:** Reject outputs overlapping any input directory in either direction, refuse to erase arbitrary directories without a builder-owned marker, and build into a temporary directory before replacing a successful output.

**Acceptance test:** Dangerous output paths leave every input untouched; a failed build preserves the previous working package.

This is the most urgent finding because a developer should never lose work by misconfiguring a build command.

## 2. Studio and the CLI still have separate definitions of a valid game project

**Priority: immediate. Cross-repository inconsistency.**
**Location:** `studio/src/main.rs`, `load_game_sources()`, `expand_raw_script()`, and `sdk_include()`.

Your fix moves the Python builder to scoped, cached `require()` modules. But the Studio source served during this review still assembles raw projects using its own recursive `@include` implementation, including its own SDK-name mapping and restrictions on included-file returns. Its README describes that same older path. ([GitHub][3])

Studio explicitly takes this separate path when it finds `src/main.luau` rather than an already-built `game.luau`. That makes the divergence developer-facing, not dead code. 

The likely experience is:

> “My game builds with the official CLI, but opening its source project in the official Studio fails.”

Even after updating Studio for this particular change, **maintaining two bundlers leaves the underlying problem intact**.

**What I would change:** Give both tools one canonical project-to-runtime-input pipeline. Studio can invoke the existing builder into a temporary location initially; it does not need an independent Rust reimplementation.

**Acceptance test:** The same fixture, containing nested modules, SDK imports, and module return values, produces equivalent runtime inputs through CLI build and Studio raw-project loading.

## 3. Script execution limits do not cover all entry points

**Priority: immediate, before accepting arbitrary creator code. Code-level safety gap.**
**Location:** `rust/src/scripting.rs`, `load_inner()`, `tick()`, and `run_tasks()`.

There **is** an execution-budget mechanism, so “you have no limits” would be an inaccurate criticism. However, the budget becomes active inside `run_tasks()`. Module evaluation, `on_start`, and the direct `on_tick` call occur outside that active budget. Network and UI callbacks are also dispatched before scheduled tasks run. 

Consequently, the scheduled-task infinite-loop test does not establish that an infinite loop in ordinary lifecycle code is contained.

The developer-facing failure could be:

> “A mistake in my startup function hangs the host instead of producing a useful error.”

**What I would change:** Put every transition into game-owned code behind a common budgeted execution boundary. Include module initialization, lifecycle callbacks, save/restore hooks, and scheduled work. Audit memory limits alongside execution limits.

**Acceptance test:** Run deliberately nonterminating code in each entry point in isolated test processes. Every case must hit a controlled limit without leaving the application unusable.

I have not executed those hang tests; this finding follows from the visible budget activation and callback ordering.

## 4. Shared state is server-ordered, but game rules are still client-authored

**Priority: before trusted scores, progression, trading, or rewards. Architectural boundary.**
**Location:** `tools/docs/shared-state-v1.md` and the shared-state SDK.

The documentation correctly acknowledges this: clients propose state, and the cooperative shared-state contract is not a cheat-resistant game authority. Compare-and-set handles ordering and conflicting updates; it does not establish that the proposed outcome is legitimate. 

A developer can easily hear “authoritative state” and assume more protection than that. The important distinction is:

> **“The server accepted this version” is not the same as “the server verified this game action.”**

For a cooperative experiment, this is a reasonable tradeoff. For valuable inventory or competitive outcomes, it is a different architectural requirement.

**What I would change:** Make the trust boundary explicit in the API documentation and examples. Before supporting valuable outcomes, provide a trusted validation path where clients submit actions and server-side code validates the resulting transitions.

That does **not** require putting individual games’ rules into the Rust engine. A game-owned reducer could remain game-owned while executing in a trusted environment.

**Acceptance test:** A modified client submits a correctly sequenced but impossible reward claim. The trusted path rejects it.

## 5. Two Luau implementations need behavioral conformance tests

**Priority: high. Portability risk—not a demonstrated incompatibility.**
**Location:** `rust/Cargo.toml`, scripting integration, `third-game`, and `tools/tests/test_preview_conformance.py`.

Native builds use `mlua` with vendored Luau; browser builds use `luaur-rt`. These are two runtime implementations, not merely two platform wrappers around the same VM. 

The preview conformance test I inspected builds the probe and checks that API markers appear in source/generated text and documentation. That is useful documentation-coverage checking, but it does not prove that those APIs behave identically across native and browser execution. 

The developer concern would be:

> “Does portable mean my program runs identically, or that both platforms recognize roughly the same API?”

**What I would change:** Make `third-game` an executable acceptance suite. Feed identical inputs into both runtimes and compare structured outcomes: module caching, callback order, task scheduling, error behavior, JSON conversion, and SDK state transitions.

Use a fixed input trace and seeded randomness where applicable. Test failure behavior as well as successful calls.

**Acceptance test:** Every supported SDK release passes the same native/browser behavioral fixtures.

I did not establish an automated cross-runtime equivalence gate from the inspected paths; I am not claiming the project has no other tests.

## 6. Package integrity depends too much on separate downloads and host behavior

**Priority: high, before frequent remote updates. Release-design risk.**
**Location:** `ios_app/cubacadabra/GamePackage.swift`; related mobile refresh behavior.

The iOS loader fetches `manifest.json` and `game.luau` separately. Its cache writes also store manifest and script separately under game-specific keys. Version comparison helps choose between bundled and cached content, but those mechanisms alone do not bind every file to one release. Android’s documentation describes a similar remote-refresh model. ([GitHub][4])

The risk is a mixed package: new metadata with old code, or code and assets from different releases. Whether that happens depends partly on publishing paths and deployment atomicity, which I could not fully verify from the public backend-facing material.

**What I would change:** Treat a release as one immutable, verifiable object: a release identifier, checksummed files, an immutable release directory, and one pointer selecting the active release.

Download and validate into staging, then switch the cache atomically. Preserve a complete last-known-good release.

**Acceptance test:** Interrupt deployment and downloading between every file. A client must load either the complete old release or the complete new release—never a mixture.

**SemVer selects a release; it does not prove that its files belong together.**

## 7. Launching a known game scans the catalog to discover its package

**Priority: medium-high. Concrete scalability and reliability weakness.**
**Location:** `web/src/game/loadGamePackage.js`, `loadUploadedCubeBaseUrl()`.

For an uploaded game, this function searches paginated catalog responses for the requested ID. The configured bounds allow up to **200 sequential page requests**, with **50 entries per page**. 

That makes a direct game link depend on catalog size and ordering. The browser already knows which game it wants, yet it performs a browsing operation to locate it.

**What I would change:** Resolve a game directly by its identifier. Have that lookup return the launchable release, package location, and relevant access/moderation status.

Keep catalog pagination for browsing—not launching.

**Acceptance test:** Launching a game requires a fixed number of metadata requests whether the catalog contains ten games or ten thousand.

This is a relatively small correction now, and a much more annoying migration after multiple clients copy the same lookup pattern.

## 8. The creator, builder, and browser disagree about valid game IDs

**Priority: high. Concrete example of schema drift.**
**Location:** `tools/.../game_creator.py`, `game_builder.py`, and `web/.../loadGamePackage.js`.

Here is a small but revealing inconsistency:

The creator can generate a one-character ID from a title such as `A`. The builder requires a nonempty string but does not enforce the browser’s ID-length rule. The browser requires 3–64 characters and falls back to `first-game` when the requested ID is invalid. These code paths imply that a project can pass creation/build validation but fail browser selection and select the default game instead. 

The developer’s reaction would be:

> “Your tools created this project. Why won’t your client open it?”

**What I would change:** Establish one canonical package contract covering identifiers, versions, paths, assets, and world references. Generate host validators from it, or expose shared validation through Rust.

Also distinguish “no game specified” from “an explicitly requested game is invalid.” The latter should produce an error, not silently change the destination.

**Acceptance test:** A corpus of valid and invalid manifests produces the same accept/reject results in the creator, builder, Studio, browser, and mobile loaders.

## 9. The supported workflow still assumes the maintainer’s checkout

**Priority: high for developer adoption. Distribution and onboarding weakness.**
**Location:** Studio/mobile setup documentation, `game_creator.py`, and SDK workspace configuration.

Studio expects sibling `rust` and `tools` repositories. Mobile setup instructions similarly assume a multi-repository workspace and a sibling backend. That backend is not among the 11 public repositories listed in the organization. 

There is also a smaller version of the same problem around the SDK module fix: the editor alias is workspace configuration, while release builds must resolve the canonical SDK from the toolchain. `create_game()` therefore does not copy executable SDK source into every project. Standalone offline projects can opt into a project-local copy explicitly. ([GitHub][3])

**What I would change:** Separate two journeys.

A **game creator** should install a supported tool/Studio release, create a project anywhere, and get the correct SDK, editor configuration, and runnable preview.

A **platform contributor** can use a multi-repository workspace, but needs a pinned, known-good revision set and an explicit explanation of backend availability.

A monorepo is not required. A reproducible workspace is.

**Acceptance test:** On a clean machine, create a game outside the Cubacadabra checkout, import an SDK helper, obtain editor navigation, build, and preview without editing internal paths.

## 10. Licensing is contradictory where developers are supposed to copy and reuse code

**Priority: resolve before encouraging substantial third-party adoption. Developer-trust issue.**
**Location:** `examples/README.md`, `examples/LICENSE`, and the distributable SDK helpers.

The examples README says GPL-3.0-or-later, while the actual `examples/LICENSE` contains Apache License 2.0. That is a direct contradiction in the material a developer would consult before reusing an example. ([GitHub][5])

Separately, `tools` has a GPL license, and the bundler incorporates SDK helper source into game output. That makes the licensing policy for **redistributed helper code** something you should explain explicitly, rather than leaving creators to infer it from the tool repository’s root license. 

The question you need to answer unambiguously is:

> “What obligations apply to my own game, copied example code, bundled SDK helpers, and artwork?”

**What I would change:** Publish a short licensing matrix for those categories, align README statements with license files, and ensure generated packages carry the notices required by the chosen policy.

**The weakness is not choosing GPL. It is leaving the reuse boundary unclear.** I am flagging ambiguity, not concluding that every game must be GPL.

---

## What I would tackle first

**Fix #1 and #3 before adding more creator-facing functionality:** protect files and contain erroneous scripts. Then fix #2 by removing the duplicate build pipeline rather than merely teaching both implementations the latest syntax.

For the first outside-developer experience, I would next address **#8 and #9**: an officially generated project should be valid everywhere and should not require your personal repository arrangement.

The broader architectural lesson is consistent across these findings:

**Define each important contract once, and make every consumer prove that it follows it.**

That means one project build pipeline, one package-validation contract, one coherent release identity, and executable tests for the native/browser promise. Those changes would prevent more future “why did you invent this?” feedback than adding another layer of features.

[1]: https://github.com/orgs/cubacadabra/repositories "cubacadabra repositories · GitHub"
[2]: https://github.com/cubacadabra/tools/blob/662c18a43deafb5b8f814d61697e0d62eb167e22/src/cubacadabra/game_builder.py "tools/src/cubacadabra/game_builder.py at 662c18a43deafb5b8f814d61697e0d62eb167e22 · cubacadabra/tools · GitHub"
[3]: https://github.com/cubacadabra/tools/commit/662c18a43deafb5b8f814d61697e0d62eb167e22 "fixing include term to proper luau · cubacadabra/tools@662c18a · GitHub"
[4]: https://github.com/cubacadabra/ios_app/blob/main/cubacadabra/GamePackage.swift "ios_app/cubacadabra/GamePackage.swift at main · cubacadabra/ios_app · GitHub"
[5]: https://github.com/cubacadabra/examples "GitHub - cubacadabra/examples: Small Cubacadabra game projects for learning and testing the platform, including Adventure 101, Survival 101, and The Wild West. Demonstrates the portable manifests, Luau source, assets, and project structure consumed by Studio and the build tools. · GitHub"
