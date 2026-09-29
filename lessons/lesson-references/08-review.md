## Where to look when reviewing

- **The no-Git invariant:** a contract test puts a Git wrapper on the search path that
  records any call and builds a plain-file lesson; it must stay unused.
- **Blob IDs** are checked against `git hash-object` for an untracked and a
  dirty file. Repositories that use SHA-256 object names get IDs that do not
  match.
- **Artifact 1.5.0** only adds optional fields; older artifacts give shorter
  references.
- **The UI** was tested with React Testing Library, not by eye: check that the
  icon is discreet enough and the popover sits well in real blocks.
