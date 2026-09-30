## Previews and jumps

The browser renders a `#…` link from that frozen table, never by parsing the
destination itself. The [destination handling](#find-links-code:54-57) you saw
earlier is the only place that reads it. A preview renders the target with its
normal renderer, trimmed by `previewNode` below, and sits in a portal so it can
hold whole code blocks and is never clipped. Clicking sets the URL hash to the
block's `block-<id>` anchor, which the block itself watches.
