# SnapPea - Skärmdump och skärminspelningsprogram
# Swedish (sv) översättningar

# Allmänna åtgärder
cancel = Avbryt
capture = Fånga
settings = Inställningar

# Spara platser
save-to = Spara till
    .clipboard = { save-to } Urklipp
    .pictures = { save-to } Bilder
    .documents = { save-to } Dokument

# Verktygsfältets verktygstips
move-toolbar = Flytta verktygsfältet (Ctrl+hjkl)
screenshot-video = Skärmdump / video
select-region = Välj region (R)
select-screen = Välj Skärm (S)

# Kontextkänsliga verktygstips för kopiering/spara
copy-selected-region = Kopiera vald region (Enter)
copy-selected-screen = Kopiera vald skärm (Enter)
copy-all-screens = Kopiera alla skärmar (Enter)
copy-screen = Kopiera skärm (Enter)

save-selected-region = Spara vald region (Ctrl+Enter)
save-selected-screen = Spara vald skärm (Ctrl+Enter)
save-all-screens = Spara alla skärmar (Ctrl+Enter)
save-screen = Spara skärm (Ctrl+Enter)

# Inspelning
record-selection = Spela in val (Shift+R)
record-disabled = Inaktiverad: välj en region eller skärm först
stop-recording = Stoppa inspelning
freehand-annotation = Frihandsannotering (högerklicka för alternativ)
minimize-to-tray = Minimera till systemfältet

# OCR verktygstips
copy-ocr-text = Kopiera OCR text (O)
recognize-text = Tolka text (O)
install-tesseract = Installera tesseract för att aktivera OCR

# QR verktygstips
copy-qr-code = Kopiera QR-kod (Q)
scan-qr-code = Skanna QR-kod  (Q)

# Avbryt knapp
cancel-escape = Avbryt (Escape)

# Färger
color-red = Röd
color-green = Grön
color-blue = Blå
color-yellow = Gul
color-orange = Orange
color-purple = Lila
color-white = Vit
color-black = Svart

# Formverktyg
arrow = Pil
oval-circle = Oval eller cirkel
rectangle-square = Rektangel eller fyrkant
shape-cycle-hint = Skift+A för att växla mellan former, A för att växla
color = Färg
shadow = Skugga
clear-annotations = Rensa annoteringar

# Redigeringsverktyg
redact-blackout = Skärma bort (svärta över)
pixelate-blur = Pixelera (oskärpa)
redact-cycle-hint = Skift+D för att växla mellan verktyg, D för att växla
pixelation-size = Pixelering: { $size }px
clear-redactions = Rensa borttagningar

# Blyertspenna/teckning inställningar
thickness = Tjocklek: { $size }px
fade-duration = Blekna: { $duration }s
clear-drawings = Rensa ritningar

# Verktygstips för formverktyg
draw-arrow = Rita pil (A, högerklicka för inställningar)
draw-circle = Rita cirkel (A, Ctrl för perfekt, högerklicka för inställningar)
draw-rectangle = Rita rektangel (A, Ctrl för kvadrat, högerklicka för inställningar)

# Verktygstips för redigeringsverktyg
redact-tool = Redigera (D, högerklicka för inställningar)
pixelate-tool = Pixelera (D, högerklicka för inställningar)

# Inställningslådans flikar
general = Allmänt
picture = Bild
video = Video

# Inställningslåda - Allmänt
magnifier = Förstoringsglas
set-as-default-portal = Ställ in som standard
set-as-default-portal-description = Använd SnapPea som standardportal för skärmdumpar för ditt system
toolbar-opacity = Verktygsfältets opacitet (inaktiv): { $percent }%
app-name = SnapPea
app-author = av Hojjat Abdollahi

# Inställningslåda - Bild
save-location = Spara till:
pictures = Bilder
documents = Dokument
custom = Anpassad
browse = Bläddra...
copy-on-save = Kopiera vid sparning

# Inställningslåda - Plats för att spara video
video-save-location = Spara videor till:
videos = Video

# Inställningslåda - Video
encoder = Kodare:
format = Format:
format-mp4 = MP4
format-webm = WebM
format-mkv = MKV
framerate = Bildfrekvens:
fps-24 = 24 fps
fps-30 = 30 fps
fps-60 = 60 fps
show-cursor = Visa markör
hide-to-tray = Dölj till fält vid inspelning

# Systemfält
tray-title = Snappea inspelning
tray-tooltip-title = Snappea - inspelning
tray-tooltip-desc = Klicka för att stoppa inspelning
hide-toolbar = Dölj fält
show-toolbar = Visa fält

# OCR meddelanden
no-text-detected = Ingen text upptäckt
invalid-ocr-scale = Ogiltig OCR-mappningsskala
tesseract-image-error = Misslyckades att skapa tesseract bild: { $error }
tesseract-ocr-error = Tesseract OCR misslyckades: { $error }

# Kommandoradsanvändning
cli-usage = Användning: snappea --record --output FILE --output-name NAME --region X,Y,W,H --logical-size W,H --encoder ENC [--container FMT] [--framerate FPS] [--toplevel-index IDX]
cli-missing-args = Saknar obligatoriska argument för --record
