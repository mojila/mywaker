# mywaker — build & install
#
# Targets:
#   make build         Build the release binary (target/release/mywaker)
#   make debug         Build the debug binary
#   make install       Install into a system $PATH directory (sudo when needed)
#   make install-user  Install into ~/.local/bin (no root)
#   make uninstall     Remove the installed binary
#   make info          Show the detected platform & install paths
#   make clean         Remove build artifacts and staged packages
#   make pkg-deb       Build a .deb  (Debian/Ubuntu)
#   make pkg-rpm       Build a .rpm  (Fedora/RHEL)
#   make pkg-arch      Build an Arch package (makepkg)
#
# Overridable:
#   PREFIX  install root (default /usr/local, or /opt/homebrew on macOS arm64)
#   DESTDIR staging root (for packaging / relocatable installs)
#   TARGET  rustc target triple (for cross-compilation)
#   CARGO   cargo binary (default: cargo)

CARGO   ?= cargo
NAME    := mywaker
VERSION := $(shell sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n1 | tr -d '\n')

# ---- platform detection -----------------------------------------------------
# Darwin | Linux
OS ?= $(shell uname -s)
# arm64 | aarch64 | x86_64
ARCH ?= $(shell uname -m)
# e.g. debian | fedora | arch (lowercased ID from /etc/os-release)
DISTRO ?= $(shell { grep -E '^ID=' /etc/os-release 2>/dev/null || printf 'unknown'; } | head -n1 | cut -d= -f2 | tr -d '"' | tr -d '\n')

# Platform-aware default install prefix.
ifeq ($(OS),Darwin)
  HAS_HOMEBREW := $(shell test -d /opt/homebrew && echo yes)
  ifeq ($(HAS_HOMEBREW),yes)
    PREFIX ?= /opt/homebrew
  else
    PREFIX ?= /usr/local
  endif
else
  PREFIX ?= /usr/local
endif

BINDIR := $(PREFIX)/bin

# Binary path (accounts for cross-compilation via TARGET).
ifeq ($(TARGET),)
  BIN := target/release/$(NAME)
else
  BIN := target/$(TARGET)/release/$(NAME)
endif

# Use sudo only for a direct install into a directory we cannot write.
SUDO := $(shell { test -w "$(BINDIR)" 2>/dev/null || test -n "$(DESTDIR)"; } && echo "" || echo sudo)

.DEFAULT_GOAL := help
.PHONY: help build debug install install-user uninstall info clean pkg-deb pkg-rpm pkg-arch

help: ## Print this help
	@echo "mywaker $(VERSION) — build & install"
	@echo ""
	@echo "Usage:"
	@grep -E '^[a-zA-Z0-9_-]+:.*?## .*$$' $(MAKEFILE_LIST) | \
		awk 'BEGIN { FS = ":.*?## " }; { printf "  \033[36m%-14s\033[0m %s\n", $$1, $$2 }'
	@echo ""
	@echo "Platform: $(OS)/$(ARCH) (distro=$(DISTRO))"
	@echo "Install : $(PREFIX)/bin   (override: make install PREFIX=/path)"
	@echo ""

build: ## Build the release binary
	$(CARGO) build --release $(if $(TARGET),--target $(TARGET),)

debug: ## Build the debug binary
	$(CARGO) build

install: build ## Install to system $PATH (sudo when needed)
	$(SUDO) mkdir -p "$(DESTDIR)$(BINDIR)"
	$(SUDO) install -m 0755 "$(BIN)" "$(DESTDIR)$(BINDIR)/$(NAME)"
	@echo "Installed $(NAME) $(VERSION) -> $(DESTDIR)$(BINDIR)/$(NAME)"

install-user: build ## Install to ~/.local/bin (no root)
	mkdir -p "$(HOME)/.local/bin"
	install -m 0755 "$(BIN)" "$(HOME)/.local/bin/$(NAME)"
	@echo "Installed $(NAME) $(VERSION) -> $(HOME)/.local/bin/$(NAME)"

uninstall: ## Remove the installed binary
	$(SUDO) rm -f "$(DESTDIR)$(BINDIR)/$(NAME)"
	@echo "Removed $(DESTDIR)$(BINDIR)/$(NAME)"

info: ## Show detected platform and install paths
	@echo "name    = $(NAME) $(VERSION)"
	@echo "os      = $(OS)"
	@echo "arch    = $(ARCH)"
	@echo "distro  = $(DISTRO)"
	@echo "prefix  = $(PREFIX)"
	@echo "bindir  = $(BINDIR)"
	@echo "binary  = $(BIN)"
	@echo "target  = $(TARGET)"

clean: ## Remove build artifacts and staged packages
	$(CARGO) clean
	rm -rf build

# ---- distro packages (run on their native distro) --------------------------
DEB_ARCH := $(shell if [ "$(ARCH)" = "x86_64" ]; then echo amd64; elif [ "$(ARCH)" = "aarch64" ]; then echo arm64; else echo "$(ARCH)"; fi)

pkg-deb: build ## Build a .deb for Debian/Ubuntu
	@rm -rf build/deb && mkdir -p build/deb/DEBIAN build/deb/usr/local/bin
	@sed -e 's|@VERSION@|$(VERSION)|g' -e 's|@DEB_ARCH@|$(DEB_ARCH)|g' \
		packaging/debian/control > build/deb/DEBIAN/control
	@cp "$(BIN)" build/deb/usr/local/bin/$(NAME)
	@dpkg-deb --build build/deb "build/$(NAME)_$(VERSION)_$(DEB_ARCH).deb"
	@echo "Built build/$(NAME)_$(VERSION)_$(DEB_ARCH).deb"

pkg-rpm: build ## Build a .rpm for Fedora/RHEL
	@rm -rf build/rpm && mkdir -p build/rpm/{BUILD,RPMS,SOURCES,SPECS,SRPMS}
	@cp "$(BIN)" build/rpm/SOURCES/$(NAME)
	@sed 's|@VERSION@|$(VERSION)|g' packaging/mywaker.spec > build/rpm/SPECS/mywaker.spec
	@rpmbuild --define "_topdir $(CURDIR)/build/rpm" -bb build/rpm/SPECS/mywaker.spec
	@echo "Built .rpm in build/rpm/RPMS/"

pkg-arch: build ## Build an Arch package (makepkg)
	@rm -rf build/arch && mkdir -p build/arch
	@sed -e 's|@VERSION@|$(VERSION)|g' -e 's|@BIN@|$(CURDIR)/$(BIN)|g' \
		packaging/PKGBUILD > build/arch/PKGBUILD
	@cd build/arch && makepkg -f
	@echo "Built Arch package in build/arch/"