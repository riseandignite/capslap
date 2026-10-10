import sys
from pathlib import Path

# Ensure pyside root is in sys.path
pyside_root = Path(__file__).resolve().parent.parent.parent
if str(pyside_root) not in sys.path:
    sys.path.insert(0, str(pyside_root))

from app.main import main

__all__ = ["main"]
