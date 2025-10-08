#!/bin/bash

# Fix production build by creating models directory structure
# and copying the latest core binary

echo "Fixing production builds..."

# Create models directories for both architectures
mkdir -p dist/mac/CapSlap.app/Contents/Resources/models
mkdir -p dist/mac-arm64/CapSlap.app/Contents/Resources/models

# Copy latest core binary
if [ -f "../rust/target/release/core" ]; then
    echo "Copying latest core binary..."
    cp ../rust/target/release/core dist/mac/CapSlap.app/Contents/Resources/core
    cp ../rust/target/release/core dist/mac-arm64/CapSlap.app/Contents/Resources/core
fi

# Optional: Copy pre-downloaded models if they exist
if [ -f "../rust/models/ggml-tiny.bin" ]; then
    echo "Copying tiny model to production builds..."
    cp ../rust/models/ggml-tiny.bin dist/mac/CapSlap.app/Contents/Resources/models/
    cp ../rust/models/ggml-tiny.bin dist/mac-arm64/CapSlap.app/Contents/Resources/models/
fi

echo "Production build fix completed!"
