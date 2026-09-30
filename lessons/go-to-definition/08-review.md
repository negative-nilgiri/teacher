## Where to look when reviewing

- **The keyword table** in `definitions.rs`: running it over the existing
  lessons found 24 definitions, all correct. That run also led to two
  refinements: multi-line signatures keep their whole body, and JS/TS and Go
  bindings count only at the top level of a file, so locals never link.
- **Recognized shell forms** are `name() {` and `function name {`; a plain
  `name {` is a command call in bash and zsh.
- **Keyboard:** names in code are not focusable, to avoid dozens of tab stops
  per block; go to definition is pointer-driven.
- **The UI** was tested with React Testing Library, not by eye.
