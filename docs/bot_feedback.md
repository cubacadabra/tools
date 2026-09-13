The architecture already has most of the proposed foundation, but its
  strongest semantic guarantees currently disappear at the native adapters.

  Specifically:

  - Rust already emits structured username codes such as validation.invalid_characters, save_failed.taken, and
    save_failed.unauthorized in /Users/aa/cubacadabra2/rust/crates/app/src/profile.rs:159.

  - The canonical scenarios already exist in /Users/aa/cubacadabra2/rust/crates/app/tests/username-contract.json.
  - Web and Swift already execute that fixture.
  - But Swift deliberately skips code and username_validation_error assertions in /Users/aa/cubacadabra/ios_app/scripts/
    check_app_contract.swift:150.

  - The Swift model drops the username code entirely in /Users/aa/cubacadabra/ios_app/cubacadabra/
    AppRuntimeBridge.swift:5.

  - Android also drops the code and validation state in /Users/aa/cubacadabra/android_app/app/src/main/java/dev/
    andrewarrow/cubacadabra/game/AppRuntime.kt:6, and it does not run the shared scenarios.

  That means the precise failure highlighted in the feedback is possible today: Rust knows why a username failed, while
  a native binding preserves only an English message.

  The first coherent patch should therefore:

  1. Decode the structured username feedback code and validation error in Swift and Kotlin.
  2. Stop excluding those fields from the Swift fixture assertions.
  3. Run the same canonical username scenarios through Android’s production adapter.
  4. Add one cross-repository CI gate that requires Rust, WASM, Swift, and Kotlin projections to pass that fixture.
  5. Treat the English message as fallback presentation, not the semantic contract.

  Afterward, the review unit becomes concrete:

  one fixture scenario
  → Rust transition
  → emitted effect
  → Rust snapshot
  → Swift projection
  → Kotlin projection
  → web projection

  I would do this before adding more fixtures or moving another feature. It proves the mechanism with one complete
  vertical slice.

  The other recommendations are partly present already:

  - protocol_version: 1 exists in /Users/aa/cubacadabra2/rust/crates/app/src/lib.rs:111.
  - The boundary is already essentially dispatch, snapshot, and poll_effect.
  - HTTP responses already return to Rust for interpretation.

  Version handling should be second: expose the ABI version independently so hosts can reject it before decoding the
  full snapshot—Swift currently decodes first and checks afterward in /Users/aa/cubacadabra/ios_app/cubacadabra/
  AppRuntimeBridge.swift:205.

  So my order would be:

  1. Make the existing username fixture genuinely cross-platform and semantic.
  2. Make ABI-version rejection happen before payload decoding.
  3. Generalize the fixture format and add scenarios for session, safety, appearance, and catalog.
  4. Add structured diagnostic events only where operational debugging demonstrates a need.

  The encouraging result is that you do not need to design this system from scratch. You need to close the last,
  consequential gap in the system already built.
