## Sending requests

`answer` in `src/learnverify.rs` ties the pieces together. The first half reads
the endpoint and model without the API key and looks every job up in the
cache; the second half only asks for the key if something missed. An unchanged lesson therefore
rechecks offline, and a missing key only affects the blocks that really need a
request.

Misses go to `Client::send_all` in `src/learnverify/client.rs`: four worker
threads share one HTTP agent (one connection pool), each request has the
10-second timeout learnpick already used, and results come back in request
order so grading stays deterministic.

On top of that, all of a run's requests share a 20-second budget, a constant
in `client.rs` like the timeout. The API is fast, so a run that needs longer
means something went wrong: no request starts after the deadline, and one that
is running only gets the time left. Either way the block is reported in
`verify.unavailable` as "time budget of 20 s exceeded".
