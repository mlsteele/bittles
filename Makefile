.phony: doc-local

all:
	cargo build

run:
	cargo run ubuntu-14.04.5-desktop-amd64.iso.torrent

doc-local:
	cargo rustdoc --open -- --no-defaults --passes collapse-docs --passes unindent-comments --passes strip-priv-imports
