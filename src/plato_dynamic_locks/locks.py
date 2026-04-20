"""Dynamic lock management with contention tracking."""

import time
from dataclasses import dataclass, field
from typing import Optional

@dataclass
class Lock:
    resource: str
    owner: str
    acquired_at: float = field(default_factory=time.time)
    expires_at: float = 0.0

class DynamicLockManager:
    def __init__(self, default_ttl: float = 300.0):
        self.default_ttl = default_ttl
        self._locks: dict[str, Lock] = {}

    def acquire(self, resource: str, owner: str, ttl: float = None) -> bool:
        if resource in self._locks:
            existing = self._locks[resource]
            if existing.expires_at > 0 and time.time() > existing.expires_at:
                del self._locks[resource]
            else:
                return False
        expiry = time.time() + (ttl or self.default_ttl)
        self._locks[resource] = Lock(resource=resource, owner=owner, expires_at=expiry)
        return True

    def release(self, resource: str, owner: str) -> bool:
        lock = self._locks.get(resource)
        if lock and lock.owner == owner:
            del self._locks[resource]
            return True
        return False

    def is_locked(self, resource: str) -> bool:
        lock = self._locks.get(resource)
        if not lock: return False
        if lock.expires_at > 0 and time.time() > lock.expires_at:
            del self._locks[resource]
            return False
        return True

    def owner_of(self, resource: str) -> Optional[str]:
        lock = self._locks.get(resource)
        return lock.owner if lock and self.is_locked(resource) else None

    def extend(self, resource: str, owner: str, ttl: float = None) -> bool:
        lock = self._locks.get(resource)
        if lock and lock.owner == owner:
            lock.expires_at = time.time() + (ttl or self.default_ttl)
            return True
        return False

    def force_release(self, resource: str) -> bool:
        return self._locks.pop(resource, None) is not None

    def cleanup_expired(self) -> int:
        now = time.time()
        expired = [r for r, l in self._locks.items() if l.expires_at > 0 and now > l.expires_at]
        for r in expired:
            del self._locks[r]
        return len(expired)

    def contention_score(self, resource: str) -> float:
        return 1.0 if self.is_locked(resource) else 0.0

    @property
    def stats(self) -> dict:
        return {"active_locks": len(self._locks),
                "resources": list(self._locks.keys())}
