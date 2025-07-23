# Makefile 

.PHONY: all default build install install-system uninstall uninstall-system clean run test check fmt clippy update repo status dev package install-complete

# --- Variables ---
BIN_NAME := fs
RELEASE_BIN_PATH := target/release/$(BIN_NAME)
LOCAL_INSTALL_PATH := $(HOME)/.local/bin/$(BIN_NAME)
SYSTEM_INSTALL_PATH := /usr/local/bin/$(BIN_NAME)

# --- Default Target ---
all: default

default:
	@echo "Available targets:"
	@awk -F':' '/^[a-zA-Z0-9_-]+:/ {print "  "$$1}' $(MAKEFILE_LIST) | grep -v .PHONY | grep -v "^  #"

# --- Build ---
build:
	@echo "Building release binary..."
	@cargo build --release --bin file_system --target-name $(BIN_NAME)

# --- Install ---
install: build
	@echo "Installing user binary to $(LOCAL_INSTALL_PATH)"
	@mkdir -p "$(dir $(LOCAL_INSTALL_PATH))"
	@if [ -f "$(RELEASE_BIN_PATH)" ]; then \
		cp -f "$(RELEASE_BIN_PATH)" "$(LOCAL_INSTALL_PATH)"; \
		chmod 755 "$(LOCAL_INSTALL_PATH)"; \
		echo "User binary installed at $(LOCAL_INSTALL_PATH)"; \
	else \
		echo "Error: Release binary not found (run 'make build' first)"; \
		exit 2; \
	fi

install-system: build
	@echo "Installing system binary to $(SYSTEM_INSTALL_PATH) (requires sudo)"
	@if [ -f "$(RELEASE_BIN_PATH)" ]; then \
		sudo mkdir -p "$(dir $(SYSTEM_INSTALL_PATH))"; \
		sudo cp -f "$(RELEASE_BIN_PATH)" "$(SYSTEM_INSTALL_PATH)"; \
		sudo chmod 755 "$(SYSTEM_INSTALL_PATH)"; \
		echo "System binary installed at $(SYSTEM_INSTALL_PATH)"; \
	else \
		echo "Error: Release binary not found (run 'make build' first)"; \
		exit 2; \
	fi

# --- Uninstall ---
uninstall:
	@echo "Attempting to remove user binary from $(LOCAL_INSTALL_PATH)"
	@read -p "Confirm uninstall user binary? [y/N] " ans; \
	if [ "$$ans" = "y" ] || [ "$$ans" = "Y" ]; then \
		if [ -f "$(LOCAL_INSTALL_PATH)" ]; then \
			rm -f "$(LOCAL_INSTALL_PATH)"; \
			echo "User binary uninstalled from $(LOCAL_INSTALL_PATH)"; \
		else \
			echo "User binary not found at $(LOCAL_INSTALL_PATH)"; \
		fi; \
		echo ""; \
		echo "IMPORTANT: If you created a shell alias or function for '$(BIN_NAME)', you must remove it manually."; \
		echo "  Example (for bash/zsh): Open ~/.bashrc or ~/.zshrc and remove lines like:"; \
		echo "    alias $(BIN_NAME)='$(LOCAL_INSTALL_PATH)'"; \
		echo "    $(BIN_NAME)() { ... }"; \
		echo "  Then, run 'source ~/.bashrc' or 'source ~/.zshrc' to apply changes."; \
	else \
		echo "Uninstall cancelled."; \
	fi

uninstall-system:
	@echo "Attempting to remove system binary from $(SYSTEM_INSTALL_PATH) (requires sudo)"
	@read -p "Confirm uninstall system binary? [y/N] " ans; \
	if [ "$$ans" = "y" ] || [ "$$ans" = "Y" ]; then \
		if sudo test -f "$(SYSTEM_INSTALL_PATH)"; then \
			sudo rm -f "$(SYSTEM_INSTALL_PATH)"; \
			echo "System binary uninstalled from $(SYSTEM_INSTALL_PATH)"; \
		else \
			echo "System binary not found at $(SYSTEM_INSTALL_PATH)"; \
		fi; \
		echo ""; \
		echo "IMPORTANT: If you created a shell alias or function for '$(BIN_NAME)', you must remove it manually."; \
		echo "  This Makefile cannot modify your shell configuration files."; \
	else \
		echo "Uninstall cancelled."; \
	fi

# --- Development & Testing ---
clean:
	@echo "Cleaning build artifacts..."
	@cargo clean

run:
	@echo "Running $(BIN_NAME)..."
	@cargo run --bin file_system

test:
	@echo "Running tests..."
	@cargo test

check:
	@echo "Running cargo check..."
	@cargo check

fmt:
	@echo "Running cargo fmt..."
	@cargo fmt

clippy:
	@echo "Running cargo clippy..."
	@cargo clippy --all-targets -- -D warnings

update:
	@echo "Updating cargo dependencies..."
	@cargo update

# --- Utility Targets ---
repo:
	@echo "Generating repository.txt using repomix..."
	@bash -eu -c '\
		read -p "Enter directories (space-separated): " DIRS; \
		if [ -z "$$DIRS" ]; then \
			echo "Error: at least one directory required." >&2; exit 1; \
		fi; \
		read -p "Enter files (optional, space-separated): " FILES; \
		LIST=""; \
		FAILED=0; \
		for d in $$DIRS; do \
			if [ -d "$$d" ]; then LIST="$$LIST$$d\n"; \
			else echo "Error: Directory not found: $$d" >&2; FAILED=1; fi; \
		done; \
		for f in $$FILES; do \
			if [ -z "$$f" ]; then continue; fi; \
			if [ -f "$$f" ]; then LIST="$$LIST$$f\n"; \
			else echo "Error: File not found: $$f" >&2; FAILED=1; fi; \
		done; \
		if [ "$$FAILED" -eq 1 ]; then exit 2; fi; \
		echo -e "$$LIST" | repomix --stdin --style plain --output repository.txt; \
		if [ -f repository.txt ]; then \
			echo "Success: repository.txt created."; \
		else \
			echo "Error: repository.txt was not generated!" >&2; \
			exit 3; \
		fi; \
	'

status:
	@echo "--- $(BIN_NAME) Status ---"
	@echo "Release binary path: $(RELEASE_BIN_PATH)"
	@if [ -x "$(RELEASE_BIN_PATH)" ]; then \
		echo "  Status: Present and executable"; \
	else \
		echo "  Status: Missing (run 'make build')"; \
	fi; \
	echo "User install path: $(LOCAL_INSTALL_PATH)"; \
	if [ -f "$(LOCAL_INSTALL_PATH)" ]; then \
		echo "  Status: Installed"; \
	else \
		echo "  Status: Not installed"; \
	fi; \
	echo "System install path: $(SYSTEM_INSTALL_PATH)"; \
	if [ -f "$(SYSTEM_INSTALL_PATH)" ]; then \
		echo "  Status: Installed"; \
	else \
		echo "  Status: Not installed"; \
	fi; \
	echo "Command '$(BIN_NAME)' in PATH:"; \
	if command -v $(BIN_NAME) >/dev/null; then \
		echo "  Found at $$(which $(BIN_NAME))"; \
	else \
		echo "  Not found"; \
	fi

dev: check test clippy fmt
	@echo "Development checks complete"

package: build
	@echo "Packaging $(BIN_NAME)..."
	@mkdir -p dist
	@tar czf dist/$(BIN_NAME)_package.tar.gz -C target/release $(BIN_NAME)
	@echo "Package created at dist/$(BIN_NAME)_package.tar.gz"

install-complete: clean build install
	@echo "Complete installation workflow finished."