# SnapPea - Screenshot and Screen Recording Application
# Czech (cs) translations

# General actions
cancel = Zrušit
capture = Zachytit
settings = Nastavení

# Save locations
save-to = Uložit do
    .clipboard = { save-to } schránky
    .pictures = { save-to } obrázků
    .documents = { save-to } dokumentů

# Toolbar tooltips
move-toolbar = Přesunout panel nástrojů (Ctrl+hjkl)
screenshot-video = Snímek obrazovky / Video
select-region = Vybrat oblast (R)
select-screen = Vybrat obrazovku (S)

# Context-sensitive copy/save tooltips
copy-selected-region = Kopírovat vybranou oblast (Enter)
copy-selected-screen = Kopírovat vybranou obrazovku (Enter)
copy-all-screens = Kopírovat všechny obrazovky (Enter)
copy-screen = Kopírovat obrazovku (Enter)

save-selected-region = Uložit vybranou oblast (Ctrl+Enter)
save-selected-screen = Uložit vybranou obrazovku (Ctrl+Enter)
save-all-screens = Uložit všechny obrazovky (Ctrl+Enter)
save-screen = Uložit obrazovku (Ctrl+Enter)

# Recording
record-selection = Nahrát výběr (Shift+R)
record-disabled = Zakázáno: nejprve vyberte oblast nebo obrazovku
stop-recording = Zastavit nahrávání
freehand-annotation = Volné kreslení (pravé tlačítko pro možnosti)
minimize-to-tray = Minimalizovat do oznamovací oblasti

# OCR tooltips
copy-ocr-text = Kopírovat OCR text (O)
recognize-text = Rozpoznat text (O)
install-tesseract = Nainstalujte tesseract pro povolení OCR

# QR tooltips
copy-qr-code = Kopírovat QR kód (Q)
scan-qr-code = Skenovat QR kód (Q)

# Cancel button
cancel-escape = Ukončit (Escape)

# Colors
color-red = Červená
color-green = Zelená
color-blue = Modrá
color-yellow = Žlutá
color-orange = Oranžová
color-purple = Fialová
color-white = Bílá
color-black = Černá

# Shape tools
arrow = Šipka
oval-circle = Ovál nebo kruh
rectangle-square = Obdélník nebo čtverec
shape-cycle-hint = Shift+A pro přepínání tvarů, A pro zapnutí/vypnutí
color = Barva
shadow = Stín
clear-annotations = Vymazat kreslení

# Redact tools
redact-blackout = Skrýt (začernit)
pixelate-blur = Pixelizovat (rozmazat)
redact-cycle-hint = Shift+D pro přepínání nástrojů, D pro zapnutí/vypnutí
pixelation-size = Pixelizace: { $size } px
clear-redactions = Vymazat skrytí

# Pencil/drawing settings
thickness = Tloušťka: { $size } px
fade-duration = Zmizení: { $duration } s
clear-drawings = Vymazat kresby

# Shape tool tooltips
draw-arrow = Nakreslit šipku (A, pravé tlačítko pro nastavení)
draw-circle = Nakreslit kruh (A, Ctrl pro dokonalý tvar, pravé tlačítko pro nastavení)
draw-rectangle = Nakreslit obdélník (A, Ctrl pro čtverec, pravé tlačítko pro nastavení)

# Redact tool tooltips
redact-tool = Skrýt (D, pravé tlačítko pro nastavení)
pixelate-tool = Pixelizovat (D, pravé tlačítko pro nastavení)

# Settings drawer tabs
general = Obecné
picture = Obrázek
video = Video

# Settings drawer - General
magnifier = Lupa
set-as-default-portal = Nastavit jako výchozí
set-as-default-portal-description = Použít SnapPea jako výchozí portál pro snímky obrazovky ve vašem systému
toolbar-opacity = Průhlednost panelu nástrojů (neaktivního): { $percent } %
app-name = SnapPea
app-author = od Hojjata Abdollahiho
app-version = v{ $version } ({ $hash })

