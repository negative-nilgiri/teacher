# References for questions about a lesson

Asking an agent about a lesson used to mean retyping "the code in the third
section, around line 40". Every block now offers a reference to copy: an icon
appears in its header on hover or focus, and selecting code or diff lines shows
a small "Ask" chip. The popover takes an optional question and copies text like
this:

```text
Question about lesson "Queues" (lessons/queue/lesson.json), block `pop` (code):
- shown: src/queue.rs, lines 40–42; selected line 41 (highlight group 0)
- version: worktree file at build time, blob 3f2a91c…, on top of commit 5d11bde…
- exact shown text: block `pop` in lessons/queue/lesson.learn
My question: Why the front?
```

Nothing is sent anywhere: learn still holds no credentials and talks to no
agent. The reference names content but never includes it, and never reveals a
quiz answer.
