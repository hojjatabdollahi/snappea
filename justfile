set dotenv-load := true

name := 'cosmic-x'
trim := 'cosmic-x-trim'
export APPID := 'com.system76.CosmicX'

rootdir := ''
prefix := '/usr'

base-dir := absolute_path(clean(rootdir / prefix))

export INSTALL_DIR := base-dir / 'share'

cargo-target-dir := env('CARGO_TARGET_DIR', 'target')

bin-src := cargo-target-dir / 'release' / name
bin-dst := base-dir / 'bin' / name

trim-bin-src := cargo-target-dir / 'release' / trim
trim-bin-dst := base-dir / 'bin' / trim

desktop-src := name / 'data' / 'com.system76.CosmicX.desktop'
desktop-dst := base-dir / 'share' / 'applications' / 'com.system76.CosmicX.desktop'

trim-desktop-src := trim / 'res' / 'com.system76.CosmicX.Trim.desktop'
trim-desktop-dst := base-dir / 'share' / 'applications' / 'com.system76.CosmicX.Trim.desktop'

metainfo-src := name / 'data' / 'com.system76.CosmicX.metainfo.xml'
metainfo-dst := base-dir / 'share' / 'metainfo' / 'com.system76.CosmicX.metainfo.xml'

appicon-src := name / 'data' / 'logo.svg'
appicon-dst := base-dir / 'share' / 'icons' / 'hicolor' / 'scalable' / 'apps' / 'com.system76.CosmicX.svg'

icons-src := name / 'data' / 'icons' / 'scalable' / 'actions'
trim-icons-src := trim / 'res' / 'icons' / 'scalable' / 'actions'
icons-dst := base-dir / 'share' / 'icons' / 'hicolor' / 'scalable' / 'actions'

portal-src := name / 'data' / 'cosmic-x.portal'
portal-dst := base-dir / 'share' / 'xdg-desktop-portal' / 'portals' / 'cosmic-x.portal'

service-src := name / 'data' / 'com.system76.CosmicX.service'
service-dst := base-dir / 'share' / 'dbus-1' / 'services' / 'com.system76.CosmicX.service'

default: build-release

# Compiles the whole workspace in debug mode
build-debug *args:
    cargo build {{args}}

# Compiles the whole workspace in release mode
build-release *args:
    cargo build --release {{args}}

# Check with cargo
check *args:
    cargo clippy --workspace --all-targets {{args}}

test:
    cargo test --workspace

# Cleans build artifacts
clean:
    cargo clean

# Removes vendored dependencies
clean-vendor:
    rm -rf .cargo vendor vendor.tar

# Compiles release profile with vendored dependencies
build-vendored *args: vendor-extract (build-release '--frozen --offline' args)

# Vendor dependencies locally
vendor:
    #!/usr/bin/env bash
    mkdir -p .cargo
    cargo vendor --sync Cargo.toml | head -n -1 > .cargo/config.toml
    echo 'directory = "vendor"' >> .cargo/config.toml
    tar pcf vendor.tar .cargo vendor
    rm -rf .cargo vendor

# Extracts vendored dependencies
vendor-extract:
    rm -rf vendor
    tar pxf vendor.tar

# Runs the capture app
run *args:
    cargo run -p {{name}} -- {{args}}

# Runs the trimmer on a clip
run-trim *args:
    cargo run -p {{trim}} -- {{args}}

# Build a .deb package (works correctly in git worktrees)
deb *args:
    cargo build --release --locked {{args}}
    SOURCE_DATE_EPOCH=$(git log -1 --format=%ct) cargo deb -p {{name}} --no-build

# `install` runs as root; building there would leave target/ root-owned.
_built:
    @for bin in {{bin-src}} {{trim-bin-src}}; do \
        if [ ! -f "$bin" ]; then \
            echo "error: $bin not found — run 'just' first, as your own user." >&2; \
            echo "       (with CARGO_TARGET_DIR set, use 'sudo -E just install')" >&2; \
            exit 1; \
        fi; \
    done

