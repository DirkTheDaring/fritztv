PROJECT := fritztv
REPO_USER ?= DirkTheDaring
REPO_NAME ?= fritztv
VERSION := $(shell grep "^version =" Cargo.toml | head -n1 | cut -d '"' -f 2)
RPMBUILD_DIR := $(CURDIR)/target/rpmbuild
DEBIAN_VERSION ?= latest
FEDORA_VERSION ?= latest
# Detect container runtime: prefer podman, then docker. Allow override (e.g. CONTAINER=nerdctl).
CONTAINER ?= $(shell command -v podman > /dev/null 2>&1 && echo podman || echo docker)
CONTAINER_BUILD_FLAGS ?=
CONTAINER_RUN_FLAGS ?=
# Check if SELinux is enabled. If so, append :z to volume mounts.
# We check if 'selinuxenabled' exists and exits with 0.
SELINUX_ENABLED := $(shell command -v selinuxenabled >/dev/null 2>&1 && selinuxenabled && echo 1 || echo 0)
VERSION := $(shell grep "^version =" Cargo.toml | head -n1 | cut -d '"' -f 2)

ifeq ($(SELINUX_ENABLED),1)
	VOLUME_LABEL ?= :z
else
	VOLUME_LABEL ?=
endif

# Cargo caching: use named volumes to persist index/git deps between container runs.
# This avoids re-downloading crates every time.
# We mount them into the container's /root/.cargo (standard location for root in Docker).
CARGO_CACHE_ARGS ?= -v cargo-registry:/root/.cargo/registry -v cargo-git:/root/.cargo/git

# Images for cross-compilation
CROSS_IMAGE_WIN := ghcr.io/cross-rs/x86_64-pc-windows-gnu:main
CROSS_IMAGE_MAC := ghcr.io/cross-rs/x86_64-apple-darwin:main

.PHONY: all fedora fedora-container debian-container windows-container macos-container linux packages release clean

all: fedora

fedora:
	mkdir -p $(RPMBUILD_DIR)/{SOURCES,SPECS,BUILD,RPMS,SRPMS}
	# Create source tarball using git archive (clean and respects gitignore)
	git archive --format=tar.gz --prefix=$(PROJECT)-$(VERSION)/ -o $(RPMBUILD_DIR)/SOURCES/$(PROJECT)-$(VERSION).tar.gz HEAD
	# Copy auxiliary files
	cp config.toml $(RPMBUILD_DIR)/SOURCES/
	cp packaging/rpm/fritztv.service packaging/rpm/fritztv.sysconfig packaging/rpm/fritztv.sysusers $(RPMBUILD_DIR)/SOURCES/
	# Copy spec file and build
	cp packaging/rpm/fritztv.spec $(RPMBUILD_DIR)/SPECS/
	sed -i "s/^Version:.*/Version:        $(VERSION)/" $(RPMBUILD_DIR)/SPECS/fritztv.spec
	rpmbuild --define "_topdir $(RPMBUILD_DIR)" -ba $(RPMBUILD_DIR)/SPECS/fritztv.spec
	mkdir -p target/rpm-host-x86_64
	find $(RPMBUILD_DIR)/RPMS -name "*.rpm" -exec cp {} target/rpm-host-x86_64/ \;
	find $(RPMBUILD_DIR)/SRPMS -name "*.rpm" -exec cp {} target/rpm-host-x86_64/ \;

windows-container:
	mkdir -p target/exe-windows-x86_64
	$(CONTAINER) build $(CONTAINER_BUILD_FLAGS) -f packaging/windows/Dockerfile -t $(PROJECT)-win .
	$(CONTAINER) run $(CONTAINER_RUN_FLAGS) $(CARGO_CACHE_ARGS) --rm -v $(CURDIR):/project$(VOLUME_LABEL) \
		-w /project \
		$(PROJECT)-win \
		cargo build --release --target x86_64-pc-windows-gnu
	cp target/x86_64-pc-windows-gnu/release/$(PROJECT).exe target/exe-windows-x86_64/$(PROJECT)-windows-x86_64.exe

