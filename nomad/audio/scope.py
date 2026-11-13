"""Ring buffer feeding the oscilloscope.

Single writer (audio thread) / single reader (GUI thread), no locks. The scope
is cosmetic: a window() racing a write() may contain a few samples from the
newest block mixed with older ones (torn read), but it never raises.
window(n) returns at most min(n, capacity, written_so_far) samples (shorter,
not zero-padded). NaN samples are stored as-is.
"""
import numpy as np


class ScopeBuffer:
    def __init__(self, sample_rate: float, seconds: float = 2.0) -> None:
        self.capacity = max(1, int(sample_rate * seconds))
        self._buf = np.zeros(self.capacity, dtype=np.float32)
        self._total = 0  # samples ever written; published after the copy

    def write(self, block: np.ndarray) -> None:
        cap = self.capacity
        if len(block) > cap:  # only the latest `cap` samples matter
            self._total += len(block) - cap
            block = block[len(block) - cap:]
        i = self._total % cap
        n1 = min(len(block), cap - i)
        np.copyto(self._buf[i:i + n1], block[:n1], casting="same_kind")
        if n1 < len(block):
            np.copyto(self._buf[:len(block) - n1], block[n1:], casting="same_kind")
        self._total += len(block)

    def window(self, n: int) -> np.ndarray:
        total = self._total
        n = max(0, min(int(n), self.capacity, total))
        end = total % self.capacity
        start = end - n
        if start >= 0:
            return self._buf[start:end].copy()
        return np.concatenate((self._buf[start:], self._buf[:end]))
