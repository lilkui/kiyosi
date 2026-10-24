import importlib.util
import math
import unittest
from pathlib import Path

SPEC = importlib.util.spec_from_file_location(
    "sobol_benchmark", Path(__file__).resolve().parents[2] / "benchmarks" / "sobol.py"
)
benchmark = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(benchmark)


def row(method, paths, rmse, seconds):
    return {
        "sampling": method,
        "path_count": paths,
        "rmse": rmse,
        "total_seconds": seconds,
    }


class SobolBenchmarkTests(unittest.TestCase):
    def test_rmse_keeps_opposing_seed_errors(self):
        result = benchmark.summarize([9, 11], 10, [5, 1, 2])
        self.assertEqual(result["rmse"], 1)
        self.assertEqual(result["bias"], 0)
        self.assertEqual(result["total_seconds"], 2)
        asymmetric = benchmark.summarize([12, 16], 10, [1])
        self.assertEqual(asymmetric["rmse"], math.sqrt(20))
        self.assertEqual(asymmetric["bias"], 4)

    def test_equivalent_error_compares_different_measured_path_counts(self):
        rows = [
            row("pseudo_random", 4096, 0.11, 1),
            row("pseudo_random", 32768, 0.04, 8),
            row("sobol", 4096, 0.03, 2),
            row("sobol", 32768, 0.02, 16),
        ]
        result = benchmark.compare_at_target(rows, 0.05, True)
        self.assertEqual(
            result["selected_path_counts"], {"pseudo_random": 32768, "sobol": 4096}
        )
        self.assertEqual(result["runtime_reduction"], 0.75)
        self.assertTrue(result["passed"])

    def test_fast_inaccurate_result_cannot_pass(self):
        rows = [
            row("pseudo_random", 32768, 0.04, 8),
            row("sobol", 4096, 0.10, 0.1),
            row("sobol", 32768, 0.03, 9),
        ]
        result = benchmark.compare_at_target(rows, 0.05, True)
        self.assertEqual(result["selected_path_counts"]["sobol"], 32768)
        self.assertEqual(result["runtime_reduction"], -0.125)
        self.assertFalse(result["passed"])

    def test_unreachable_target_and_unstable_reference_cannot_pass(self):
        rows = [row("pseudo_random", 4096, 0.01, 1), row("sobol", 4096, 0.02, 0.1)]
        unavailable = benchmark.compare_at_target(rows, 0.015, True)
        self.assertIsNone(unavailable["selected_path_counts"]["sobol"])
        self.assertIsNone(unavailable["runtime_reduction"])
        self.assertFalse(unavailable["passed"])
        unstable = benchmark.compare_at_target(rows, 0.02, False)
        self.assertEqual(unstable["runtime_reduction"], 0.9)
        self.assertFalse(unstable["passed"])

    def test_gate_requires_both_european_and_path_dependent_improvement(self):
        self.assertFalse(benchmark.performance_gate({"european": {"passed": True}}))
        self.assertFalse(benchmark.performance_gate({"snowball": {"passed": True}}))
        self.assertTrue(
            benchmark.performance_gate(
                {"european": {"passed": True}, "phoenix": {"passed": True}}
            )
        )
        self.assertFalse(
            benchmark.performance_gate(
                {
                    "european": {"passed": True, "reference_valid": True},
                    "phoenix": {"passed": True, "reference_valid": True},
                    "snowball": {"passed": False, "reference_valid": False},
                }
            )
        )

    def test_twenty_percent_reduction_is_inclusive(self):
        baseline = row("pseudo_random", 32768, 0.01, 1)
        for seconds, expected in ((0.8, True), (0.800001, False)):
            with self.subTest(seconds=seconds):
                result = benchmark.compare_at_target(
                    [baseline, row("sobol", 4096, 0.01, seconds)], 0.01, True
                )
                self.assertEqual(result["passed"], expected)


if __name__ == "__main__":
    unittest.main()
