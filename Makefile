# SPDX-License-Identifier: GPL-2.0-or-later

DESTDIR ?=
PREFIX  ?= /usr/local

CARGO      ?= cargo
SELINUXOPT ?= $(shell test -x /usr/sbin/selinuxenabled && selinuxenabled && echo -Z)

binpath := $(DESTDIR)/$(PREFIX)/bin/crun-vm
manpath := $(DESTDIR)/$(PREFIX)/share/man/man1/crun-vm.1.gz

all: out/crun-vm out/crun-vm.1.gz

.PHONY: out/crun-vm
out/crun-vm:
	mkdir -p $(@D)
	$(CARGO) build --release
	cp target/release/crun-vm $@

out/crun-vm.1.gz: docs/5-crun-vm.1.ronn
	mkdir -p $(@D)
	ronn --pipe --roff $< | gzip > $@

.PHONY: clean
clean:
	rm -fr out target

.PHONY: install
install: out/crun-vm install-man
	install ${SELINUXOPT} -D -m 0755 $< $(binpath)

.PHONY: install-man
install-man: out/crun-vm.1.gz
	install ${SELINUXOPT} -D -m 0644 $< $(manpath)

.PHONY: uninstall
uninstall:
	rm -f $(binpath) $(manpath)

.PHONY: lint
lint:
	tests/lint.sh

.PHONY: test
test:
	tests/env.sh build
	tests/env.sh start
	tests/env.sh run all all
