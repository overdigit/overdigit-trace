include env

all: deb

deb: src/bin/client.rs src/bin/server.rs
	cargo deb --target aarch64-unknown-linux-gnu

clean:
	-rm -r target

.PHONY: all clean deb
