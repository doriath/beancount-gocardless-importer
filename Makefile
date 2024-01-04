check: format
	cargo clippy
	cargo check

format: format-nix format-rust

format-nix:
	nix fmt

format-rust:
	cargo fmt

fmt: format

update-rust-overlay:
	nix flake lock --update-input rust-overlay
