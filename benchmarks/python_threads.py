"""Shared timing for fixed-work Python threading benchmarks."""

import statistics
import time
from concurrent.futures import ThreadPoolExecutor


def median_thread_time(run, count, workers):
    samples = []
    with ThreadPoolExecutor(max_workers=workers) as pool:
        for _ in range(5):
            started = time.perf_counter()
            futures = [pool.submit(run, count // workers) for _ in range(workers)]
            for future in futures:
                future.result()
            samples.append((time.perf_counter() - started) * 1000)
    return statistics.median(samples)
