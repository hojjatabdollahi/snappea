set dotenv-load := true

name := 'snappea'
export APPID := 'io.github.hojjatabdollahi.snappea'

rootdir := ''
prefix := '/usr'

base-dir := absolute_path(clean(rootdir / prefix))

export INSTALL_DIR := base-dir / 'share'

bin-src := 'target' / 'release' / name
bin-dst := base-dir / 'bin' / name

desktop-src := 'data' / 'io.github.hojjatabdollahi.snappea.desktop'
desktop-dst := base-dir / 'share' / 'applications' / 'io.github.hojjatabdollahi.snappea.desktop'

appicon-src := 'data' / 'logo.svg'
appicon-dst := base-dir / 'share' / 'icons' / 'hicolor' / 'scalable' / 'apps' / 'io.github.hojjatabdollahi.snappea.svg'

portal-src := 'data' / 'snappea.portal'
portal-dst := base-dir / 'share' / 'xdg-desktop-portal' / 'portals' / 'snappea.portal'

service-src := 'data' / 'io.github.hojjatabdollahi.snappea.service'
service-dst := base-dir / 'share' / 'dbus-1' / 'services' / 'io.github.hojjatabdollahi.snappea.service'

icons-src := 'data' / 'icons'
icons-dst := base-dir / 'share' / 'icons'

default: build-release

# Compiles in debug mode
build-debug *args:
    cargo build {{args}}

# Compiles in release mode
build-release *args:
    cargo build --release {{args}}

# Check with cargo
check *args:
    cargo check {{args}}

# Cleans build artifacts
clean:
    cargo clean

# Runs with debug profile
run *args:
    cargo run {{args}}

# Install files
install:
    install -Dm0755 {{bin-src}} {{bin-dst}}
    install -Dm0644 {{desktop-src}} {{desktop-dst}}
    install -Dm0644 {{appicon-src}} {{appicon-dst}}
    install -Dm0644 {{portal-src}} {{portal-dst}}
    install -Dm0644 {{service-src}} {{service-dst}}
    install -Dm0644 {{icons-src}}/hicolor/scalable/actions/ocr-symbolic.svg {{icons-dst}}/hicolor/scalable/actions/ocr-symbolic.svg
    install -Dm0644 {{icons-src}}/hicolor/scalable/actions/qr-symbolic.svg {{icons-dst}}/hicolor/scalable/actions/qr-symbolic.svg
    install -Dm0644 {{icons-src}}/hicolor/scalable/actions/arrow-symbolic.svg {{icons-dst}}/hicolor/scalable/actions/arrow-symbolic.svg
    install -Dm0644 {{icons-src}}/hicolor/scalable/actions/circle-symbolic.svg {{icons-dst}}/hicolor/scalable/actions/circle-symbolic.svg
    install -Dm0644 {{icons-src}}/hicolor/scalable/actions/square-symbolic.svg {{icons-dst}}/hicolor/scalable/actions/square-symbolic.svg
    install -Dm0644 {{icons-src}}/hicolor/scalable/actions/redact-symbolic.svg {{icons-dst}}/hicolor/scalable/actions/redact-symbolic.svg
    install -Dm0644 {{icons-src}}/hicolor/scalable/actions/pixelate-symbolic.svg {{icons-dst}}/hicolor/scalable/actions/pixelate-symbolic.svg

# Install portal config to use SnapPea as the default screenshot tool
install-portal:
    mkdir -p ~/.config/xdg-desktop-portal
    echo '[preferred]' > ~/.config/xdg-desktop-portal/portals.conf
    echo 'org.freedesktop.impl.portal.Screenshot=snappea' >> ~/.config/xdg-desktop-portal/portals.conf
    @echo "Portal config installed to ~/.config/xdg-desktop-portal/portals.conf"
    @echo "Run 'systemctl --user restart xdg-desktop-portal' to apply changes"

# Uninstall files
uninstall:
    rm -f {{bin-dst}}
    rm -f {{desktop-dst}}
    rm -f {{appicon-dst}}
    rm -f {{portal-dst}}
    rm -f {{service-dst}}
    rm -f {{icons-dst}}/hicolor/scalable/actions/ocr-symbolic.svg
    rm -f {{icons-dst}}/hicolor/scalable/actions/qr-symbolic.svg
    rm -f {{icons-dst}}/hicolor/scalable/actions/arrow-symbolic.svg
    rm -f {{icons-dst}}/hicolor/scalable/actions/circle-symbolic.svg
    rm -f {{icons-dst}}/hicolor/scalable/actions/square-symbolic.svg
    rm -f {{icons-dst}}/hicolor/scalable/actions/redact-symbolic.svg
    rm -f {{icons-dst}}/hicolor/scalable/actions/pixelate-symbolic.svg
    @if [ -f ~/.config/xdg-desktop-portal/portals.conf ] && grep -q "snappea" ~/.config/xdg-desktop-portal/portals.conf 2>/dev/null; then \
        echo ""; \
        echo "Note: Portal config file exists at ~/.config/xdg-desktop-portal/portals.conf"; \
        echo "You may want to remove the snappea entry or delete the file if no longer needed."; \
    fi