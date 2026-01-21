# SnapPea

A screenshot portal implementation for the COSMIC desktop environment with annotation capabilities.

## Features

- Interactive screenshot selection
- Annotation tools: arrows, circles, squares
- Text recognition (OCR)
- QR code detection
- Redaction and pixelation
- Multi-window and multi-output support
- Keyboard shortcuts

## Installation

Build and install with [just](https://github.com/casey/just):

```sh
just
sudo just install
```

To uninstall:

```sh
sudo just uninstall
```

> [!IMPORTANT]
To override the default screenshot portal, create or edit `~/.config/xdg-desktop-portal/portals.conf`:

```ini
[preferred]
org.freedesktop.impl.portal.Screenshot=snappea
```

and run

```bash
systemctl --user restart xdg-desktop-portal
```

## License

GPL-3.0-or-later
