Misses go to `Client::send_all` in `src/learnverify/client.rs`: four worker
threads share one HTTP agent (one connection pool), and results come back in
request order so grading stays deterministic.

The TypeSafe API answers most requests in about 0.3 s, but about a third stall
for 13 s or more, whatever the client or request. So `send_until` gives each
attempt 2 s and sends a timed-out request again. Other failures, such as an
HTTP error status, are fast and deterministic, so they are not retried.

All of a run's attempts share a 20-second budget. Both durations are constants
in `client.rs`. No attempt starts after the deadline, and one that is running
only gets the time left; a block still unanswered is reported in
`verify.unavailable` as "time budget of 20 s exceeded".
