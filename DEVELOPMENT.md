# Development

This file includes information useful for development of this crate.

## Nix

This repository is using nix to manage all required dependencies and support
hermetic builds of the binary. Once you install nix, all other dependecies will
install automatically once you enter the directory (you will need to run
`direnv allow` to actually make it work).