# Install files
install: _built
    install -Dm0755 {{bin-src}} {{bin-dst}}
    install -Dm0755 {{trim-bin-src}} {{trim-bin-dst}}
    install -Dm0644 {{desktop-src}} {{desktop-dst}}
    install -Dm0644 {{trim-desktop-src}} {{trim-desktop-dst}}
    install -Dm0644 {{metainfo-src}} {{metainfo-dst}}
    install -Dm0644 {{appicon-src}} {{appicon-dst}}
    install -Dm0644 -t {{icons-dst}} {{icons-src}}/*.svg
    install -Dm0644 -t {{icons-dst}} {{trim-icons-src}}/*.svg
    install -Dm0644 {{portal-src}} {{portal-dst}}
    install -Dm0644 {{service-src}} {{service-dst}}
    @just rootdir={{rootdir}} prefix={{prefix}} _refresh-caches
    @echo ""
    @echo "Installed cosmic-x and cosmic-x-trim to {{base-dir}}/bin."
    @echo "Run 'just install-portal' (as your user, not root) to make COSMIC X"
    @echo "the default screenshot tool."

# Skipped under a rootdir, where the package manager owns the caches.
_refresh-caches:
    @if [ -z "{{rootdir}}" ]; then \
        if command -v update-desktop-database >/dev/null 2>&1; then \
            update-desktop-database -q "{{base-dir}}/share/applications" || true; \
        fi; \
        if command -v gtk-update-icon-cache >/dev/null 2>&1; then \
            gtk-update-icon-cache -qtf "{{base-dir}}/share/icons/hicolor" || true; \
        fi; \
    fi

# Report which optional runtime tools are present
doctor:
    @for tool in ffmpeg gst-launch-1.0 tesseract; do \
        if command -v "$tool" >/dev/null 2>&1; then \
            echo "  ok      $tool"; \
        else \
            echo "  missing $tool"; \
        fi; \
    done
    @echo ""
    @echo "ffmpeg and GStreamer are required by cosmic-x-trim."
    @echo "tesseract enables OCR."

# Per-user, so resolve the invoking user's home even under sudo.
portal-conf := '"$(getent passwd "${SUDO_USER:-$(id -un)}" | cut -d: -f6)/.config/xdg-desktop-portal/cosmic-portals.conf"'

# Install portal config to use COSMIC X as the default screenshot tool
install-portal:
    @CONF={{portal-conf}}; \
    mkdir -p "$(dirname "$CONF")"; \
    printf '[preferred]\ndefault=cosmic;gtk;\norg.freedesktop.impl.portal.Screenshot=cosmic-x\n' > "$CONF"; \
    echo "Portal config written to $CONF"; \
    echo "Run 'systemctl --user restart xdg-desktop-portal' to apply."

# Uninstall files
uninstall:
    rm -f {{bin-dst}}
    rm -f {{trim-bin-dst}}
    rm -f {{desktop-dst}}
    rm -f {{trim-desktop-dst}}
    rm -f {{metainfo-dst}}
    rm -f {{appicon-dst}}
    @for f in {{icons-src}}/*.svg {{trim-icons-src}}/*.svg; do \
        rm -f "{{icons-dst}}/$(basename "$f")"; \
    done
    rm -f {{portal-dst}}
    rm -f {{service-dst}}
    @just rootdir={{rootdir}} prefix={{prefix}} _refresh-caches
    @OURS="$(printf '[preferred]\ndefault=cosmic;gtk;\norg.freedesktop.impl.portal.Screenshot=cosmic-x\n')"; \
    CONF={{portal-conf}}; \
    if [ -f "$CONF" ]; then \
        if [ "$(cat "$CONF")" = "$OURS" ]; then \
            rm -f "$CONF"; \
            echo "Removed $CONF"; \
        else \
            echo "Note: $CONF has custom content — not removing."; \
        fi; \
    fi
