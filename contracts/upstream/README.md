`asp.schema.json` is the unmodified Apache-2.0 schema from
[kobe0938/harbor](https://github.com/kobe0938/harbor/blob/8ee7f0188b4c55aeca47d90b430da24bbbdbda89/asp/asp.schema.json).
Its reference revision differs from the RFC revision: the RFC links to a separate
`asp` branch. Both are recorded in `../compatibility.json`.

This schema describes a connection. It does not provision, isolate, or authenticate
a sandbox on its own. The project adapter and its execution tests belong to WP05.
