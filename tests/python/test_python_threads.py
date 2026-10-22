"""Check fixed work and worker failures in the Python benchmark harness."""

import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "benchmarks"))

from python_threads import median_thread_time


class PythonThreadBenchmarkTests(unittest.TestCase):
    def test_fixed_work_is_split_across_workers_for_every_sample(self):
        for workers in (1, 4):
            with self.subTest(workers=workers):
                counts = []
                median_thread_time(counts.append, 12, workers)
                self.assertEqual(counts, [12 // workers] * (5 * workers))

    def test_worker_failure_reaches_the_caller(self):
        def fail(count):
            raise RuntimeError("pricing failed")

        with self.assertRaisesRegex(RuntimeError, "pricing failed"):
            median_thread_time(fail, 12, 4)
