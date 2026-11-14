# OpenFactory VCI component

`tech.openfactory.platform` serves model `tech.openfactory.platform.v1` by
forwarding Check, Set, config Get and state Get to `/usr/bin/openfactory-storage
vci`. The CLI owns the schema and rendering; this component caches no settings.
Every call has a 30-second deadline. A timeout or output error kills and reaps the
CLI. Each output stream is limited to 1 MiB. systemd allows 45 seconds to stop. The shared Debian/Fedora drop-in sets
`Restart=no`: after a bus loss or process failure an operator explicitly starts
the component again.

A failed Set returns a plain D-Bus Failed error so it appears in configd commit
output. configd can still advance running config; confirm application with
`gettree /openfactory rfc7951 | openfactory-storage vci status --check --tree -`.
No OpenFactory service depends on this component or the VCI bus.

Build with the distribution's packaged Rust crates and `vci` 0.2:
`cargo test --offline`. The package build uses `/usr/share/cargo/registry` and
compiles the associated YANG module before installation. The runtime never
formats storage or restarts a service.
