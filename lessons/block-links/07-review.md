## Where to look when reviewing

- **Compatibility:** schema 2.3.0 has the 2.2.0 shape; older lessons keep `#…`
  links as ordinary links, so nothing existing breaks.
- **The frozen table** is built by [the resolver](#resolve-code:287-293) and
  keyed by destination as written.
- **Previews** were tested with React Testing Library, not by eye: check the
  popover size and placement in a real browser.
- **The distant-name rule** was narrowed while dogfooding it on the existing
  lessons, from 16 findings (mostly plain words) to 6 real ones.
