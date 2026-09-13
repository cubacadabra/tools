# Cubacadabra preview licensing

This is the current repository policy for the developer preview. It is a
project policy summary, not legal advice.

| Material | Current license or obligation |
| --- | --- |
| Tools, Rust engine, Studio, web, iOS, and Android source | GPL-3.0-or-later unless a file or dependency carries its own notice. |
| Example game source and artwork | GPL-3.0-or-later; see the example repository `LICENSE`. |
| SDK helpers copied into a generated game package | GPL-3.0-or-later because the builder incorporates their source into `game.luau`. |
| A creator's original game code and artwork | The creator should choose and publish a license for that material; the SDK license still applies to copied SDK source. |

The examples README and `examples/LICENSE` are aligned on GPL-3.0-or-later.
Before encouraging redistribution of generated packages, add the applicable
license and attribution notices to the package distribution workflow and keep
those notices alongside the release artifact.
