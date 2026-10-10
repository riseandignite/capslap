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

# We don't bundle models by default to keep binary size small.
# The app will download them to this directory on demand.
echo "Production build fix completed!"
