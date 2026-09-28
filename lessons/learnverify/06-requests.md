## Sending requests

`answer` in `src/learnverify.rs` ties the pieces together. The first half reads
the endpoint and model without the API key and looks every job up in the
cache; the second half only asks for the key if something missed. An unchanged lesson therefore
rechecks offline, and a missing key only affects the blocks that really need a
request.
