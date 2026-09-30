## What lint adds

Beyond the [range check](#resolve-code:270-283) that makes a lesson invalid,
lint gives advice: previews longer than 15 lines, links to the next or
previous block, links to a later block, excerpts of the same file repeated far
apart, and names formatted as code that no block within three blocks shows.
The last rule only considers names that look like code, using `code_shaped`
below, because plain words match comments and strings in far-away blocks.
