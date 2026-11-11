# Aether

> [!IMPORTANT]
> Aether has been absorbed directly into [Air](https://github.com/posit-dev/air) as `air_r_parser`. Due to how the Biome infrastructure works, it is much nicer for us to manage the parser and formatter together in the same repo. But fear not! Air is MIT licensed, so feel free to pin against `air_r_parser` in your own Rust projects (but we do not make any promises of following semver).

Aether is an extremely fast R code parser, written in Rust.

It is meant to underlie Rust projects that need to parse R code, such as
[Air](https://github.com/posit-dev/air),
[Ark](https://github.com/posit-dev/ark), and
[Jarl](https://github.com/etiennebacher/jarl).
