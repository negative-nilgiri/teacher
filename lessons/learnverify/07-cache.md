## Cache

The key in `src/learnverify/cache.rs` hashes the check version, the endpoint, and
the exact request body. The body carries the model name *as requested*, such as
`jev-latest`: the resolved name only arrives in the response, so it is stored in
the entry instead. Entries expire after 24 hours so a moving alias cannot serve
old answers for ever.

The cache holds lesson text and code, and on Linux the temporary directory is
shared, so `open_in` refuses anything but a private directory of the current
user.
