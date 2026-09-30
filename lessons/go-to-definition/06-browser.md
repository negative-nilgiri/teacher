## Marking names in the browser

The browser already renders code with highlight.js, whose output puts
comments and strings in their own tokens. The marking step in
`web/src/definitions.ts` walks that output, skips those tokens, and wraps a name
only when a definition allows the usage: a function only when called, a macro
only as `name!`, other kinds anywhere.
