# SPDX-License-Identifier: GPL-2.0-or-later

DESTDIR ?=
PREFIX  ?= /usr/local

CARGO      ?= cargo
SELINUXOPT ?= $(shell test -x /usr/sbin/selinuxenabled && selinuxenabled && echo -Z)

binpath := $(DESTDIR)/$(PREFIX)/bin/crun-vm

.PHONY: build
build:
	$(CARGO) build --release
	mkdir -p bin
	cp target/release/crun-vm bin/crun-vm

.PHONY: build-debug
build-debug:
	$(CARGO) build
	mkdir -p bin
	cp target/debug/crun-vm bin/crun-vm.debug

.PHONY: clean
clean:
	rm -fr bin target

.PHONY: install
install: build
	install ${SELINUXOPT} -D -m 0755 bin/crun-vm $(binpath)

.PHONY: uninstall
uninstall:
	rm -f $(binpath)

.PHONY: lint
lint:
	tests/lint.sh

.PHONY: test
test:
	tests/env.sh build
	tests/env.sh start
	tests/env.sh run all all
