"""Print paired pricing timings from Google Benchmark JSON output."""

import argparse
import json
from pathlib import Path
from statistics import median


def report(path: Path) -> str:
    data = json.loads(path.read_text(encoding="utf-8"))
    cases: dict[str, dict[str, list[dict]]] = {}
    prefixes = {
        "matrix/kiyosi/": "kiyosi",
        "matrix/kiyosi_only/": "kiyosi_only",
        "matrix/quantlib/": "quantlib",
    }
    for row in data["benchmarks"]:
        name = row["name"]
        if row.get("error_occurred"):
            raise ValueError(f"{name}: {row.get('error_message', 'benchmark failed')}")
        for prefix, library in prefixes.items():
            if name.startswith(prefix):
                if row.get("run_type") == "aggregate":
                    if row.get("aggregate_name") != "median":
                        break
                    name = name.removesuffix("_median")
                name = name.removesuffix("/real_time")
                cases.setdefault(name[len(prefix) :], {}).setdefault(library, []).append(row)
                break
    if not cases:
        raise ValueError("JSON contains no matrix benchmarks")

    def measurement(rows: list[dict], wall_time: bool = False) -> tuple[float, float]:
        iterations = [row for row in rows if row.get("run_type") != "aggregate"]
        selected = iterations or rows
        units = {"ns": 1.0, "us": 1e3, "ms": 1e6, "s": 1e9}
        clock = "real_time" if wall_time else "cpu_time"
        return (median(row[clock] * units[row["time_unit"]] for row in selected),
                median(row["price"] for row in selected))

    lines = ["| Case | Kiyosi ns | QuantLib ns | QuantLib / Kiyosi | Price delta (K - Q) |",
             "| --- | ---: | ---: | ---: | ---: |"]
    for name, libraries in sorted(cases.items()):
        own = libraries.get("kiyosi") or libraries.get("kiyosi_only")
        if not own:
            raise ValueError(f"QuantLib case has no Kiyosi counterpart: {name}")
        own_ns, own_price = measurement(own, name.endswith("_cuda"))
        other = libraries.get("quantlib")
        label = f"{name} (Kiyosi only)" if "kiyosi_only" in libraries else name
        if other:
            other_ns, other_price = measurement(other)
            ratio = f"{other_ns / own_ns:.2f}x" if own_ns else "n/a"
            lines.append(f"| {label} | {own_ns:.1f} | {other_ns:.1f} | "
                         f"{ratio} | {own_price - other_price:.6g} |")
        else:
            lines.append(f"| {label} | {own_ns:.1f} | n/a | n/a | n/a |")
    return "\n".join(lines)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("json", type=Path, help="Google Benchmark JSON output")
    print(report(parser.parse_args().json))
