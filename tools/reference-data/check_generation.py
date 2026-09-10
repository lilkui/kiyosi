"""Runnable checks for regeneration and failure atomicity; standard library only."""

from copy import deepcopy
import json
from pathlib import Path
import tempfile
from unittest.mock import patch

import generate


def check_generation():
    original = generate.FIXTURE.read_bytes()
    scenarios = json.loads((generate.PROJECT / "scenarios.json").read_text())
    with tempfile.TemporaryDirectory() as directory:
        fixture = Path(directory) / "fixture.tsv"
        inputs = Path(directory) / "scenarios.json"
        fixture.write_bytes(original)
        inputs.write_text(json.dumps(scenarios))
        generate.regenerate(fixture, inputs)
        first = fixture.read_bytes()
        generate.regenerate(fixture, inputs)
        assert fixture.read_bytes() == first == original, "regeneration must match committed bytes"
        retained = lambda data: [line for line in data.splitlines() if not line.startswith(b"ql-")]
        assert retained(first) == retained(original), "retained provenance and checks changed"
        lines = first.decode().splitlines()
        index = next(i for i, line in enumerate(lines) if line.startswith("ql-"))
        fields = lines[index].split("\t")
        outputs = generate.attributes(fields[5])
        outputs["price"] = 123456.0
        fields[5] = generate.encode(outputs)
        lines[index] = "\t".join(fields)
        fixture.write_text("\n".join(lines) + "\n")
        generate.regenerate(fixture, inputs)
        assert fixture.read_bytes() == first, "owned prices must be recomputed"

        invalid = []
        for key in generate.INPUTS:
            case = deepcopy(scenarios)
            del case[0]["inputs"][key]
            invalid.append(case)
        invalid.append(scenarios + [scenarios[0]])
        for value in (-1, float("nan"), float("inf")):
            case = deepcopy(scenarios)
            case[0]["tolerances"]["price"] = value
            invalid.append(case)
        for field, value in (("option", "unknown"), ("spot", 0), ("rate", float("nan")),
                             ("expiry", "2025-01-06")):
            case = deepcopy(scenarios)
            case[0]["inputs"][field] = value
            invalid.append(case)
        for case in invalid:
            inputs.write_text(json.dumps(case))
            try:
                generate.regenerate(fixture, inputs)
            except ValueError:
                pass
            else:
                raise AssertionError("invalid scenario accepted")
            assert fixture.read_bytes() == first, "failure replaced fixture"

        inputs.write_text(json.dumps(scenarios))
        for invalid_output in ("price=nan", "delta=1"):
            fields[5] = invalid_output
            with patch.object(generate, "european_row", return_value="\t".join(fields)):
                try:
                    generate.regenerate(fixture, inputs)
                except ValueError:
                    pass
                else:
                    raise AssertionError("invalid output accepted")
            assert fixture.read_bytes() == first, "invalid output replaced fixture"
        real_measure = generate.measure
        for bad in (float("nan"), float("inf"), 1000.0):
            def corrupted(market, name, scale=1, price_only=False):
                if name == "vega" and scale == 2:
                    return bad
                return real_measure(market, name, scale, price_only)
            with patch.object(generate, "measure", side_effect=corrupted):
                try:
                    generate.regenerate(fixture, inputs)
                except ValueError:
                    pass
                else:
                    raise AssertionError("unstable or non-finite Greek accepted")
            assert fixture.read_bytes() == first, "bad Greek replaced fixture"

    # Exercise price-derived higher Greeks on the smooth matrix, independent of Kiyosi.
    for case in scenarios[:18]:
        for name in ("speed", "charm", "color", "vanna", "zomma"):
            direct = generate.measure(case["inputs"], name)
            fallback = generate.measure(case["inputs"], name, price_only=True)
            assert abs(direct - fallback) < 1e-7, (case["case_id"], name, direct, fallback)
    print("Generation checks passed")


if __name__ == "__main__":
    check_generation()
