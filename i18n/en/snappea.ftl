# SnapPea - Screenshot and Screen Recording Application
# English (en) translations

# General actions
cancel = Cancel
capture = Capture
settings = Settings

# Save locations
save-to = Save to
    .clipboard = { save-to } Clipboard
    .pictures = { save-to } Pictures
    .documents = { save-to } Documents

# Toolbar tooltips
move-toolbar = Move Toolbar (Ctrl+hjkl)
screenshot-video = Screenshot / Video
select-region = Select Region (R)
select-screen = Select Screen (S)

# Context-sensitive copy/save tooltips
copy-selected-region = Copy Selected Region (Enter)
copy-selected-screen = Copy Selected Screen (Enter)
copy-all-screens = Copy All Screens (Enter)
copy-screen = Copy Screen (Enter)

save-selected-region = Save Selected Region (Ctrl+Enter)
save-selected-screen = Save Selected Screen (Ctrl+Enter)
save-all-screens = Save All Screens (Ctrl+Enter)
save-screen = Save Screen (Ctrl+Enter)

# Recording
record-selection = Record selection (Shift+R)
record-disabled = Disabled: select a region or screen first
stop-recording = Stop Recording
freehand-annotation = Freehand Annotation (right-click for options)
minimize-to-tray = Minimize to System Tray

# OCR tooltips
copy-ocr-text = Copy OCR Text (O)
recognize-text = Recognize Text (O)
install-tesseract = Install tesseract to enable OCR

# QR tooltips
copy-qr-code = Copy QR Code (Q)
scan-qr-code = Scan QR Code (Q)

# Cancel button
cancel-escape = Cancel (Escape)

# Colors
color-red = Red
color-green = Green
color-blue = Blue
color-yellow = Yellow
color-orange = Orange
color-purple = Purple
color-white = White
color-black = Black

# Shape tools
arrow = Arrow
oval-circle = Oval or Circle
rectangle-square = Rectangle or Square
shape-cycle-hint = Shift+A to cycle shapes, A to toggle
color = Color
shadow = Shadow
clear-annotations = Clear Annotations

# Redact tools
redact-blackout = Redact (black out)
pixelate-blur = Pixelate (blur out)
redact-cycle-hint = Shift+D to cycle tools, D to toggle
pixelation-size = Pixelation: { $size }px
clear-redactions = Clear Redactions

# Pencil/drawing settings
thickness = Thickness: { $size }px
fade-duration = Fade: { $duration }s
clear-drawings = Clear Drawings

# Shape tool tooltips
draw-arrow = Draw Arrow (A, right-click for settings)
draw-circle = Draw Circle (A, Ctrl for perfect, right-click for settings)
draw-rectangle = Draw Rectangle (A, Ctrl for square, right-click for settings)

# Redact tool tooltips
redact-tool = Redact (D, right-click for settings)
pixelate-tool = Pixelate (D, right-click for settings)

# Settings drawer tabs
general = General
picture = Picture
video = Video

# Settings drawer - General
magnifier = Magnifier
set-as-default-portal = Set as Default
set-as-default-portal-description = Use SnapPea as the default screenshot portal for your system
toolbar-opacity = Toolbar opacity (idle): { $percent }%
app-name = SnapPea
app-author = by Hojjat Abdollahi
app-version = v{ $version } ({ $hash })

# Settings drawer - Picture
save-location = Save to:
pictures = Pictures
documents = Documents
custom = Custom
browse = Browse...
copy-on-save = Copy on save

# Settings drawer - Video save location
video-save-location = Save videos to:
videos = Videos

# Settings drawer - Video
encoder = Encoder:
format = Format:
format-mp4 = MP4
format-webm = WebM
format-mkv = MKV
framerate = Framerate:
fps-24 = 24 fps
fps-30 = 30 fps
fps-60 = 60 fps
show-cursor = Show cursor
hide-to-tray = Hide to tray when recording

# System tray
tray-title = Snappea Recording
tray-tooltip-title = Snappea - Recording
tray-tooltip-desc = Click to stop recording
hide-toolbar = Hide Toolbar
show-toolbar = Show Toolbar

# OCR messages
no-text-detected = No text detected
invalid-ocr-scale = Invalid OCR mapping scale
tesseract-image-error = Failed to create tesseract image: { $error }
tesseract-ocr-error = Tesseract OCR failed: { $error }

# Screen selection hints
select-screen-hint = Click or press Enter to select this screen
select-screen-navigate = Click to select • Arrow keys to navigate
change-selection-hint = Press 'S' or Screen button to change selection

# File dialogs
browse-screenshots-title = Select save location for screenshots
browse-videos-title = Select save location for videos

# Status overlays
scanning-qr = Scanning for QR codes...
downloading-ocr-models = Downloading OCR models...
running-ocr = Running OCR...
ocr-error = OCR error: { $error }

# Notifications
recording-saved = Recording Saved
saved-to = Saved to { $path }

# Default screenshot filename prefix
screenshot-filename-prefix = Screenshot

# Command line usage
cli-usage = Usage: snappea --record --output FILE --output-name NAME --region X,Y,W,H --logical-size W,H --encoder ENC [--container FMT] [--framerate FPS] [--toplevel-index IDX]
cli-missing-args = Missing required arguments for --record

# Media editor
edit-title = SnapPea Edit
edit-about = About
edit-system-tools = System Tools
edit-ffmpeg-version = ffmpeg: { $version }
edit-ffmpeg-not-installed = ffmpeg: not installed (needed for video trimming and GIF conversion)
edit-gifski-version = gifski: { $version }
edit-gifski-not-installed = gifski: not installed (cargo install gifski for higher quality GIFs)
edit-loading = Loading
edit-loading-video = Loading video...
edit-encoding-gif = Encoding GIF...
edit-loaded = Loaded
edit-no-media = No media
edit-save = Save
edit-save-as = Save As...
edit-discard = Discard
edit-format = Format
edit-format-gif = GIF
edit-format-video = Video
edit-scale = Scale
edit-trim = Trim
edit-trimmed-filename = { $stem }-trimmed.{ $ext }
edit-optimize-ffmpeg = Optimize (ffmpeg)
edit-saved-size = Saved ({ $size } KB)
edit-saved-path-size = Saved { $path } ({ $size } KB)
edit-save-failed = Save failed: { $error }
edit-copied-to-clipboard = Copied to clipboard
edit-copied-file-path = Copied file path to clipboard
edit-copy-failed = Copy failed: { $error }
edit-copied = Copied
edit-cut = Cut
edit-delete-chunk = Delete
edit-undelete-chunk = Restore
edit-cuts = Cuts:
edit-chunk = Chunk
edit-deleted = deleted
edit-speed = Speed
