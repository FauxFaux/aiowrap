## aiowrap

[![](https://img.shields.io/crates/v/aiowrap.svg)](https://crates.io/crates/aiowrap)
[![](https://travis-ci.org/FauxFaux/aiowrap.svg)](https://travis-ci.org/FauxFaux/aiowrap)

A couple of utilities that I have ended up wanting in various projects,
around `futures::io::AsyncRead` streams.

 * `ShortRead` is an intentionally, controllably naughty `AsyncRead` for testing.
 * `DequeReader` is an `AsyncBufRead` which can be arbitrarily extended.

## Documentation

Please read the [aiowrap documentation on docs.rs](https://docs.rs/aiowrap/).

## License

`MIT or Apache 2.0`.
