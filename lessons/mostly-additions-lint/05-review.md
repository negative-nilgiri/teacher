## Where to look when reviewing

- **The ratio test** is `deleted > added × ratio`, so exactly one deletion per
  ten additions is still reported at the default 0.10.
- **Exclusions:** new files (their own `error`) and Markdown from Git sources
  (rendered prose). A Markdown file from a patch is still reported.
- **Tests** in `src/lint/rules.rs`: ratio and minimum boundaries, several hunks
  in one file with only the qualifying one reported, a new file, Markdown from
  a patch, and a Git source that points at the file selection and skips the
  Markdown file.
