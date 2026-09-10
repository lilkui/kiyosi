"""Runnable checks for regeneration and failure atomicity; standard library only."""

from copy import deepcopy
import json
from pathlib import Path
import tempfile
from unittest.mock import patch

import generate
import american
import digital
import barrier


def check_generation():
    barrier.check_bindings()
    original = generate.FIXTURE.read_bytes()
    scenarios = json.loads((generate.PROJECT / "scenarios.json").read_text())
    profiles = json.loads((generate.PROJECT / "numerical_engines.json").read_text())
    with tempfile.TemporaryDirectory() as directory:
        fixture = Path(directory) / "fixture.tsv"
        inputs = Path(directory) / "scenarios.json"
        engines = Path(directory) / "engines.json"
        fixture.write_bytes(original)
        inputs.write_text(json.dumps(scenarios))
        generate.regenerate(fixture, inputs)
        first = fixture.read_bytes()
        generate.regenerate(fixture, inputs)
        assert fixture.read_bytes() == first == original, "regeneration must match committed bytes"
        retained = lambda data: [line for line in data.splitlines() if not line.startswith(b"ql-")]
        assert retained(first) == retained(original), "retained provenance and checks changed"
        rows = [line.split("\t") for line in first.decode().splitlines() if line.startswith("ql-")]
        barrier_rows = [row for row in rows if row[1] == "BarrierOption"]
        assert len(barrier_rows) == 120
        for engine in barrier.ENGINES:
            matching = [row for row in barrier_rows if row[2] == engine]
            assert len(matching) == 60
            assert sum(generate.attributes(row[4])["wrapper"] == "true" for row in matching) == (48 if engine == barrier.ENGINES[0] else 12)
        digital_rows = [row for row in rows if row[1] in digital.INSTRUMENTS.values()]
        assert len(digital_rows) == 120, "digital matrix incomplete"
        for engine in digital.ENGINES:
            matching = [row for row in digital_rows if row[2] == engine]
            assert len(matching) == 40
            assert sum(generate.attributes(row[4])["wrapper"] == "true" for row in matching) == (4 if engine == "FiniteDifferenceDigitalEngine" else 36)
        for profile in profiles:
            matching = [row for row in rows if row[1] == "EuropeanOption" and row[2] == profile["engine"]]
            assert len(matching) == 18, "numerical matrix incomplete"
            assert sum(generate.attributes(row[4])["wrapper"] == "true" for row in matching) == 2
        american_rows = [row for row in rows if row[1] == "AmericanOption"]
        american_inputs = {case["case_id"]: case["inputs"] for case in
                           json.loads((generate.PROJECT / "american.json").read_text())["scenarios"]}
        assert len(american_rows) == 50, "American matrix incomplete"
        for engine in american.ENGINES:
            matching = [row for row in american_rows if row[2] == engine]
            assert len(matching) == 10
            assert sum(generate.attributes(row[4])["wrapper"] == "true" for row in matching) == 2
        for row in american_rows:
            attributes = generate.attributes(row[4])
            assert attributes["source_symbol"] == "QuantLib.FdBlackScholesVanillaEngine"
            assert float(attributes["refinement_price"]) <= american.STABILITY["price"]
            if float(attributes["refinement_price"]) > max(1e-9, float(attributes["coarse_refinement_price"])):
                # Grid alignment can make tiny discretization errors non-monotonic.
                # Verify another refinement against the existing uncertainty, without enlarging it.
                market = american_inputs[row[0].rsplit("-", 1)[0]]
                refined = american.measure(market, "price", (6400, 6400))
                price = float(generate.attributes(row[5])["price"])
                assert abs(refined - price) <= float(attributes["uncertainty_price"])
        # Migration is independent of the old target values and preserves other rows.
        for line in retained(first):
            fields = line.decode().split("\t")
            if fields[0] in set(american.LEGACY) | digital.LEGACY or len(fields) == 10 and fields[1] == "BarrierOption" and fields[5] != "-" and generate.attributes(fields[4]).get("monitoring") == "continuous":
                fields[5] = "price=123456"
                if fields[8] != "-":
                    parts = fields[8].split("|")
                    parts[2] = "123456"
                    fields[8] = "|".join(parts)
                assert barrier.migrate(digital.migrate(american.migrate("\t".join(fields)))) == line.decode()
                continue
            if fields[0] not in generate.LEGACY_NUMERICAL:
                assert generate.migrate_numerical(line.decode()) == line.decode()
                continue
            fields[5] = "price=123456"
            if fields[8] != "-":
                parts = fields[8].split("|")
                parts[2] = "123456"
                fields[8] = "|".join(parts)
            assert generate.migrate_numerical("\t".join(fields)) == line.decode()
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
        invalid_profiles = []
        for mutation in ("unknown engine", "unknown setting", "fractional steps", "bad shift", "missing budget"):
            changed = deepcopy(profiles)
            if mutation == "unknown engine": changed[0]["engine"] = "UnknownEngine"
            elif mutation == "unknown setting": changed[0]["settings"]["typo"] = 1
            elif mutation == "fractional steps": changed[0]["settings"]["steps"] = 1.5
            elif mutation == "bad shift": changed[0]["shifts"]["spot_shift"] = float("nan")
            else: del changed[0]["numerical_tolerances"]["zomma"]
            invalid_profiles.append(changed)
        for changed in invalid_profiles:
            engines.write_text(json.dumps(changed))
            try:
                generate.regenerate(fixture, inputs, engines)
            except ValueError:
                pass
            else:
                raise AssertionError("invalid numerical profile accepted")
            assert fixture.read_bytes() == first, "invalid profile replaced fixture"
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

        real_american_measure = american.measure
        for bad in (float("nan"), float("inf"), 1000.0):
            def corrupted_american(market, name, grid=american.GRIDS[-1], scale=1):
                if name == "vega" and scale == 2:
                    return bad
                return real_american_measure(market, name, grid, scale)
            with patch.object(american, "measure", side_effect=corrupted_american):
                try:
                    generate.regenerate(fixture, inputs)
                except ValueError:
                    pass
                else:
                    raise AssertionError("unstable American Greek accepted")
            assert fixture.read_bytes() == first, "bad American Greek replaced fixture"

        for bad in (float("nan"), float("inf"), 1000.0):
            def corrupted_digital(market, name, scale=1, price_only=False):
                if "payoff" in market and name == "vega" and scale == 2:
                    return bad
                return real_measure(market, name, scale, price_only)
            with patch.object(generate, "measure", side_effect=corrupted_digital):
                try:
                    generate.regenerate(fixture, inputs)
                except ValueError:
                    pass
                else:
                    raise AssertionError("unstable digital Greek accepted")
            assert fixture.read_bytes() == first, "bad digital Greek replaced fixture"

    # Exercise price-derived higher Greeks on the smooth matrix, independent of Kiyosi.
    for case in scenarios[:18]:
        for name in ("speed", "charm", "color", "vanna", "zomma"):
            direct = generate.measure(case["inputs"], name)
            fallback = generate.measure(case["inputs"], name, price_only=True)
            assert abs(direct - fallback) < 1e-7, (case["case_id"], name, direct, fallback)
    for identifier, market in digital.scenarios(digital.configuration()):
        if market["expiry"] == "2025-01-07":
            continue
        for name in ("speed", "charm", "color", "vanna", "zomma"):
            direct = generate.measure(market, name)
            fallback = generate.measure(market, name, price_only=True)
            assert abs(direct - fallback) < digital.STABILITY[name], (identifier, name, direct, fallback)
    # AnalyticEuropeanEngine prices pre-expiry; at expiry QuantLib marks the instrument
    # expired, so verify strict strike settlement directly through its payoff bindings.
    for direction, cash, asset in ((generate.ql.Option.Call, [0, 0, 10], [0, 0, 101]),
                                    (generate.ql.Option.Put, [10, 0, 0], [99, 0, 0])):
        assert [generate.ql.CashOrNothingPayoff(direction, 100, 10)(s) for s in (99, 100, 101)] == cash
        assert [generate.ql.AssetOrNothingPayoff(direction, 100)(s) for s in (99, 100, 101)] == asset
    print("Generation checks passed")


if __name__ == "__main__":
    check_generation()