# Settings drawer - Picture
save-location = Uložit do:
pictures = Obrázky
documents = Dokumenty
custom = Vlastní
browse = Procházet...
copy-on-save = Kopírovat při uložení

# Settings drawer - Video save location
video-save-location = Uložit videa do:
videos = Videa

# Settings drawer - Video
encoder = Enkodér:
format = Formát:
format-mp4 = MP4
format-webm = WebM
format-mkv = MKV
framerate = Snímková frekvence:
fps-24 = 24 fps
fps-30 = 30 fps
fps-60 = 60 fps
show-cursor = Zobrazit kurzor
hide-to-tray = Skrýt do oznamovací oblasti při nahrávání

# System tray
tray-title = Nahrávání Snappea
tray-tooltip-title = Snappea - Nahrávání
tray-tooltip-desc = Klikněte pro zastavení nahrávání
hide-toolbar = Skrýt panel nástrojů
show-toolbar = Zobrazit panel nástrojů

# OCR messages
no-text-detected = Nebyl rozpoznán žádný text
invalid-ocr-scale = Neplatné měřítko OCR mapování
tesseract-image-error = Nepodařilo se vytvořit obrázek pro tesseract: { $error }
tesseract-ocr-error = OCR Tesseract selhalo: { $error }

# Screen selection hints
select-screen-hint = Klikněte nebo stiskněte Enter pro výběr této obrazovky
select-screen-navigate = Klikněte pro výběr • Pro navigaci použijte šipky
change-selection-hint = Stiskněte „S“ nebo tlačítko Obrazovka pro změnu výběru

# File dialogs
browse-screenshots-title = Vyberte umístění pro ukládání snímků obrazovky
browse-videos-title = Vyberte umístění pro ukládání videí

# Status overlays
scanning-qr = Vyhledávání QR kódů...
downloading-ocr-models = Stahování OCR modelů...
running-ocr = Probíhá OCR...
ocr-error = Chyba OCR: { $error }

# Notifications
recording-saved = Nahrávka uložena
saved-to = Uloženo do { $path }

# Default screenshot filename prefix
screenshot-filename-prefix = Snímek obrazovky

# Command line usage
cli-usage = Použití: snappea --record --output SOUBOR --output-name NÁZEV --region X,Y,W,H --logical-size W,H --encoder ENC [--container FMT] [--framerate FPS] [--toplevel-index IDX]
cli-missing-args = Chybí povinné argumenty pro --record

# Media editor
edit-title = Úpravy SnapPea
edit-about = O aplikaci
edit-system-tools = Systémové nástroje
edit-ffmpeg-version = ffmpeg: { $version }
edit-ffmpeg-not-installed = ffmpeg: není nainstalován (potřebný pro ořez videa a převod do GIF)
edit-gifski-version = gifski: { $version }
edit-gifski-not-installed = gifski: není nainstalován (cargo install gifski pro kvalitnější GIFy)
edit-loading = Načítání
edit-loading-video = Načítání videa...
edit-encoding-gif = Enkódování GIFu...
edit-loaded = Načteno
edit-no-media = Žádná média
edit-save = Uložit
edit-save-as = Uložit jako...
edit-discard = Zahodit
edit-format = Formát
edit-format-gif = GIF
edit-format-video = Video
edit-scale = Měřítko
edit-trim = Ořez
edit-trimmed-filename = { $stem }-oříznuto.{ $ext }
edit-optimize-ffmpeg = Optimalizovat (ffmpeg)
edit-saved-size = Uloženo ({ $size } KB)
edit-saved-path-size = Uloženo { $path } ({ $size } KB)
edit-save-failed = Uložení se nezdařilo: { $error }
edit-copied-to-clipboard = Zkopírováno do schránky
edit-copied-file-path = Cesta k souboru zkopírována do schránky
edit-copy-failed = Kopírování se nezdařilo: { $error }
edit-copied = Zkopírováno
edit-cut = Vystřihnout
edit-delete-chunk = Smazat
edit-undelete-chunk = Obnovit
edit-cuts = Vystřižené části:
edit-chunk = Část
edit-deleted = smazáno
edit-speed = Rychlost
