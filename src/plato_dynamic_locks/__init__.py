"""Dynamic lock management — acquire, release, contention tracking, timeout.
Part of the PLATO framework."""
from .locks import DynamicLockManager
__version__ = "0.1.0"
__all__ = ["DynamicLockManager"]
