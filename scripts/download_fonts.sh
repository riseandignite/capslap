#!/bin/bash
set -e

# Directory where fonts should be saved
FONTS_DIR="$(dirname "$0")/../rust/src/fonts"
mkdir -p "$FONTS_DIR"

echo "Downloading fonts to $FONTS_DIR..."

download_google_font() {
    local font_name="$1"
    local variant="$2"
    local output_name="$3"
    
    # Clean font name for URL (lowercase, hyphens)
    local clean_name=$(echo "$font_name" | tr '[:upper:]' '[:lower:]' | tr ' ' '-')
    
    # Default to regular/700 if variant not specified or specific map needed
    # GWFH API requires specific variant definition (e.g. 700, 900, regular)
    # We'll try to map common names to weights
    local weight="regular"
    if [[ "$variant" == *"Bold"* ]] || [[ "$variant" == "700" ]]; then weight="700"; fi
    if [[ "$variant" == "Black" ]] || [[ "$variant" == *"ExtraBold"* ]]; then weight="900"; fi
    if [[ "$variant" == "SemiBold" ]]; then weight="600"; fi
    
    # URL for zip download from google-webfonts-helper
    local url="https://gwfh.mranftl.com/api/fonts/${clean_name}?download=zip&subsets=latin&variants=${weight}"
    
    echo "Downloading ${font_name} (${weight}) from ${url}..."
    if ! curl -L -s "$url" -o temp_font.zip; then
        echo "Failed to download $font_name"
        return
    fi

    # Check if file is actually a zip
    if ! unzip -t temp_font.zip >/dev/null 2>&1; then
        echo "Error: Downloaded file for $font_name is not a valid zip archive (likely font not found or API error)."
        rm temp_font.zip
        return
    fi
    
    unzip -j -o temp_font.zip -d "$FONTS_DIR"
    rm temp_font.zip
}

# --- Modern / Sans ---
# Open Sans (Already have Roboto, Montserrat, Kanit, Poppins, WorkSans)
download_google_font "Open Sans" "ExtraBold"
download_google_font "Lato" "Black"
download_google_font "Raleway" "Black"

# --- Display / Impact ---
# Bebas Neue
download_google_font "Bebas Neue" ""
# Anton (Alternative to Impact)
download_google_font "Anton" ""
# Lilita One
download_google_font "Lilita One" ""

# --- Fun / Comic ---
# Comic Neue
download_google_font "Comic Neue" "Bold"
# Fredoka
download_google_font "Fredoka" "SemiBold"
# Chewy
download_google_font "Chewy" ""
# Luckiest Guy
download_google_font "Luckiest Guy" ""

# --- Serif / Elegant ---
# Playfair Display
download_google_font "Playfair Display" "Bold"
# Merriweather
download_google_font "Merriweather" "Black"
# Lora
download_google_font "Lora" "Bold"
# Cinzel
download_google_font "Cinzel" "Bold"
# Bodoni Moda
download_google_font "Bodoni Moda" "Bold" # Note: Variable font usually

# --- Handwritten / Script ---
# Permanent Marker
download_google_font "Permanent Marker" ""
# Patrick Hand
download_google_font "Patrick Hand" ""
# Amatic SC
download_google_font "Amatic SC" "Bold"
# Caveat Brush
download_google_font "Caveat Brush" ""
# Pacifico
download_google_font "Pacifico" ""

echo "Cleaning up non-font files..."
find "$FONTS_DIR" -type f ! -name "*.ttf" ! -name "*.otf" -delete

echo "Font download complete!"
ls -lh "$FONTS_DIR"
