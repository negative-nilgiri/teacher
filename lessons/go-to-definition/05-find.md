`find_definitions` in `definitions.rs` combines the two for every line of a
block. The compiler
runs it over every code block and over the new-side lines of every diff, and
freezes name → sites in the artifact (1.7.0).
