set dotenv-load := true

name := 'blazingshot'
export APPID := 'org.freedesktop.impl.portal.blazingshot'

rootdir := ''
prefix := '/usr'

base-dir := absolute_path(clean(rootdir / prefix))

export INSTALL_DIR := base-dir / 'share'

bin-src := 'target' / 'release' / name
bin-dst := base-dir / 'libexec' / name

portal-src := 'data' / 'blazingshot.portal'
portal-dst := base-dir / 'share' / 'xdg-desktop-portal' / 'portals' / 'blazingshot.portal'

service-src := 'data' / 'org.freedesktop.impl.portal.blazingshot.service'
service-dst := base-dir / 'share' / 'dbus-1' / 'services' / 'org.freedesktop.impl.portal.blazingshot.service'

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
    install -Dm0644 {{portal-src}} {{portal-dst}}
    install -Dm0644 {{service-src}} {{service-dst}}

# Uninstall files
uninstall:
    rm -f {{bin-dst}}
    rm -f {{portal-dst}}
    rm -f {{service-dst}}
