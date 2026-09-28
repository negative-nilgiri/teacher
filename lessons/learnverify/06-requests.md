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
