# This Makefile is intended for developer convenience.  For the most part
# all the targets here simply wrap calls to the `cargo` tool.  Therefore,
# most targets must be marked 'PHONY' to prevent `make` getting in the way
#
#prog :=xnixperms

DESTDIR ?=
PREFIX ?= /usr/local
BINDIR ?= $(PREFIX)/bin

SELINUXOPT ?= $(shell test -x /usr/sbin/selinuxenabled && selinuxenabled && echo -Z)
# Get crate version by parsing the line that starts with version.
CRATE_VERSION ?= $(shell grep ^version Cargo.toml | awk '{print $$3}')
GIT_TAG ?= $(shell git describe --tags)

# Set this to any non-empty string to enable unoptimized
# build w/ debugging features.
debug ?=

# Set path to cargo executable
CARGO ?= cargo

# All complication artifacts, including dependencies and intermediates
# will be stored here, for all architectures.  Use a non-default name
# since the (default) 'target' is used/referenced ambiguously in many
# places in the tool-chain (including 'make' itself).
CARGO_TARGET_DIR ?= targets
export CARGO_TARGET_DIR  # 'cargo' is sensitive to this env. var. value.

ifdef debug
$(info debug is $(debug))
  # These affect both $(CARGO_TARGET_DIR) layout and contents
  # Ref: https://doc.rust-lang.org/cargo/guide/build-cache.html
  release :=
  profile :=debug
else
  release :=--release
  profile :=release
endif

.PHONY: all
all: build

bin:
	mkdir -p $@

$(CARGO_TARGET_DIR):
	mkdir -p $@

.PHONY: build
build: bin $(CARGO_TARGET_DIR)
	$(CARGO) build $(release)
	cp $(CARGO_TARGET_DIR)/$(profile)/crun-vm bin/crun-vm$(if $(debug),.debug,)

.PHONY: clean
clean:
	rm -rf bin
	if [ "$(CARGO_TARGET_DIR)" = "targets" ]; then rm -rf targets; fi

.PHONY: install
install:
	install ${SELINUXOPT} -D -m0755 bin/crun-vm $(DESTDIR)/$(BINDIR)/crun-vm

.PHONY: uninstall
uninstall:
	rm -f $(DESTDIR)/$(BINDIR)/crun-vm

#.PHONY: unit
unit: $(CARGO_TARGET_DIR)
	$(SHELL) tests/env.sh build
	$(SHELL) tests/env.sh start
	$(SHELL) tests/env.sh run all all

#.PHONY: code_coverage
code_coverage: $(CARGO_TARGET_DIR)
	# Downloads tarpaulin only if same version is not present on local
	$(CARGO) install cargo-tarpaulin
	$(CARGO) tarpaulin -v

.PHONY: validate
validate: $(CARGO_TARGET_DIR)
	$(SHELL) lint.sh
