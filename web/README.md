# Frontend contract

The Vite build writes production assets to `web/dist`. That directory is the
input expected by the Rust embedding step; running the installed Rust binaries
must not require Node.js.

The client bootstraps with `GET /api/v1/state`:

```json
{
  "lesson": { "title": "…", "nodes": [] },
  "progress": {
    "completed_questions": 0,
    "total_questions": 0,
    "questions": {}
  }
}
```

Nodes are tagged by `type`, carry numeric `node_id` and diagnostic `source_id`
fields, and follow the shapes in `src/types.ts`. A public `multiple_choice` node
contains `prompt`, generated `{ choice_id, content }` choices, and public
`hints`. It never contains correctness markers or the authored explanation.

`POST /api/v1/questions/{node_id}/submit` accepts
`{ "choice_id": number }`; `POST /api/v1/questions/{node_id}/reveal` has no
body. Either mutation may return a full state response, or this focused response:

```json
{
  "progress": { "completed_questions": 1, "total_questions": 1, "questions": {} },
  "question": {
    "attempts": [{ "choice_id": 0, "correct": true }],
    "completed": true,
    "revealed": false,
    "answer": { "choice_id": 0, "explanation": "…" }
  }
}
```

`answer` is absent until a correct submission or explicit reveal. The client
treats the response as authoritative shared state; only radio selection, hint
expansion, pending state, and other presentation details remain local.