macos-container:
	mkdir -p target/bin-macos-x86_64
	$(CONTAINER) build $(CONTAINER_BUILD_FLAGS) -f packaging/macos/Dockerfile -t $(PROJECT)-mac .
	$(CONTAINER) run $(CONTAINER_RUN_FLAGS) $(CARGO_CACHE_ARGS) --rm -v $(CURDIR):/project$(VOLUME_LABEL) \
		-w /project \
		-e CROSS_SDK_VERSION=10.15 \
		$(PROJECT)-mac \
		cargo build --release --target x86_64-apple-darwin
	cp target/x86_64-apple-darwin/release/$(PROJECT) target/bin-macos-x86_64/$(PROJECT)-macos-x86_64

linux:
	mkdir -p target/bin-linux-x86_64
	cargo build --release
	cp target/release/$(PROJECT) target/bin-linux-x86_64/$(PROJECT)-linux-x86_64

packages: fedora-container debian-container windows-container macos-container linux
	@echo "All packages built in target/"

clean:
	cargo clean

.PHONY: debian-container

debian-container:
	mkdir -p target/deb-debian-$(DEBIAN_VERSION)-x86_64 target/deb-debian-$(DEBIAN_VERSION)-x86_64-build
	$(CONTAINER) build $(CONTAINER_BUILD_FLAGS) -f packaging/deb/Dockerfile --build-arg DEBIAN_VERSION=$(DEBIAN_VERSION) -t $(PROJECT)-deb:$(DEBIAN_VERSION) .
	$(CONTAINER) run $(CONTAINER_RUN_FLAGS) $(CARGO_CACHE_ARGS) --rm \
		-v $(CURDIR):/work$(VOLUME_LABEL) \
		-v $(CURDIR)/target/deb-debian-$(DEBIAN_VERSION)-x86_64:/out$(VOLUME_LABEL) \
		-v $(CURDIR)/target/deb-debian-$(DEBIAN_VERSION)-x86_64-build:/build$(VOLUME_LABEL) \
		-e BUILD_DIR=/build \
		-w /work \
		$(PROJECT)-deb:$(DEBIAN_VERSION) \
		bash /work/packaging/deb/build-deb.sh /out

.PHONY: fedora-container
fedora-container:
	mkdir -p target/rpm-fedora-$(FEDORA_VERSION)-x86_64
	$(CONTAINER) build $(CONTAINER_BUILD_FLAGS) -f packaging/rpm/Dockerfile --build-arg FEDORA_VERSION=$(FEDORA_VERSION) -t $(PROJECT)-rpm:$(FEDORA_VERSION) .
	$(CONTAINER) run $(CONTAINER_RUN_FLAGS) $(CARGO_CACHE_ARGS) --rm \
		-v $(CURDIR):/work$(VOLUME_LABEL) \
		-v $(CURDIR)/target/rpm-fedora-$(FEDORA_VERSION)-x86_64:/out$(VOLUME_LABEL) \
		-w /work \
		$(PROJECT)-rpm:$(FEDORA_VERSION) \
		bash /work/packaging/rpm/build-rpm.sh /out

release:
	@if ! command -v gh >/dev/null 2>&1; then \
		echo "Error: github-cli (gh) is not installed."; \
		exit 1; \
	fi
	@if gh release view --repo $(REPO_USER)/$(REPO_NAME) v$(VERSION) >/dev/null 2>&1; then \
		echo "Release v$(VERSION) already exists. Skipping creation."; \
		exit 0; \
	fi
	$(MAKE) packages
	gh release create --repo $(REPO_USER)/$(REPO_NAME) v$(VERSION) \
		--title "v$(VERSION)" \
		--generate-notes \
		target/bin-linux-x86_64/$(PROJECT)-linux-x86_64 \
		target/exe-windows-x86_64/$(PROJECT)-windows-x86_64.exe \
		target/bin-macos-x86_64/$(PROJECT)-macos-x86_64 \
		$$(find target/deb-debian-*-x86_64 -name "*.deb") \
		$$(find target/rpm-fedora-*-x86_64 -name "*.rpm") \
