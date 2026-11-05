# OVRCR patch

This is the upstream source package for `vt100` 0.16.2 from
<https://github.com/doy/vt100-rust>, with one OVRCR patch: `CSI 3 J` clears
the active grid's saved lines in place. The original crates.io checksum from
`Cargo.lock` was
`054ff75fb8fa83e609e685106df4faeffdf3a735d3c74ebce97ec557d5d36fd9`.

The upstream package manifest, README, changelog and MIT license are retained.
This local patch is for OVRCR and must not be published as the upstream crate.
