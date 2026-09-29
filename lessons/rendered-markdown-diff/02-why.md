## Why whole documents

A hunk is a fragment of a file. It can start inside a list, end inside a
fenced code block, or leave a `$$` open, and rendering such a fragment breaks
everything after the cut. So the compiler reads the **complete** before and
after documents for every Markdown file in a Git diff, and the browser only
renders pieces that are complete blocks. React still never compares text.
